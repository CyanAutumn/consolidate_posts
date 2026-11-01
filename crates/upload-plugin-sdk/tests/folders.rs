use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use upload_core::*;

fn install(root: &Path, folder: &str, id: &str) -> PathBuf {
    let directory = root.join(folder);
    fs::create_dir_all(&directory).unwrap();
    let executable = if cfg!(windows) {
        "plugin.exe"
    } else {
        "plugin"
    };
    fs::copy(
        env!("CARGO_BIN_EXE_plugin-contract-fixture"),
        directory.join(executable),
    )
    .unwrap();
    let manifest = json!({ "schemaVersion":1, "version":"0.1.0", "enabled":true, "id":id, "name":"测试插件", "category":"测试", "description":"实际子进程协议测试", "status":"ready", "kinds":["image","text","file"], "acceptsTitle":true,"acceptsBody":true,"maxFiles":100,"maxFileBytes":null,"entry":{"type":"process","path":executable},"timeoutSeconds":1 });
    fs::write(
        directory.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    directory
}
fn edit(folder: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let path = folder.join("plugin.json");
    let mut manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut manifest);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}
fn wait(engine: &Engine, id: &str) -> Job {
    let started = Instant::now();
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
            started.elapsed() < Duration::from_secs(8),
            "timed out: {job:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn submit(engine: &Engine) -> Vec<Job> {
    engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "测试发布".into(),
                body: "完整正文".into(),
                asset_ids: vec![],
            },
            target_ids: vec!["fixture".into()],
        })
        .unwrap()
}

#[test]
fn real_process_upload_and_replay_use_same_entry_and_archived_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    install(&root, "fixture", "fixture");
    let engine = Engine::with_plugin_dir(directory.path().join("data"), &root, || {}).unwrap();
    assert_eq!(engine.snapshot().unwrap().drivers[0].id, "fixture");
    let source = directory.path().join("中文 原图.png");
    fs::write(&source, b"image-bytes").unwrap();
    let asset = engine
        .import_paths(vec![source.to_string_lossy().into()])
        .unwrap()
        .assets
        .remove(0);
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "插件标题".into(),
                body: "插件正文".into(),
                asset_ids: vec![asset.id],
            },
            target_ids: vec!["fixture".into()],
        })
        .unwrap();
    let output = wait(&engine, &jobs[1].id);
    assert_eq!(output.status, JobStatus::Succeeded, "{:?}", output.error);
    let archive = engine.history().unwrap().archives.remove(0);
    fs::remove_file(source).unwrap();
    let target = engine
        .create_target("fixture".into(), "插件新账号".into())
        .unwrap();
    let plan = engine
        .schedule_replay(ReplayRequest {
            archive_ids: vec![archive.id],
            target_id: target.id.clone(),
            start_at: now_ms(),
            interval_minutes: 0,
            force: false,
        })
        .unwrap()
        .plan;
    let job = engine
        .jobs()
        .unwrap()
        .into_iter()
        .find(|job| job.plan_id.as_deref() == Some(&plan.id))
        .unwrap();
    let done = wait(&engine, &job.id);
    assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
    let receipt = done.receipt.unwrap();
    assert!(receipt.location.contains(&target.id));
    assert_eq!(
        fs::read(Path::new(&receipt.location).join(&receipt.files[0].file_name)).unwrap(),
        b"image-bytes"
    );
    assert_eq!(engine.history().unwrap().archives.len(), 1);
}

#[test]
fn process_plugins_receive_separate_workspace_identity_and_storage() {
    use upload_core::workspaces::{ContentRating, WorkspaceConfig, WorkspaceHub};
    let directory = tempfile::tempdir().unwrap();
    let plugins = directory.path().join("plugins");
    install(&plugins, "fixture", "fixture");
    let hub =
        WorkspaceHub::with_plugin_dir(directory.path().join("data"), &plugins, || {}).unwrap();
    let adult = hub
        .create_workspace(WorkspaceConfig {
            name: "R18 区".into(),
            content_rating: ContentRating::R18,
            enabled_driver_ids: vec!["fixture".into()],
        })
        .unwrap();
    for id in ["default", adult.workspace.id.as_str()] {
        let engine = hub.engine(id).unwrap();
        let jobs = submit(&engine);
        let done = wait(&engine, &jobs[1].id);
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
        let data = PathBuf::from(engine.snapshot().unwrap().data_dir).join("plugin-data/fixture");
        let received: PluginWorkspace =
            serde_json::from_slice(&fs::read(data.join("workspace.json")).unwrap()).unwrap();
        assert_eq!(received.id, id);
        assert_eq!(PathBuf::from(received.data_dir), data);
        if id != "default" {
            assert_eq!(received.content_rating, ContentRating::R18);
            assert_eq!(received.name, "R18 区");
        }
    }
    assert_eq!(
        hub.engine("default")
            .unwrap()
            .history()
            .unwrap()
            .archives
            .len(),
        1
    );
    assert_eq!(
        hub.engine(&adult.workspace.id)
            .unwrap()
            .history()
            .unwrap()
            .archives
            .len(),
        1
    );
}

#[test]
fn malformed_disabled_duplicate_and_missing_plugins_have_isolated_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    install(&root, "first", "duplicate");
    install(&root, "second", "duplicate");
    let disabled = install(&root, "disabled", "disabled");
    edit(&disabled, |manifest| manifest["enabled"] = json!(false));
    let missing = install(&root, "missing", "missing");
    edit(&missing, |manifest| {
        manifest["entry"]["path"] = json!("missing.exe")
    });
    let escaping = install(&root, "escaping", "escaping");
    edit(&escaping, |manifest| {
        manifest["entry"]["path"] = json!("../plugin.exe")
    });
    let bad_version = install(&root, "version", "version");
    edit(&bad_version, |manifest| {
        manifest["schemaVersion"] = json!(99)
    });
    fs::create_dir(root.join("no-manifest")).unwrap();
    let engine = Engine::with_plugin_dir(directory.path().join("data"), root, || {}).unwrap();
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.plugin_reports.len(), 7);
    assert_eq!(
        snapshot
            .plugin_reports
            .iter()
            .filter(|report| report.state == "error")
            .count(),
        6
    );
    assert_eq!(
        snapshot
            .drivers
            .iter()
            .filter(|driver| driver.id == "duplicate")
            .count(),
        1
    );
    assert!(snapshot
        .drivers
        .iter()
        .all(|driver| driver.status == "unavailable"));
    assert!(!snapshot
        .history
        .targets
        .iter()
        .any(|target| target.driver_id == "disabled"));
    // Even with no executable plugin, mandatory local archiving still works.
    let job = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "仍可归档".into(),
                ..Draft::default()
            },
            target_ids: vec![],
        })
        .unwrap()
        .remove(0);
    assert_eq!(wait(&engine, &job.id).status, JobStatus::Succeeded);
}

#[test]
fn process_failure_timeout_and_invalid_progress_do_not_report_success() {
    for (marker, expected) in [
        ("hang", "plugin_timeout"),
        ("crash", "plugin_exit"),
        ("malformed", "plugin_protocol"),
        ("bad-progress", "plugin_protocol"),
        ("large-error", "plugin_exit"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("plugins");
        let folder = install(&root, "fixture", "fixture");
        fs::write(folder.join(marker), b"").unwrap();
        let engine = Engine::with_plugin_dir(directory.path().join("data"), root, || {}).unwrap();
        let jobs = submit(&engine);
        let done = wait(&engine, &jobs[1].id);
        assert_eq!(done.status, JobStatus::Failed, "{marker}: {:?}", done.error);
        assert_eq!(done.error.unwrap().code, expected, "{marker}");
        assert_eq!(engine.history().unwrap().archives.len(), 1);
    }
}

#[test]
fn cancelling_running_process_terminates_it_promptly() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let folder = install(&root, "fixture", "fixture");
    fs::write(folder.join("hang"), b"").unwrap();
    edit(&folder, |manifest| manifest["timeoutSeconds"] = json!(30));
    let engine = Engine::with_plugin_dir(directory.path().join("data"), root, || {}).unwrap();
    let jobs = submit(&engine);
    let started = Instant::now();
    while !folder.join("started").exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    engine.cancel(jobs[1].id.clone()).unwrap();
    assert_eq!(wait(&engine, &jobs[1].id).status, JobStatus::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn validation_rejection_and_missing_folder_are_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let folder = install(&root, "fixture", "fixture");
    fs::write(folder.join("reject-validation"), b"").unwrap();
    let engine = Engine::with_plugin_dir(directory.path().join("data"), &root, || {}).unwrap();
    let checks = engine
        .preflight(Draft {
            title: "测试".into(),
            ..Draft::default()
        })
        .unwrap();
    assert!(!checks[0].eligible);
    assert!(checks[0].issues[0].contains("validation rejected"));
    drop(engine);
    let engine = Engine::with_plugin_dir(
        directory.path().join("data"),
        directory.path().join("not-installed"),
        || {},
    )
    .unwrap();
    assert!(engine.snapshot().unwrap().drivers.is_empty());
    assert_eq!(engine.snapshot().unwrap().plugin_reports[0].state, "error");
}

#[test]
fn newly_installed_plugin_can_replay_history_from_before_installation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    fs::create_dir(&root).unwrap();
    let engine = Engine::with_plugin_dir(directory.path().join("data"), &root, || {}).unwrap();
    let jobs = engine
        .enqueue(PublishRequest {
            draft: Draft {
                title: "插件安装前的历史".into(),
                ..Draft::default()
            },
            target_ids: vec![],
        })
        .unwrap();
    assert_eq!(wait(&engine, &jobs[0].id).status, JobStatus::Succeeded);
    let record = engine.history().unwrap().archives.remove(0);
    drop(engine);
    install(&root, "fixture", "fixture");
    let engine = Engine::with_plugin_dir(directory.path().join("data"), &root, || {}).unwrap();
    assert!(engine
        .history()
        .unwrap()
        .targets
        .iter()
        .any(|target| target.id == "fixture"));
    let result = engine
        .schedule_replay(ReplayRequest {
            archive_ids: vec![record.id],
            target_id: "fixture".into(),
            start_at: now_ms(),
            interval_minutes: 0,
            force: false,
        })
        .unwrap();
    let job = engine
        .jobs()
        .unwrap()
        .into_iter()
        .find(|job| job.plan_id.as_deref() == Some(&result.plan.id))
        .unwrap();
    assert_eq!(wait(&engine, &job.id).status, JobStatus::Succeeded);
}

#[test]
fn retries_share_operation_id_but_explicit_replays_have_new_identity() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let folder = install(&root, "fixture", "fixture");
    fs::write(folder.join("crash"), b"").unwrap();
    let engine = Engine::with_plugin_dir(directory.path().join("data"), root, || {}).unwrap();
    let first = submit(&engine)[1].clone();
    let first = wait(&engine, &first.id);
    assert_eq!(first.status, JobStatus::Failed);
    assert_eq!(
        fs::read_to_string(folder.join("operation-id")).unwrap(),
        first.id
    );
    let second = engine.retry(first.id.clone()).unwrap().remove(0);
    assert_eq!(wait(&engine, &second.id).status, JobStatus::Failed);
    fs::remove_file(folder.join("crash")).unwrap();
    let third = engine.retry(second.id).unwrap().remove(0);
    assert_eq!(wait(&engine, &third.id).status, JobStatus::Succeeded);
    assert_eq!(
        fs::read_to_string(folder.join("operation-id")).unwrap(),
        first.id
    );
    let replay = engine
        .schedule_replay(ReplayRequest {
            archive_ids: vec![first.archive_id.unwrap()],
            target_id: "fixture".into(),
            start_at: now_ms(),
            interval_minutes: 0,
            force: true,
        })
        .unwrap();
    let job = engine
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.plan_id.as_deref() == Some(&replay.plan.id))
        .unwrap();
    assert_eq!(wait(&engine, &job.id).status, JobStatus::Succeeded);
    assert_eq!(
        fs::read_to_string(folder.join("operation-id")).unwrap(),
        job.id
    );
    assert_ne!(job.id, first.id);
}
