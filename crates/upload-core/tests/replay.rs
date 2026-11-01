use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use upload_core::{
    drivers::{UploadContext, UploadDriver},
    *,
};

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
            "task did not finish: {job:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn archive(engine: &Engine, title: &str) -> ArchiveRecord {
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: title.into(),
                body: "历史正文".into(),
                asset_ids: vec![],
            },
            target_ids: vec![],
        })
        .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(wait(engine, &jobs[0].id).status, JobStatus::Succeeded);
    engine
        .history()
        .unwrap()
        .archives
        .into_iter()
        .find(|record| record.id == jobs[0].id)
        .unwrap()
}
fn request(ids: Vec<String>, target: &str) -> ReplayRequest {
    ReplayRequest {
        archive_ids: ids,
        target_id: target.into(),
        start_at: now_ms(),
        interval_minutes: 0,
        force: false,
    }
}
fn wait_plan(engine: &Engine, plan: &ReplayPlan) -> Vec<Job> {
    let jobs: Vec<_> = engine
        .jobs()
        .unwrap()
        .into_iter()
        .filter(|job| job.plan_id.as_deref() == Some(&plan.id))
        .collect();
    jobs.into_iter().map(|job| wait(engine, &job.id)).collect()
}
struct FailingDriver;
impl UploadDriver for FailingDriver {
    fn id(&self) -> &'static str {
        "local-archive"
    }
    fn validate(&self, _: &Publication) -> Vec<String> {
        vec![]
    }
    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        assert_eq!(context.target.unwrap().id, "local-archive");
        assert!(publication.assets[0].path.contains("archives"));
        assert!(publication.assets[0].sha256.is_some());
        Err(AppError::new("remote_failure", "模拟平台上传失败"))
    }
}

#[test]
fn archive_survives_delivery_failure_and_deleted_source_then_retries_from_copy() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");
    let source = directory.path().join("原始模型.safetensors");
    fs::write(&source, b"model-content").unwrap();
    let engine = Engine::with_drivers(&data, vec![Arc::new(FailingDriver)], || {}).unwrap();
    let asset = engine
        .import_paths(vec![source.to_string_lossy().into()])
        .unwrap()
        .assets
        .remove(0);
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "旧账号作品".into(),
                body: "完整说明".into(),
                asset_ids: vec![asset.id],
            },
            target_ids: vec!["local-archive".into()],
        })
        .unwrap();
    assert_eq!(wait(&engine, &jobs[1].id).status, JobStatus::Failed);
    let history = engine.history().unwrap();
    assert_eq!(history.archives.len(), 1);
    let record = &history.archives[0];
    assert_eq!(record.publication.title, "旧账号作品");
    assert_eq!(record.publication.body, "完整说明");
    assert_eq!(
        fs::read(&record.publication.assets[0].path).unwrap(),
        b"model-content"
    );
    fs::remove_file(source).unwrap();
    drop(engine);
    let engine = Engine::open(&data, || {}).unwrap();
    let retried = engine.retry(jobs[1].id.clone()).unwrap();
    let done = wait(&engine, &retried[0].id);
    assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
    assert_eq!(done.archive_id, Some(record.id.clone()));
    let receipt = done.receipt.unwrap();
    assert_eq!(
        fs::read(Path::new(&receipt.location).join(&receipt.files[0].file_name)).unwrap(),
        b"model-content"
    );
    assert_eq!(engine.history().unwrap().archives.len(), 1);
}

#[test]
fn all_history_replays_oldest_first_and_deduplicates_per_account() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path(), || {}).unwrap();
    let first = archive(&engine, "第一篇");
    let second = archive(&engine, "第二篇");
    let target = engine
        .create_target("local-archive".into(), " 新账号 ".into())
        .unwrap();
    assert_eq!(target.name, "新账号");
    assert_eq!(
        engine
            .create_target("local-archive".into(), "新账号".into())
            .unwrap_err()
            .code,
        "target_exists"
    );
    let mut req = request(
        vec![second.id.clone(), first.id.clone(), first.id.clone()],
        &target.id,
    );
    req.start_at = now_ms() + 60_000;
    req.interval_minutes = 60;
    let preview = engine.preview_replay(req.clone()).unwrap();
    assert_eq!(preview.eligible_count, 2);
    assert_eq!(preview.items[0].archive_id, first.id);
    assert_eq!(
        preview.items[1].scheduled_at.unwrap() - preview.items[0].scheduled_at.unwrap(),
        3_600_000
    );
    let planned = engine.schedule_replay(req.clone()).unwrap();
    assert_eq!(
        engine.preview_replay(req.clone()).unwrap().eligible_count,
        0
    );
    req.force = true;
    assert_eq!(
        engine.preview_replay(req.clone()).unwrap().eligible_count,
        0,
        "force must not duplicate pending jobs"
    );
    assert_eq!(
        engine.schedule_replay(req.clone()).unwrap_err().code,
        "nothing_to_replay"
    );
    engine
        .set_plan_status(planned.plan.id, PlanStatus::Cancelled)
        .unwrap();
    req.start_at = now_ms();
    req.interval_minutes = 0;
    req.force = false;
    let planned = engine.schedule_replay(req.clone()).unwrap();
    let done = wait_plan(&engine, &planned.plan);
    assert!(done.iter().all(|job| job.status == JobStatus::Succeeded));
    assert!(done
        .iter()
        .all(|job| job.receipt.as_ref().unwrap().location.contains(&target.id)));
    assert_eq!(
        engine.history().unwrap().archives.len(),
        2,
        "replay must reuse archives"
    );
    assert_eq!(
        engine.preview_replay(req.clone()).unwrap().eligible_count,
        0
    );
    req.force = true;
    assert_eq!(
        engine.preview_replay(req.clone()).unwrap().eligible_count,
        2
    );
    req.force = false;
    req.target_id = "local-archive".into();
    assert_eq!(
        engine.preview_replay(req).unwrap().eligible_count,
        2,
        "another account has independent delivery history"
    );
}

#[test]
fn archive_failure_blocks_upload_and_can_be_retried() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path(), || {}).unwrap();
    let blocked = directory.path().join("archives");
    fs::write(&blocked, b"prevents creation of archive directory").unwrap();
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "不可丢失".into(),
                ..Draft::default()
            },
            target_ids: vec!["local-archive".into()],
        })
        .unwrap();
    assert_eq!(wait(&engine, &jobs[0].id).status, JobStatus::Failed);
    assert_eq!(
        wait(&engine, &jobs[1].id).error.unwrap().code,
        "archive_failed"
    );
    assert!(engine.history().unwrap().archives.is_empty());
    assert!(!directory.path().join("deliveries").exists());
    fs::remove_file(blocked).unwrap();
    let retry = engine.retry(jobs[0].id.clone()).unwrap();
    assert_eq!(retry.len(), 2);
    assert_eq!(wait(&engine, &retry[1].id).status, JobStatus::Succeeded);
    assert_eq!(engine.history().unwrap().archives.len(), 1);
    assert_eq!(
        engine.retry(jobs[0].id.clone()).unwrap_err().code,
        "already_retried"
    );
}

#[test]
fn scheduled_plan_and_pause_survive_restart_then_resume_without_rearchiving() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path(), || {}).unwrap();
    let archived = archive(&engine, "定时补传");
    let mut req = request(vec![archived.id], "local-archive");
    req.start_at += 1_000;
    let plan = engine.schedule_replay(req).unwrap().plan;
    engine
        .set_plan_status(plan.id.clone(), PlanStatus::Paused)
        .unwrap();
    drop(engine);
    std::thread::sleep(Duration::from_millis(1_100));
    let engine = Engine::open(directory.path(), || {}).unwrap();
    let history = engine.history().unwrap();
    assert_eq!(history.plans[0].status, PlanStatus::Paused);
    let jobs = engine.jobs().unwrap();
    assert_eq!(
        jobs.iter()
            .find(|job| job.plan_id.as_deref() == Some(&plan.id))
            .unwrap()
            .status,
        JobStatus::Scheduled
    );
    engine
        .set_plan_status(plan.id.clone(), PlanStatus::Active)
        .unwrap();
    assert_eq!(wait_plan(&engine, &plan)[0].status, JobStatus::Succeeded);
    assert_eq!(engine.history().unwrap().archives.len(), 1);
}

#[test]
fn corrupt_archive_is_rejected_even_if_size_and_mtime_match() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("data"), || {}).unwrap();
    let source = directory.path().join("picture.png");
    fs::write(&source, b"valid").unwrap();
    let asset = engine
        .import_paths(vec![source.to_string_lossy().into()])
        .unwrap()
        .assets
        .remove(0);
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                asset_ids: vec![asset.id],
                ..Draft::default()
            },
            target_ids: vec![],
        })
        .unwrap();
    assert_eq!(wait(&engine, &jobs[0].id).status, JobStatus::Succeeded);
    let record = engine.history().unwrap().archives.remove(0);
    let copy = &record.publication.assets[0].path;
    let modified = fs::metadata(copy).unwrap().modified().unwrap();
    fs::write(copy, b"wrong").unwrap();
    fs::File::options()
        .write(true)
        .open(copy)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let result = engine
        .schedule_replay(request(vec![record.id.clone()], "local-archive"))
        .unwrap();
    let failed = wait_plan(&engine, &result.plan).remove(0);
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.error.unwrap().code, "archive_corrupt");
    assert!(!directory.path().join("data/deliveries").exists());
    fs::remove_file(copy).unwrap();
    let preview = engine
        .preview_replay(request(vec![record.id], "local-archive"))
        .unwrap();
    assert_eq!(preview.skipped_count, 1);
}

#[test]
fn legacy_archive_migrates_using_saved_copy_after_original_is_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path(), || {}).unwrap();
    let source = directory.path().join("legacy.png");
    fs::write(&source, b"old-image").unwrap();
    let asset = engine
        .import_paths(vec![source.to_string_lossy().into()])
        .unwrap()
        .assets
        .remove(0);
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "旧版归档".into(),
                asset_ids: vec![asset.id],
                ..Draft::default()
            },
            target_ids: vec![],
        })
        .unwrap();
    let mut job = wait(&engine, &jobs[0].id);
    assert_eq!(job.status, JobStatus::Succeeded);
    let record = engine.history().unwrap().archives.remove(0);
    job.publication.assets[0].path = source.to_string_lossy().into();
    fs::remove_file(&source).unwrap();
    drop(engine);
    let db = rusqlite::Connection::open(directory.path().join("upload-hub.sqlite3")).unwrap();
    let mut old = serde_json::to_value(job).unwrap();
    for field in [
        "kind",
        "targetId",
        "archiveId",
        "dependsOn",
        "planId",
        "scheduledAt",
    ] {
        old.as_object_mut().unwrap().remove(field);
    }
    db.execute(
        "UPDATE jobs SET data=?1 WHERE id=?2",
        rusqlite::params![old.to_string(), record.id],
    )
    .unwrap();
    db.execute_batch(
        "DROP TABLE archives; DROP TABLE targets; DROP TABLE replay_plans; PRAGMA user_version=1;",
    )
    .unwrap();
    drop(db);
    let engine = Engine::open(directory.path(), || {}).unwrap();
    let history = engine.history().unwrap();
    assert_eq!(history.archives.len(), 1);
    assert_eq!(history.archives[0].id, record.id);
    assert_eq!(
        history.archives[0].receipt.location,
        record.receipt.location
    );
    assert_ne!(
        history.archives[0].publication.assets[0].path,
        source.to_string_lossy()
    );
    assert_eq!(
        fs::read(&history.archives[0].publication.assets[0].path).unwrap(),
        b"old-image"
    );
    let result = engine
        .schedule_replay(request(vec![record.id], "local-archive"))
        .unwrap();
    assert_eq!(
        wait_plan(&engine, &result.plan)[0].status,
        JobStatus::Succeeded
    );
}

#[test]
fn same_workspace_cannot_dispatch_from_two_engines() {
    let directory = tempfile::tempdir().unwrap();
    let first = Engine::open(directory.path(), || {}).unwrap();
    assert_eq!(
        Engine::open(directory.path(), || {}).err().unwrap().code,
        "workspace_busy"
    );
    drop(first);
    assert!(Engine::open(directory.path(), || {}).is_ok());
}
