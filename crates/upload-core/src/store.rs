use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

pub struct Store {
    connection: Mutex<Connection>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 2 {
            return Err(AppError::new(
                "database_version",
                "数据库来自更新版本，请升级应用",
            ));
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS assets (id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS archives (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS targets (id TEXT PRIMARY KEY, driver_id TEXT NOT NULL, name TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(driver_id,name COLLATE NOCASE));
            CREATE TABLE IF NOT EXISTS replay_plans (id TEXT PRIMARY KEY, data TEXT NOT NULL);
            PRAGMA user_version=2;")?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        // Persisted scheduled/queued jobs have not written externally and can resume.
        // In-flight uploads have uncertain remote outcomes and require explicit retry.
        for mut job in store.jobs()? {
            if matches!(job.status, JobStatus::Running | JobStatus::Cancelling)
                || (job.kind == JobKind::Legacy && job.status.is_active())
            {
                job.status = JobStatus::Interrupted;
                job.error = Some(AppError::new(
                    "interrupted",
                    "上次运行中断，请核对结果后手动重试",
                ));
                store.update_job(&job.id.clone(), |stored| *stored = job.clone())?;
            }
            if job.kind == JobKind::Legacy
                && job.driver_id == "local-archive"
                && job.status == JobStatus::Succeeded
            {
                if let Some(receipt) = job.receipt.clone() {
                    let archive = crate::archive::record_from_receipt(&job, receipt)?;
                    store.db()?.execute(
                        "INSERT OR IGNORE INTO archives(id,created_at,data) VALUES(?1,?2,?3)",
                        params![
                            archive.id,
                            archive.created_at,
                            serde_json::to_string(&archive)?
                        ],
                    )?;
                    store.update_job(&job.id, |stored| {
                        stored.kind = JobKind::Archive;
                        stored.archive_id = Some(archive.id);
                    })?;
                }
            }
        }
        for job in store.jobs()? {
            if job.status == JobStatus::WaitingArchive {
                let parent = job
                    .depends_on
                    .as_deref()
                    .map(|id| store.job(id))
                    .transpose()?;
                if parent
                    .as_ref()
                    .is_none_or(|parent| !parent.status.is_active())
                {
                    store.update_job(&job.id, |stored| {
                        stored.status = JobStatus::Failed;
                        stored.error = Some(AppError::new(
                            "archive_failed",
                            "前置归档未完成，请重试归档任务",
                        ));
                    })?;
                }
            }
        }
        Ok(store)
    }
    pub fn ensure_targets(&self, catalog: &[DriverDescriptor]) -> Result<()> {
        for driver in catalog {
            let target = UploadTarget {
                id: driver.id.clone(),
                driver_id: driver.id.clone(),
                name: if driver.id == "local-archive" {
                    "本机默认目录".into()
                } else {
                    "默认账号（待连接）".into()
                },
                created_at: now_ms(),
            };
            self.db()?.execute(
                "INSERT OR IGNORE INTO targets(id,driver_id,name,data) VALUES(?1,?2,?3,?4)",
                params![
                    target.id,
                    target.driver_id,
                    target.name,
                    serde_json::to_string(&target)?
                ],
            )?;
        }
        Ok(())
    }
    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::new("database", "数据库锁异常"))
    }
    pub fn import(&self, path: &Path) -> Result<Asset> {
        let path = path.canonicalize()?;
        let metadata = path.metadata()?;
        if !metadata.is_file() {
            return Err(AppError::new("invalid_file", "暂不支持文件夹，请选择文件"));
        }
        let path_text = path
            .to_str()
            .ok_or_else(|| AppError::new("invalid_path", "路径必须是有效 Unicode"))?
            .to_owned();
        let db = self.db()?;
        let existing: Option<String> = db
            .query_row("SELECT id FROM assets WHERE path=?1", [&path_text], |row| {
                row.get(0)
            })
            .optional()?;
        let asset = Asset {
            id: existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            kind: AssetKind::from_path(&path),
            path: path_text,
            size: metadata.len(),
            modified_ms: modified_ms(&metadata)?,
            sha256: None,
        };
        db.execute("INSERT INTO assets(id,path,data) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET data=excluded.data",
            params![asset.id, asset.path, serde_json::to_string(&asset)?])?;
        Ok(asset)
    }
    pub fn asset(&self, id: &str) -> Result<Asset> {
        let data: Option<String> = self
            .db()?
            .query_row("SELECT data FROM assets WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        serde_json::from_str(
            &data.ok_or_else(|| AppError::new("asset_missing", "附件记录不存在，请重新导入"))?,
        )
        .map_err(Into::into)
    }
    pub fn draft(&self) -> Result<Draft> {
        let data: Option<String> = self
            .db()?
            .query_row("SELECT data FROM settings WHERE key='draft'", [], |row| {
                row.get(0)
            })
            .optional()?;
        data.map(|value| serde_json::from_str(&value).map_err(Into::into))
            .unwrap_or_else(|| Ok(Draft::default()))
    }
    pub fn save_draft(&self, draft: &Draft) -> Result<()> {
        self.db()?.execute("INSERT INTO settings(key,data) VALUES('draft',?1) ON CONFLICT(key) DO UPDATE SET data=excluded.data", [serde_json::to_string(draft)?])?;
        Ok(())
    }
    pub fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let data: Option<String> = self
            .db()?
            .query_row("SELECT data FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn save_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.db()?.execute("INSERT INTO settings(key,data) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET data=excluded.data", params![key, serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn insert_jobs(&self, jobs: &[Job]) -> Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        for job in jobs {
            tx.execute(
                "INSERT INTO jobs(id,created_at,data) VALUES(?1,?2,?3)",
                params![job.id, job.created_at, serde_json::to_string(job)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        let db = self.db()?;
        let mut statement =
            db.prepare("SELECT data FROM jobs ORDER BY created_at DESC, rowid DESC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        let data: Option<String> = self
            .db()?
            .query_row("SELECT data FROM jobs WHERE id=?1", [id], |row| row.get(0))
            .optional()?;
        serde_json::from_str(&data.ok_or_else(|| AppError::new("job_missing", "任务不存在"))?)
            .map_err(Into::into)
    }
    pub fn update_job(&self, id: &str, update: impl FnOnce(&mut Job)) -> Result<Job> {
        let db = self.db()?;
        let data: String =
            db.query_row("SELECT data FROM jobs WHERE id=?1", [id], |row| row.get(0))?;
        let mut job: Job = serde_json::from_str(&data)?;
        update(&mut job);
        db.execute(
            "UPDATE jobs SET data=?2 WHERE id=?1",
            params![id, serde_json::to_string(&job)?],
        )?;
        Ok(job)
    }

    pub fn history(&self) -> Result<HistoryState> {
        let db = self.db()?;
        Ok(HistoryState {
            archives: read_json_rows(
                &db,
                "SELECT data FROM archives ORDER BY created_at DESC, rowid DESC",
            )?,
            targets: read_json_rows(&db, "SELECT data FROM targets ORDER BY rowid")?,
            plans: read_json_rows(&db, "SELECT data FROM replay_plans ORDER BY rowid DESC")?,
        })
    }
    pub fn archive(&self, id: &str) -> Result<ArchiveRecord> {
        let data: Option<String> = self
            .db()?
            .query_row("SELECT data FROM archives WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(serde_json::from_str(&data.ok_or_else(|| {
            AppError::new("archive_missing", "历史归档不存在")
        })?)?)
    }
    pub fn target(&self, id: &str) -> Result<UploadTarget> {
        let data: Option<String> = self
            .db()?
            .query_row("SELECT data FROM targets WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(serde_json::from_str(&data.ok_or_else(|| {
            AppError::new("unknown_target", "上传目标不存在")
        })?)?)
    }
    pub fn insert_target(&self, target: &UploadTarget) -> Result<()> {
        let db = self.db()?;
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM targets WHERE driver_id=?1 AND name=?2 COLLATE NOCASE)",
            params![target.driver_id, target.name],
            |row| row.get(0),
        )?;
        if exists {
            return Err(AppError::new("target_exists", "该平台已有同名账号标识"));
        }
        db.execute(
            "INSERT INTO targets(id,driver_id,name,data) VALUES(?1,?2,?3,?4)",
            params![
                target.id,
                target.driver_id,
                target.name,
                serde_json::to_string(target)?
            ],
        )?;
        Ok(())
    }
    pub fn insert_plan(&self, plan: &ReplayPlan, jobs: &[Job]) -> Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO replay_plans(id,data) VALUES(?1,?2)",
            params![plan.id, serde_json::to_string(plan)?],
        )?;
        for job in jobs {
            tx.execute(
                "INSERT INTO jobs(id,created_at,data) VALUES(?1,?2,?3)",
                params![job.id, job.created_at, serde_json::to_string(job)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_plan_status(&self, id: &str, status: PlanStatus) -> Result<Vec<String>> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let data: Option<String> = tx
            .query_row("SELECT data FROM replay_plans WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        let mut plan: ReplayPlan = serde_json::from_str(
            &data.ok_or_else(|| AppError::new("plan_missing", "补传计划不存在"))?,
        )?;
        if plan.status == PlanStatus::Cancelled {
            return Err(AppError::new(
                "invalid_state",
                "已取消的计划不能恢复，请新建计划",
            ));
        }
        plan.status = status.clone();
        tx.execute(
            "UPDATE replay_plans SET data=?2 WHERE id=?1",
            params![id, serde_json::to_string(&plan)?],
        )?;
        let mut running = Vec::new();
        if status == PlanStatus::Cancelled {
            for mut job in read_json_rows::<Job>(&tx, "SELECT data FROM jobs")? {
                if job.plan_id.as_deref() != Some(id) || !job.status.is_active() {
                    continue;
                }
                if matches!(job.status, JobStatus::Running | JobStatus::Cancelling) {
                    job.status = JobStatus::Cancelling;
                    running.push(job.id.clone());
                } else {
                    job.status = JobStatus::Cancelled;
                    job.error = Some(AppError::cancelled());
                }
                write_job(&tx, &job)?;
            }
        }
        tx.commit()?;
        Ok(running)
    }
    /// Atomically claims at most one due task and advances that plan's real-time gate.
    pub fn claim_next(&self, now: u64) -> Result<Option<Job>> {
        let mut db = self.db()?;
        let mut jobs = read_json_rows::<Job>(&db, "SELECT data FROM jobs ORDER BY rowid")?;
        let mut plans = read_json_rows::<ReplayPlan>(&db, "SELECT data FROM replay_plans")?;
        jobs.sort_by_key(|job| job.scheduled_at.unwrap_or(job.created_at));
        for mut job in jobs {
            if !matches!(job.status, JobStatus::Scheduled | JobStatus::Queued)
                || job.scheduled_at.is_some_and(|time| time > now)
            {
                continue;
            }
            let plan = match &job.plan_id {
                Some(id) => {
                    let Some(plan) = plans.iter_mut().find(|plan| &plan.id == id) else {
                        continue;
                    };
                    if plan.status != PlanStatus::Active || plan.next_release_at > now {
                        continue;
                    }
                    Some(plan)
                }
                None => None,
            };
            let tx = db.transaction()?;
            job.status = JobStatus::Running;
            write_job(&tx, &job)?;
            if let Some(plan) = plan {
                plan.next_release_at =
                    now.saturating_add(u64::from(plan.interval_minutes) * 60_000);
                tx.execute(
                    "UPDATE replay_plans SET data=?2 WHERE id=?1",
                    params![plan.id, serde_json::to_string(plan)?],
                )?;
            }
            tx.commit()?;
            return Ok(Some(job));
        }
        Ok(None)
    }
    /// Archive index, successful parent, and dependent jobs become visible together.
    pub fn finish_job(
        &self,
        id: &str,
        result: Result<Receipt>,
        archive: Option<ArchiveRecord>,
        interrupted: bool,
    ) -> Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let data: String =
            tx.query_row("SELECT data FROM jobs WHERE id=?1", [id], |row| row.get(0))?;
        let mut job: Job = serde_json::from_str(&data)?;
        match result {
            Ok(receipt) => {
                job.status = JobStatus::Succeeded;
                job.completed_bytes = job.total_bytes;
                job.receipt = Some(receipt);
            }
            Err(error) => {
                job.status = if interrupted {
                    JobStatus::Interrupted
                } else if error.code == "cancelled" {
                    JobStatus::Cancelled
                } else {
                    JobStatus::Failed
                };
                job.error = Some(error);
            }
        }
        if let Some(archive) = &archive {
            tx.execute(
                "INSERT INTO archives(id,created_at,data) VALUES(?1,?2,?3)",
                params![
                    archive.id,
                    archive.created_at,
                    serde_json::to_string(archive)?
                ],
            )?;
            job.archive_id = Some(archive.id.clone());
            job.publication = archive.publication.clone();
        }
        write_job(&tx, &job)?;
        if job.kind == JobKind::Archive {
            for mut dependent in read_json_rows::<Job>(&tx, "SELECT data FROM jobs")? {
                if dependent.depends_on.as_deref() != Some(id)
                    || dependent.status != JobStatus::WaitingArchive
                {
                    continue;
                }
                if let Some(archive) = &archive {
                    dependent.archive_id = Some(archive.id.clone());
                    dependent.publication = archive.publication.clone();
                    dependent.status = JobStatus::Queued;
                } else {
                    dependent.status = JobStatus::Failed;
                    dependent.error = Some(AppError::new(
                        "archive_failed",
                        "本地归档未完成，已阻止上传。请重试归档任务。",
                    ));
                }
                write_job(&tx, &dependent)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn read_json_rows<T: serde::de::DeserializeOwned>(db: &Connection, sql: &str) -> Result<Vec<T>> {
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn write_job(db: &Connection, job: &Job) -> Result<()> {
    db.execute(
        "UPDATE jobs SET data=?2 WHERE id=?1",
        params![job.id, serde_json::to_string(job)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scheduled(store: &Store) -> ReplayPlan {
        let plan = ReplayPlan {
            id: "plan".into(),
            target_id: "local-archive".into(),
            created_at: 1000,
            start_at: 10_000,
            interval_minutes: 1,
            status: PlanStatus::Active,
            next_release_at: 10_000,
            archive_ids: vec!["a".into(), "b".into(), "c".into()],
            force: false,
        };
        let jobs: Vec<_> = plan
            .archive_ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let mut job = crate::new_job(
                    Publication {
                        title: id.clone(),
                        body: String::new(),
                        assets: vec![],
                    },
                    &plan.id,
                    "local-archive",
                    JobKind::Upload,
                );
                job.plan_id = Some(plan.id.clone());
                job.archive_id = Some(id.clone());
                job.target_id = Some(plan.target_id.clone());
                job.status = JobStatus::Scheduled;
                job.scheduled_at = Some(10_000 + index as u64 * 60_000);
                job
            })
            .collect();
        store.insert_plan(&plan, &jobs).unwrap();
        plan
    }
    #[test]
    fn overdue_history_keeps_real_intervals_after_restart_and_pause() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("queue.sqlite3");
        let store = Store::open(&path).unwrap();
        let plan = scheduled(&store);
        assert!(
            store.claim_next(9_999).unwrap().is_none(),
            "never before scheduled start"
        );
        let first = store.claim_next(1_000_000).unwrap().unwrap();
        assert_eq!(first.publication.title, "a");
        assert!(
            store.claim_next(1_000_000).unwrap().is_none(),
            "second worker must not burst overdue items"
        );
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.job(&first.id).unwrap().status, JobStatus::Interrupted);
        assert!(
            store.claim_next(1_059_999).unwrap().is_none(),
            "actual dispatch gate persists across restart"
        );
        store.set_plan_status(&plan.id, PlanStatus::Paused).unwrap();
        assert!(store.claim_next(2_000_000).unwrap().is_none());
        store.set_plan_status(&plan.id, PlanStatus::Active).unwrap();
        assert_eq!(
            store
                .claim_next(2_000_000)
                .unwrap()
                .unwrap()
                .publication
                .title,
            "b"
        );
        assert!(store.claim_next(2_059_999).unwrap().is_none());
        assert_eq!(
            store
                .claim_next(2_060_000)
                .unwrap()
                .unwrap()
                .publication
                .title,
            "c"
        );
    }
    #[test]
    fn cancelling_plan_cancels_pending_and_requests_running_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("queue.sqlite3")).unwrap();
        let plan = scheduled(&store);
        let running = store.claim_next(10_000).unwrap().unwrap();
        let signals = store
            .set_plan_status(&plan.id, PlanStatus::Cancelled)
            .unwrap();
        assert_eq!(signals, vec![running.id.clone()]);
        let jobs = store.jobs().unwrap();
        assert_eq!(
            jobs.iter()
                .filter(|job| job.status == JobStatus::Cancelled)
                .count(),
            2
        );
        assert_eq!(
            store.job(&running.id).unwrap().status,
            JobStatus::Cancelling
        );
        assert!(store.claim_next(10_000_000).unwrap().is_none());
        assert_eq!(
            store
                .set_plan_status(&plan.id, PlanStatus::Active)
                .unwrap_err()
                .code,
            "invalid_state"
        );
    }
}
