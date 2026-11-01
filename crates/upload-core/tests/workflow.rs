use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tempfile::TempDir;
use upload_core::{
    drivers::{LocalArchive, UploadContext, UploadDriver},
    *,
};

fn setup() -> (TempDir, Engine) {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("data"), || {}).unwrap();
    (directory, engine)
}
fn request(assets: &[Asset]) -> PublishRequest {
    PublishRequest {
        draft: Draft {
            title: "多平台创作".into(),
            body: "# 使用说明\n你好，世界。".into(),
            asset_ids: assets.iter().map(|asset| asset.id.clone()).collect(),
        },
        target_ids: vec!["local-archive".into()],
    }
}
fn import(engine: &Engine, path: &Path) -> Asset {
    let report = engine
        .import_paths(vec![path.to_string_lossy().into()])
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    report.assets[0].clone()
}
fn wait(engine: &Engine, id: &str) -> Job {
    let start = Instant::now();
    loop {
        let job = engine
            .jobs()
            .unwrap()
            .into_iter()
            .find(|job| job.id == id)
            .unwrap();
        if !job.status.is_active() {
            return job;
        }
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "job timed out: {:?}",
            job.status
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn archives_mixed_files_and_text_with_verifiable_hashes() {
    let (directory, engine) = setup();
    let mut assets = Vec::new();
    for (name, bytes, kind) in [
        ("封面.PNG", b"image-data".as_slice(), AssetKind::Image),
        ("clip.mp4", b"video-data", AssetKind::Video),
        ("weights.safetensors", b"model-data", AssetKind::Model),
        ("notes.md", b"hello", AssetKind::Text),
    ] {
        let path = directory.path().join(name);
        fs::write(&path, bytes).unwrap();
        let asset = import(&engine, &path);
        assert_eq!(asset.kind, kind);
        assets.push(asset);
    }
    let req = request(&assets);
    engine.save_draft(req.draft.clone()).unwrap();
    let jobs = engine.enqueue(req).unwrap();
    let job = wait(&engine, &jobs.last().unwrap().id);
    assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
    assert_eq!(job.completed_bytes, job.total_bytes);
    let receipt = job.receipt.unwrap();
    let destination = Path::new(&receipt.location);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["title"], "多平台创作");
    assert_eq!(manifest["body"], "# 使用说明\n你好，世界。");
    assert!(!manifest.to_string().contains("modifiedMs"));
    for (asset, file) in assets.iter().zip(receipt.files) {
        use sha2::{Digest, Sha256};
        let bytes = fs::read(destination.join(file.file_name)).unwrap();
        assert_eq!(bytes, fs::read(&asset.path).unwrap());
        assert_eq!(file.sha256, format!("{:x}", Sha256::digest(bytes)));
    }
    assert_eq!(engine.snapshot().unwrap().draft.asset_ids.len(), 4);
    assert_eq!(engine.retry(job.id).unwrap_err().code, "invalid_state");
}

#[test]
fn text_only_and_empty_files_complete_without_division_by_zero() {
    let (directory, engine) = setup();
    let jobs = engine.enqueue(request(&[])).unwrap();
    let job = wait(&engine, &jobs.last().unwrap().id);
    assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
    assert_eq!(job.total_bytes, 0);
    let path = directory.path().join("empty.txt");
    fs::write(&path, []).unwrap();
    let asset = import(&engine, &path);
    let jobs = engine.enqueue(request(&[asset])).unwrap();
    assert_eq!(
        wait(&engine, &jobs.last().unwrap().id)
            .receipt
            .unwrap()
            .files[0]
            .size,
        0
    );
}

#[test]
fn import_is_partial_deduplicated_and_rejects_directories() {
    let (directory, engine) = setup();
    let path = directory.path().join("model.gguf");
    fs::write(&path, b"model").unwrap();
    let report = engine
        .import_paths(vec![
            path.to_string_lossy().into(),
            path.to_string_lossy().into(),
            directory.path().to_string_lossy().into(),
            directory
                .path()
                .join("missing.png")
                .to_string_lossy()
                .into(),
        ])
        .unwrap();
    assert_eq!(report.assets.len(), 1);
    assert_eq!(report.errors.len(), 2);
    assert_eq!(import(&engine, &path).id, report.assets[0].id);
}

#[test]
fn invalid_requests_do_not_create_any_jobs() {
    let (_directory, engine) = setup();
    let mut req = request(&[]);
    req.target_ids.push("bilibili".into());
    assert_eq!(engine.enqueue(req).unwrap_err().code, "incompatible");
    let mut req = request(&[]);
    req.target_ids = vec!["missing-driver".into()];
    assert_eq!(engine.enqueue(req).unwrap_err().code, "unknown_target");
    let mut req = request(&[]);
    req.draft = Draft::default();
    assert_eq!(engine.enqueue(req).unwrap_err().code, "empty_publication");
    let mut req = request(&[]);
    req.draft.asset_ids.push("forged-id".into());
    assert_eq!(engine.enqueue(req).unwrap_err().code, "asset_missing");
    assert!(engine.jobs().unwrap().is_empty());
}

#[test]
fn source_mutation_is_rejected_before_submission() {
    let (directory, engine) = setup();
    let path = directory.path().join("picture.jpg");
    fs::write(&path, b"old").unwrap();
    let asset = import(&engine, &path);
    fs::write(&path, b"new-content").unwrap();
    assert_eq!(
        engine.enqueue(request(&[asset])).unwrap_err().code,
        "source_changed"
    );
    assert!(engine.jobs().unwrap().is_empty());
}

#[test]
fn cancellation_removes_partial_output_and_never_reports_success() {
    let (directory, engine) = setup();
    let path = directory.path().join("large.bin");
    fs::write(&path, vec![5u8; 3 * 1024 * 1024]).unwrap();
    let asset = import(&engine, &path);
    let cancel = AtomicBool::new(false);
    let progress = |_: u64| {
        cancel.store(true, Ordering::Release);
        Ok(())
    };
    let archive = directory.path().join("out");
    let id = uuid::Uuid::new_v4().to_string();
    let context = UploadContext {
        job_id: &id,
        operation_id: &id,
        archive_root: &archive,
        cancelled: &cancel,
        progress: &progress,
        target: None,
        workspace: None,
    };
    let result = LocalArchive.upload(
        &Publication {
            title: "title".into(),
            body: "body".into(),
            assets: vec![asset],
        },
        &context,
    );
    assert_eq!(result.unwrap_err().code, "cancelled");
    assert_eq!(fs::read_dir(archive).unwrap().count(), 0);
}

struct ControlledDriver {
    started: Arc<AtomicUsize>,
    release: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    fail: bool,
}
impl UploadDriver for ControlledDriver {
    fn id(&self) -> &'static str {
        "local-archive"
    }
    fn validate(&self, _: &Publication) -> Vec<String> {
        vec![]
    }
    fn upload(&self, _: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let count = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(count, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) && !context.cancelled.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        context.check_cancelled()?;
        if self.fail {
            return Err(AppError::new("test_failure", "受控失败"));
        }
        Ok(Receipt {
            location: "test-only".into(),
            files: vec![],
        })
    }
}

#[test]
fn queue_bounds_concurrency_and_cancels_waiting_and_running_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicUsize::new(0));
    let driver = ControlledDriver {
        started: started.clone(),
        release: release.clone(),
        active: Arc::new(AtomicUsize::new(0)),
        peak: peak.clone(),
        fail: false,
    };
    let engine = Engine::with_drivers(directory.path(), vec![Arc::new(driver)], || {}).unwrap();
    let first = engine
        .enqueue(request(&[]))
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    let second = engine
        .enqueue(request(&[]))
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    let start = Instant::now();
    while started.load(Ordering::SeqCst) != 2 {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    let third = engine
        .enqueue(request(&[]))
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    engine.cancel(third.clone()).unwrap();
    engine.cancel(first.clone()).unwrap();
    assert_eq!(wait(&engine, &first).status, JobStatus::Cancelled);
    assert_eq!(wait(&engine, &third).status, JobStatus::Cancelled);
    release.store(true, Ordering::SeqCst);
    assert_eq!(wait(&engine, &second).status, JobStatus::Succeeded);
    assert_eq!(started.load(Ordering::SeqCst), 2);
    assert_eq!(peak.load(Ordering::SeqCst), 2);
}

#[test]
fn retry_preserves_old_attempt_and_draft_persists_across_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let driver = ControlledDriver {
        started: Arc::new(AtomicUsize::new(0)),
        release: Arc::new(AtomicBool::new(true)),
        active: Arc::new(AtomicUsize::new(0)),
        peak: Arc::new(AtomicUsize::new(0)),
        fail: true,
    };
    let engine = Engine::with_drivers(directory.path(), vec![Arc::new(driver)], || {}).unwrap();
    engine.save_draft(request(&[]).draft).unwrap();
    let first = engine
        .enqueue(request(&[]))
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    assert_eq!(wait(&engine, &first).status, JobStatus::Failed);
    let second = engine
        .retry(first.clone())
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    let retry = wait(&engine, &second);
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.retry_of.as_deref(), Some(first.as_str()));
    assert_ne!(retry.id, first);
    drop(engine);
    // Simulate a task persisted as running when the application was interrupted.
    let connection =
        rusqlite::Connection::open(directory.path().join("upload-hub.sqlite3")).unwrap();
    let mut interrupted = retry;
    interrupted.status = JobStatus::Running;
    connection
        .execute(
            "UPDATE jobs SET data=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&interrupted).unwrap(), second],
        )
        .unwrap();
    drop(connection);
    let reopened = Engine::open(directory.path(), || {}).unwrap();
    let state = reopened.snapshot().unwrap();
    assert_eq!(state.draft.title, "多平台创作");
    assert_eq!(
        state
            .jobs
            .iter()
            .find(|job| job.id == second)
            .unwrap()
            .status,
        JobStatus::Interrupted
    );
    assert_eq!(
        state
            .jobs
            .iter()
            .find(|job| job.id == first)
            .unwrap()
            .status,
        JobStatus::Failed
    );
}

#[test]
fn same_file_names_from_different_directories_do_not_overwrite_each_other() {
    let (directory, engine) = setup();
    let mut assets = vec![];
    for (name, content) in [("a", b"first".as_slice()), ("b", b"second")] {
        let folder = directory.path().join(name);
        fs::create_dir(&folder).unwrap();
        let path = folder.join("photo.png");
        fs::write(&path, content).unwrap();
        assets.push(import(&engine, &path));
    }
    let id = engine
        .enqueue(request(&assets))
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    let receipt = wait(&engine, &id).receipt.unwrap();
    assert_eq!(receipt.files.len(), 2);
    assert_ne!(receipt.files[0].file_name, receipt.files[1].file_name);
    for (asset, file) in assets.iter().zip(receipt.files) {
        assert_eq!(
            fs::read(&asset.path).unwrap(),
            fs::read(Path::new(&receipt.location).join(file.file_name)).unwrap()
        );
    }
}
