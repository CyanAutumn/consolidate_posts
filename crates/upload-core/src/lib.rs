mod archive;
pub mod drivers;
pub mod model;
pub mod platform_accounts;
pub mod plugins;
mod replay;
mod store;
pub mod workspaces;

use drivers::{LocalArchive, UploadContext, UploadDriver};
pub use model::*;
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};
use store::Store;

const CONCURRENCY: usize = 2;
type Notifier = Arc<dyn Fn() + Send + Sync>;
struct Inner {
    store: Store,
    data_dir: PathBuf,
    drivers: HashMap<String, Arc<dyn UploadDriver>>,
    catalog: Vec<DriverDescriptor>,
    plugin_directory: PathBuf,
    plugin_reports: Vec<plugins::PluginReport>,
    tokens: Mutex<HashMap<String, Arc<AtomicBool>>>,
    submission: Mutex<()>,
    notify: Notifier,
    shutdown: AtomicBool,
    wake: (Mutex<()>, Condvar),
    // Held until workers and SQLite have closed; prevents duplicate dispatch by another process.
    _lease: File,
}
struct Runtime {
    inner: Arc<Inner>,
    workers: Vec<JoinHandle<()>>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::Release);
        if let Ok(tokens) = self.inner.tokens.lock() {
            for token in tokens.values() {
                token.store(true, Ordering::Release);
            }
        }
        self.inner.wake.1.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
    _runtime: Arc<Runtime>,
}

impl Engine {
    pub fn open(
        data_dir: impl AsRef<Path>,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::with_plugin_dir(data_dir, plugins::default_directory(), notify)
    }
    pub fn with_plugin_dir(
        data_dir: impl AsRef<Path>,
        plugin_dir: impl AsRef<Path>,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::with_registry(
            data_dir,
            plugins::Registry::load(plugin_dir.as_ref()),
            notify,
        )
    }
    pub fn with_drivers(
        data_dir: impl AsRef<Path>,
        drivers: Vec<Arc<dyn UploadDriver>>,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::with_registry(
            data_dir,
            plugins::Registry {
                directory: PathBuf::new(),
                catalog: drivers::catalog(),
                reports: vec![],
                drivers: drivers
                    .into_iter()
                    .map(|driver| (driver.id().into(), driver))
                    .collect(),
            },
            notify,
        )
    }
    fn with_registry(
        data_dir: impl AsRef<Path>,
        registry: plugins::Registry,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let data_dir = data_dir.as_ref().canonicalize()?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data_dir.join("workspace.lock"))?;
        lease.try_lock().map_err(|_| {
            AppError::new(
                "workspace_busy",
                "这个数据目录已由另一个 Upload Hub 实例使用",
            )
        })?;
        let store = Store::open(&data_dir.join("upload-hub.sqlite3"))?;
        store.ensure_targets(&registry.catalog)?;
        let inner = Arc::new(Inner {
            store,
            data_dir,
            drivers: registry.drivers,
            catalog: registry.catalog,
            plugin_directory: registry.directory,
            plugin_reports: registry.reports,
            tokens: Mutex::new(HashMap::new()),
            submission: Mutex::new(()),
            notify: Arc::new(notify),
            shutdown: AtomicBool::new(false),
            wake: (Mutex::new(()), Condvar::new()),
            _lease: lease,
        });
        let mut runtime = Runtime {
            inner: inner.clone(),
            workers: Vec::new(),
        };
        for worker in 0..CONCURRENCY {
            let inner = inner.clone();
            runtime.workers.push(
                std::thread::Builder::new()
                    .name(format!("upload-worker-{worker}"))
                    .spawn(move || inner.work())?,
            );
        }
        Ok(Self {
            inner,
            _runtime: Arc::new(runtime),
        })
    }
    pub fn import_paths(&self, paths: Vec<String>) -> Result<ImportReport> {
        if paths.len() > 100 {
            return Err(AppError::new("limit", "每次最多导入 100 个文件"));
        }
        let mut report = ImportReport {
            assets: Vec::new(),
            errors: Vec::new(),
        };
        for path in paths {
            match self.inner.store.import(Path::new(&path)) {
                Ok(asset) if !report.assets.iter().any(|item| item.id == asset.id) => {
                    report.assets.push(asset)
                }
                Ok(_) => {}
                Err(error) => report.errors.push(ImportFailure {
                    path,
                    message: error.message,
                }),
            }
        }
        Ok(report)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let draft = self.inner.store.draft()?;
        let assets = draft
            .asset_ids
            .iter()
            .map(|id| self.inner.store.asset(id))
            .collect::<Result<Vec<_>>>()?;
        Ok(Snapshot {
            platform_accounts: self.platform_accounts()?,
            drivers: self.inner.catalog.clone(),
            plugin_dir: self.inner.plugin_directory.to_string_lossy().into(),
            plugin_reports: self.inner.plugin_reports.clone(),
            draft,
            assets,
            jobs: self.jobs()?,
            history: self.history()?,
            data_dir: self.inner.data_dir.to_string_lossy().into(),
            archive_dir: self
                .inner
                .data_dir
                .join("archives")
                .to_string_lossy()
                .into(),
            concurrency: CONCURRENCY,
        })
    }
    pub fn history(&self) -> Result<HistoryState> {
        self.inner.store.history()
    }
    pub fn save_draft(&self, draft: Draft) -> Result<()> {
        self.resolve(&draft, false)?;
        self.inner.store.save_draft(&draft)
    }
    fn resolve(&self, draft: &Draft, require_content: bool) -> Result<Publication> {
        if draft.title.chars().count() > 300
            || draft.body.chars().count() > 100_000
            || draft.asset_ids.len() > 100
        {
            return Err(AppError::new(
                "limit",
                "标题最多 300 字、正文最多 10 万字、附件最多 100 个",
            ));
        }
        let mut seen = HashSet::new();
        let assets = draft
            .asset_ids
            .iter()
            .filter(|id| seen.insert(*id))
            .map(|id| self.inner.store.asset(id))
            .collect::<Result<Vec<_>>>()?;
        if require_content
            && assets.is_empty()
            && draft.title.trim().is_empty()
            && draft.body.trim().is_empty()
        {
            return Err(AppError::new("empty_publication", "请添加附件、标题或正文"));
        }
        Ok(Publication {
            title: draft.title.clone(),
            body: draft.body.clone(),
            assets,
        })
    }
    pub fn preflight(&self, draft: Draft) -> Result<Vec<Validation>> {
        let publication = self.resolve(&draft, true)?;
        for asset in &publication.assets {
            verify_asset(asset)?;
        }
        Ok(self.validate(&publication))
    }
    fn validate(&self, publication: &Publication) -> Vec<Validation> {
        self.inner
            .catalog
            .iter()
            .filter(|descriptor| self.require_enabled(&descriptor.id).is_ok())
            .map(|descriptor| self.validation_for(descriptor, publication))
            .collect()
    }
    fn validation_for(
        &self,
        descriptor: &DriverDescriptor,
        publication: &Publication,
    ) -> Validation {
        let mut issues = drivers::capability_issues(descriptor, publication);
        if let Err(error) = self.require_enabled(&descriptor.id) {
            issues.push(error.message);
        }
        if issues.is_empty() {
            match self.inner.drivers.get(&descriptor.id) {
                Some(driver) => issues.extend(driver.validate(publication)),
                None => issues.push("插件未加载，请检查插件目录".into()),
            }
        }
        Validation {
            driver_id: descriptor.id.clone(),
            eligible: issues.is_empty(),
            issues,
        }
    }
    fn validate_target(&self, target: &UploadTarget, publication: &Publication) -> Result<()> {
        self.require_enabled(&target.driver_id)?;
        let descriptor = self
            .inner
            .catalog
            .iter()
            .find(|driver| driver.id == target.driver_id)
            .ok_or_else(|| AppError::new("unknown_driver", "目标插件未加载，请检查插件目录"))?;
        let check = self.validation_for(descriptor, publication);
        if !check.eligible {
            return Err(AppError::new("incompatible", check.issues.join("；")));
        }
        Ok(())
    }
    pub fn create_target(&self, driver_id: String, name: String) -> Result<UploadTarget> {
        let _submission = self
            .inner
            .submission
            .lock()
            .map_err(|_| AppError::new("queue", "提交锁异常"))?;
        self.require_enabled(&driver_id)?;
        if !self
            .inner
            .catalog
            .iter()
            .any(|driver| driver.id == driver_id)
        {
            return Err(AppError::new("unknown_driver", "平台驱动不存在"));
        }
        let name = name.trim().to_string();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(AppError::new("invalid_name", "账号标识请填写 1–80 个字符"));
        }
        let target = UploadTarget {
            id: uuid::Uuid::new_v4().to_string(),
            driver_id,
            name,
            created_at: now_ms(),
        };
        self.inner.store.insert_target(&target)?;
        self.inner.changed();
        Ok(target)
    }
    pub fn enqueue(&self, request: PublishRequest) -> Result<Vec<Job>> {
        let _submission = self
            .inner
            .submission
            .lock()
            .map_err(|_| AppError::new("queue", "提交锁异常"))?;
        self.submit_publication(
            self.resolve(&request.draft, true)?,
            request.target_ids,
            None,
            1,
        )
    }
    fn submit_publication(
        &self,
        publication: Publication,
        target_ids: Vec<String>,
        retry_of: Option<String>,
        attempt: u32,
    ) -> Result<Vec<Job>> {
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for id in target_ids {
            let target = self.platform_target(&id)?;
            if !seen.insert(target.id.clone()) {
                continue;
            }
            self.validate_target(&target, &publication)?;
            targets.push(target);
        }
        for asset in &publication.assets {
            verify_asset(asset)?;
        }
        let batch_id = uuid::Uuid::new_v4().to_string();
        let mut archive = new_job(
            publication.clone(),
            &batch_id,
            "local-archive",
            JobKind::Archive,
        );
        archive.retry_of = retry_of;
        archive.attempt = attempt;
        let archive_id = archive.id.clone();
        let mut jobs = vec![archive];
        for target in targets {
            let mut job = new_job(
                publication.clone(),
                &batch_id,
                &target.driver_id,
                JobKind::Upload,
            );
            job.target_id = Some(target.id);
            job.depends_on = Some(archive_id.clone());
            job.status = JobStatus::WaitingArchive;
            jobs.push(job);
        }
        self.inner.store.insert_jobs(&jobs)?;
        self.inner.changed();
        Ok(jobs)
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        self.inner.store.jobs()
    }
    pub fn cancel(&self, id: String) -> Result<()> {
        let tokens = self
            .inner
            .tokens
            .lock()
            .map_err(|_| AppError::new("queue", "队列锁异常"))?;
        self.inner.store.update_job(&id, |job| match job.status {
            JobStatus::Scheduled | JobStatus::WaitingArchive | JobStatus::Queued => {
                job.status = JobStatus::Cancelled;
                job.error = Some(AppError::cancelled());
            }
            JobStatus::Running | JobStatus::Cancelling => {
                if let Some(token) = tokens.get(&id) {
                    token.store(true, Ordering::Release);
                }
                job.status = JobStatus::Cancelling;
            }
            _ => {}
        })?;
        let parent = self.inner.store.job(&id)?;
        if parent.kind == JobKind::Archive && parent.status == JobStatus::Cancelled {
            self.inner
                .store
                .finish_job(&id, Err(AppError::cancelled()), None, false)?;
        }
        self.inner.changed();
        Ok(())
    }
    pub fn retry(&self, id: String) -> Result<Vec<Job>> {
        let _submission = self
            .inner
            .submission
            .lock()
            .map_err(|_| AppError::new("queue", "提交锁异常"))?;
        let old = self.inner.store.job(&id)?;
        if !matches!(
            old.status,
            JobStatus::Failed | JobStatus::Cancelled | JobStatus::Interrupted
        ) {
            return Err(AppError::new(
                "invalid_state",
                "只允许重试失败、取消或中断的任务",
            ));
        }
        if old.kind == JobKind::Archive || old.kind == JobKind::Legacy {
            if self.jobs()?.iter().any(|job| {
                job.retry_of.as_deref() == Some(&id)
                    && (job.status.is_active() || job.status == JobStatus::Succeeded)
            }) {
                return Err(AppError::new(
                    "already_retried",
                    "这个归档已有执行中或已完成的重试，请查看最新任务",
                ));
            }
            let targets = if old.kind == JobKind::Legacy {
                vec![old.driver_id.clone()]
            } else {
                self.jobs()?
                    .into_iter()
                    .filter(|job| {
                        job.depends_on.as_deref() == Some(&id) && job.status != JobStatus::Cancelled
                    })
                    .filter_map(|job| job.target_id)
                    .collect()
            };
            return self.submit_publication(old.publication, targets, Some(id), old.attempt + 1);
        }
        let archive_id = old
            .archive_id
            .as_ref()
            .ok_or_else(|| AppError::new("archive_failed", "请先重试前置归档任务"))?;
        let archive = self.inner.store.archive(archive_id)?;
        let mut target = self.inner.store.target(
            old.target_id
                .as_deref()
                .ok_or_else(|| AppError::new("unknown_target", "任务缺少上传目标"))?,
        )?;
        // A configuration failure happens before a remote submission or journal
        // exists. Retrying that legacy empty target may use the platform login.
        if old
            .error
            .as_ref()
            .is_some_and(|e| matches!(e.code.as_str(), "pixiv_config" | "x_config"))
        {
            let primary = self.platform_target(&target.id)?;
            if primary.id != target.id {
                let mut attempt = old.clone();
                let mut target_ids = vec![target.id.clone()];
                let mut visited = HashSet::new();
                while let Some(previous) = &attempt.retry_of {
                    if !visited.insert(previous.clone()) {
                        return Err(AppError::new("invalid_retry", "重试记录异常"));
                    }
                    attempt = self.inner.store.job(previous)?;
                    if let Some(id) = &attempt.target_id {
                        target_ids.push(id.clone());
                    }
                }
                let has_journal = target_ids.iter().any(|id| {
                    self.inner
                        .data_dir
                        .join("plugin-data")
                        .join(&target.driver_id)
                        .join(id)
                        .join("submissions")
                        .join(format!("{}.json", attempt.id))
                        .exists()
                });
                if has_journal {
                    return Err(AppError::new(
                        "submission_uncertain",
                        "原任务已有投稿记录，请先核对平台结果，避免重复发布",
                    ));
                }
            }
            target = primary;
        }
        self.validate_target(&target, &archive.publication)?;
        if self.jobs()?.iter().any(|job| {
            job.id != old.id
                && job.archive_id.as_ref() == Some(archive_id)
                && (job.target_id.as_deref() == Some(&target.id)
                    || (matches!(target.driver_id.as_str(), "pixiv" | "x")
                        && job.driver_id == target.driver_id))
                && (job.status.is_active() || job.status == JobStatus::Succeeded)
        }) {
            return Err(AppError::new(
                "already_delivered",
                "这条归档已上传或已排队，请在历史补传中管理",
            ));
        }
        for asset in &archive.publication.assets {
            verify_asset(asset)?;
        }
        let mut job = new_job(
            archive.publication,
            &uuid::Uuid::new_v4().to_string(),
            &target.driver_id,
            JobKind::Upload,
        );
        job.target_id = Some(target.id);
        job.archive_id = Some(archive.id);
        job.retry_of = Some(id);
        job.attempt = old.attempt + 1;
        self.inner.store.insert_jobs(std::slice::from_ref(&job))?;
        self.inner.changed();
        Ok(vec![job])
    }
}

fn new_job(publication: Publication, batch_id: &str, driver_id: &str, kind: JobKind) -> Job {
    Job {
        id: uuid::Uuid::new_v4().to_string(),
        batch_id: batch_id.into(),
        driver_id: driver_id.into(),
        kind,
        status: JobStatus::Queued,
        completed_bytes: 0,
        total_bytes: publication.assets.iter().map(|asset| asset.size).sum(),
        created_at: now_ms(),
        attempt: 1,
        retry_of: None,
        error: None,
        receipt: None,
        publication,
        target_id: None,
        archive_id: None,
        depends_on: None,
        plan_id: None,
        scheduled_at: None,
    }
}
impl Inner {
    fn changed(&self) {
        self.wake.1.notify_all();
        (self.notify)();
    }
    fn work(&self) {
        while !self.shutdown.load(Ordering::Acquire) {
            let claimed = (|| -> Result<Option<(Job, Arc<AtomicBool>)>> {
                let mut tokens = self
                    .tokens
                    .lock()
                    .map_err(|_| AppError::new("queue", "队列锁异常"))?;
                if self.shutdown.load(Ordering::Acquire) {
                    return Ok(None);
                }
                let Some(job) = self.store.claim_next(now_ms())? else {
                    return Ok(None);
                };
                let token = Arc::new(AtomicBool::new(false));
                tokens.insert(job.id.clone(), token.clone());
                Ok(Some((job, token)))
            })();
            match claimed {
                Ok(Some((job, token))) => {
                    if let Err(error) = self.execute(&job, &token) {
                        eprintln!("任务 {} 执行失败：{error}", job.id);
                        let _ = self.store.finish_job(&job.id, Err(error), None, false);
                    }
                    if let Ok(mut tokens) = self.tokens.lock() {
                        tokens.remove(&job.id);
                    }
                    self.changed();
                    continue;
                }
                Err(error) => eprintln!("任务调度失败：{error}"),
                Ok(None) => {}
            }
            if let Ok(gate) = self.wake.0.lock() {
                if !self.shutdown.load(Ordering::Acquire) {
                    let _ = self.wake.1.wait_timeout(gate, Duration::from_millis(500));
                }
            }
        }
    }
    fn execute(&self, job: &Job, token: &AtomicBool) -> Result<()> {
        self.changed();
        let report = |bytes| -> Result<()> {
            self.store
                .update_job(&job.id, |job| job.completed_bytes = bytes)?;
            (self.notify)();
            Ok(())
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<(Receipt, Option<ArchiveRecord>)> {
                let target = job
                    .target_id
                    .as_deref()
                    .map(|id| self.store.target(id))
                    .transpose()?;
                let root = if job.kind == JobKind::Archive {
                    self.data_dir.join("archives")
                } else {
                    self.data_dir.join("deliveries").join(
                        &target
                            .as_ref()
                            .ok_or_else(|| AppError::new("unknown_target", "缺少上传目标"))?
                            .id,
                    )
                };
                let workspace = self
                    .store
                    .setting::<workspaces::WorkspaceProfile>("workspace")?
                    .map(|profile| PluginWorkspace {
                        id: profile.id,
                        name: profile.config.name,
                        content_rating: profile.config.content_rating,
                        data_dir: self
                            .data_dir
                            .join("plugin-data")
                            .join(&job.driver_id)
                            .to_string_lossy()
                            .into(),
                    });
                if job.kind != JobKind::Archive {
                    if let Some(workspace) = &workspace {
                        std::fs::create_dir_all(&workspace.data_dir)?;
                    }
                }
                let mut original = job.clone();
                let mut visited = std::collections::HashSet::new();
                while let Some(previous) = &original.retry_of {
                    if !visited.insert(previous.clone()) {
                        return Err(AppError::new("invalid_retry", "重试链包含循环"));
                    }
                    original = self.store.job(previous)?;
                }
                let context = UploadContext {
                    job_id: &job.id,
                    operation_id: &original.id,
                    archive_root: &root,
                    cancelled: token,
                    progress: &report,
                    target: target.as_ref(),
                    workspace: workspace.as_ref(),
                };
                context.check_cancelled()?;
                if job.kind == JobKind::Archive {
                    let receipt = LocalArchive.upload(&job.publication, &context)?;
                    let archive = archive::record_from_receipt(job, receipt.clone())?;
                    Ok((receipt, Some(archive)))
                } else {
                    archive::verify_contents(&job.publication, token)?;
                    let driver = self
                        .drivers
                        .get(&job.driver_id)
                        .ok_or_else(|| AppError::new("unknown_driver", "驱动未注册"))?;
                    Ok((driver.upload(&job.publication, &context)?, None))
                }
            },
        ))
        .unwrap_or_else(|_| Err(AppError::new("driver_panic", "驱动意外终止")));
        let (receipt, archive) = match result {
            Ok((receipt, archive)) => (Ok(receipt), archive),
            Err(error) => (Err(error), None),
        };
        let interrupted = self.shutdown.load(Ordering::Acquire)
            && receipt
                .as_ref()
                .is_err_and(|error| error.code == "cancelled");
        self.store
            .finish_job(&job.id, receipt, archive, interrupted)
    }
}
