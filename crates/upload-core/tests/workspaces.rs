use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use upload_core::{
    workspaces::{ContentRating, WorkspaceConfig, WorkspaceHub},
    *,
};

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}
fn config(name: &str, enabled: bool) -> WorkspaceConfig {
    WorkspaceConfig {
        name: name.into(),
        content_rating: ContentRating::R18,
        enabled_driver_ids: if enabled {
            vec!["local-archive".into()]
        } else {
            vec![]
        },
    }
}
fn draft(title: &str) -> Draft {
    Draft {
        title: title.into(),
        body: "独立正文".into(),
        asset_ids: vec![],
    }
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
            "job never finished: {job:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn archive(engine: &Engine, title: &str) -> ArchiveRecord {
    let job = engine
        .enqueue(PublishRequest {
            draft: draft(title),
            target_ids: vec![],
        })
        .unwrap()
        .remove(0);
    assert_eq!(wait(engine, &job.id).status, JobStatus::Succeeded);
    engine
        .history()
        .unwrap()
        .archives
        .into_iter()
        .find(|item| item.id == job.id)
        .unwrap()
}
fn replay(id: &str, delay: u64) -> ReplayRequest {
    ReplayRequest {
        archive_ids: vec![id.into()],
        target_id: "local-archive".into(),
        start_at: now_ms() + delay,
        interval_minutes: 0,
        force: false,
    }
}

#[test]
fn legacy_root_is_preserved_and_all_workspace_resources_are_isolated() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    let source = temp.path().join("shared.png");
    fs::write(&source, b"same explicit source").unwrap();
    let old = Engine::with_plugin_dir(&root, plugins(), || {}).unwrap();
    let asset = old
        .import_paths(vec![source.to_string_lossy().into()])
        .unwrap()
        .assets
        .remove(0);
    let old_draft = Draft {
        asset_ids: vec![asset.id.clone()],
        ..draft("升级前作品")
    };
    old.save_draft(old_draft.clone()).unwrap();
    let old_archive = archive(&old, "升级前归档");
    drop(old);

    let hub = WorkspaceHub::with_plugin_dir(&root, plugins(), || {}).unwrap();
    let original = hub.bootstrap(None).unwrap();
    assert_eq!(original.workspace.id, "default");
    assert_eq!(original.snapshot.draft.title, "升级前作品");
    assert_eq!(original.snapshot.assets[0].id, asset.id);
    assert_eq!(original.snapshot.history.archives[0].id, old_archive.id);
    assert_eq!(
        PathBuf::from(&original.snapshot.data_dir),
        root.canonicalize().unwrap()
    );
    assert!(std::path::Path::new(&old_archive.receipt.location).exists());

    let adult = hub.create_workspace(config("R18 创作", true)).unwrap();
    let adult_id = adult.workspace.id;
    let r18 = hub.engine(&adult_id).unwrap();
    assert!(adult.snapshot.history.archives.is_empty());
    assert!(adult.snapshot.jobs.is_empty());
    assert!(adult.workspace.selected_target_ids.is_empty());
    assert!(r18.save_draft(old_draft).is_err());
    assert!(r18.cancel(old_archive.source_job_id.clone()).is_err());
    assert!(r18.retry(old_archive.source_job_id.clone()).is_err());
    assert!(r18.preview_replay(replay(&old_archive.id, 0)).is_err());
    assert!(r18.schedule_replay(replay(&old_archive.id, 0)).is_err());
    let own_asset = r18
        .import_paths(vec![source.to_string_lossy().into()])
        .unwrap()
        .assets
        .remove(0);
    assert_ne!(asset.id, own_asset.id);
    r18.save_draft(Draft {
        asset_ids: vec![own_asset.id],
        ..draft("成人草稿")
    })
    .unwrap();
    let account = r18
        .create_target("local-archive".into(), "R18 账号".into())
        .unwrap();
    r18.select_targets(vec![account.id.clone()]).unwrap();
    let own_archive = archive(&r18, "成人历史");
    assert!(own_archive.receipt.location.contains(&adult_id));

    let general = hub
        .create_workspace(WorkspaceConfig {
            content_rating: ContentRating::AllAges,
            ..config("全年龄", false)
        })
        .unwrap();
    let all = hub.engine(&general.workspace.id).unwrap();
    assert!(general.snapshot.history.archives.is_empty());
    assert!(all.select_targets(vec![account.id]).is_err());
    assert!(all.preview_replay(replay(&own_archive.id, 0)).is_err());
    assert_eq!(
        all.enqueue(PublishRequest {
            draft: draft("错误目标"),
            target_ids: vec!["local-archive".into()]
        })
        .unwrap_err()
        .code,
        "workspace_plugin_disabled"
    );
    assert_eq!(
        all.create_target("local-archive".into(), "不应创建".into())
            .unwrap_err()
            .code,
        "workspace_plugin_disabled"
    );
    assert!(all.jobs().unwrap().is_empty());
    archive(&all, "全年龄自己的历史"); // mandatory archive is independent of plugin selection
    assert_eq!(all.history().unwrap().archives.len(), 1);
    assert_eq!(r18.history().unwrap().archives.len(), 1);
    let restored = hub.switch_workspace(&adult_id).unwrap();
    assert_eq!(restored.snapshot.draft.title, "成人草稿");
    assert_eq!(restored.workspace.selected_target_ids.len(), 1);
    assert_eq!(
        hub.switch_workspace("default")
            .unwrap()
            .snapshot
            .draft
            .title,
        "升级前作品"
    );
}

#[test]
fn switching_keeps_background_plans_running_and_restart_retains_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let hub = WorkspaceHub::with_plugin_dir(temp.path(), plugins(), || {}).unwrap();
    let workspace = hub
        .create_workspace(config("定时 R18", true))
        .unwrap()
        .workspace;
    let engine = hub.engine(&workspace.id).unwrap();
    let record = archive(&engine, "后台补传");
    let plan = engine
        .schedule_replay(replay(&record.id, 300))
        .unwrap()
        .plan;
    assert_eq!(
        hub.update_workspace(&workspace.id, config("定时 R18", false))
            .unwrap_err()
            .code,
        "workspace_plugin_busy"
    );
    hub.switch_workspace("default").unwrap();
    let job = engine
        .jobs()
        .unwrap()
        .into_iter()
        .find(|job| job.plan_id.as_ref() == Some(&plan.id))
        .unwrap();
    assert_eq!(wait(&engine, &job.id).status, JobStatus::Succeeded);
    let record = archive(&engine, "重启后补传");
    let future = engine
        .schedule_replay(replay(&record.id, 3_600_000))
        .unwrap()
        .plan;
    engine
        .set_plan_status(future.id.clone(), PlanStatus::Paused)
        .unwrap();
    engine.select_targets(vec!["local-archive".into()]).unwrap();
    engine.save_draft(draft("需要保存的草稿")).unwrap();
    drop(engine);
    drop(hub);

    let hub = WorkspaceHub::with_plugin_dir(temp.path(), plugins(), || {}).unwrap();
    assert_eq!(hub.bootstrap(None).unwrap().workspace.id, "default");
    let state = hub.switch_workspace(&workspace.id).unwrap();
    assert_eq!(state.snapshot.draft.title, "需要保存的草稿");
    assert_eq!(state.workspace.config.content_rating, ContentRating::R18);
    assert_eq!(state.workspace.selected_target_ids, vec!["local-archive"]);
    assert_eq!(
        state
            .snapshot
            .history
            .plans
            .iter()
            .find(|plan| plan.id == future.id)
            .unwrap()
            .status,
        PlanStatus::Paused
    );
    assert_eq!(
        hub.update_workspace(&workspace.id, config("改名", false))
            .unwrap_err()
            .code,
        "workspace_plugin_busy"
    );
    assert_eq!(
        hub.update_workspace(
            &workspace.id,
            WorkspaceConfig {
                content_rating: ContentRating::AllAges,
                ..config("改名", true)
            }
        )
        .unwrap_err()
        .code,
        "workspace_rating_busy"
    );
    let engine = hub.engine(&workspace.id).unwrap();
    engine
        .set_plan_status(future.id.clone(), PlanStatus::Cancelled)
        .unwrap();
    let updated = hub
        .update_workspace(&workspace.id, config("改名", false))
        .unwrap();
    assert!(updated.workspace.selected_target_ids.is_empty());
    assert_eq!(updated.snapshot.history.archives.len(), 2);
    let cancelled = engine
        .jobs()
        .unwrap()
        .into_iter()
        .find(|job| job.plan_id.as_ref() == Some(&future.id))
        .unwrap();
    assert_eq!(
        engine.retry(cancelled.id).unwrap_err().code,
        "workspace_plugin_disabled"
    );
    assert_eq!(
        engine
            .preview_replay(replay(&record.id, 0))
            .unwrap()
            .eligible_count,
        0
    );
    assert!(hub
        .bootstrap(Some("default"))
        .unwrap()
        .snapshot
        .jobs
        .is_empty());
}

#[test]
fn workspace_names_ids_and_root_leases_are_validated() {
    let temp = tempfile::tempdir().unwrap();
    let hub = WorkspaceHub::with_plugin_dir(temp.path(), plugins(), || {}).unwrap();
    assert!(WorkspaceHub::with_plugin_dir(temp.path(), plugins(), || {}).is_err());
    assert!(hub.create_workspace(config(" ", true)).is_err());
    assert!(hub.create_workspace(config("默认工作区", true)).is_err());
    assert!(hub
        .create_workspace(WorkspaceConfig {
            enabled_driver_ids: vec!["uninstalled".into()],
            ..config("bad", true)
        })
        .is_err());
    assert!(hub.switch_workspace("../outside").is_err());
    assert!(hub.engine("../outside").is_err());
    let state = hub
        .create_workspace(WorkspaceConfig {
            enabled_driver_ids: vec!["local-archive".into(), "local-archive".into()],
            ..config("  独立空间  ", true)
        })
        .unwrap();
    assert_eq!(state.workspace.config.name, "独立空间");
    assert_eq!(state.workspace.config.enabled_driver_ids.len(), 1);
    assert_eq!(
        hub.bootstrap(None).unwrap().workspace.id,
        state.workspace.id
    );
    assert!(hub
        .update_workspace(&state.workspace.id, config("默认工作区", true))
        .is_err());
}

#[test]
fn newly_installed_plugins_are_not_automatically_enabled_in_existing_workspaces() {
    let temp = tempfile::tempdir().unwrap();
    let plugin_dir = temp.path().join("plugins");
    fs::create_dir_all(plugin_dir.join("local-archive")).unwrap();
    fs::copy(
        plugins().join("local-archive/plugin.json"),
        plugin_dir.join("local-archive/plugin.json"),
    )
    .unwrap();
    let root = temp.path().join("data");
    let hub = WorkspaceHub::with_plugin_dir(&root, &plugin_dir, || {}).unwrap();
    assert_eq!(
        hub.bootstrap(None)
            .unwrap()
            .workspace
            .config
            .enabled_driver_ids,
        vec!["local-archive"]
    );
    drop(hub);
    fs::create_dir_all(plugin_dir.join("pixiv")).unwrap();
    fs::copy(
        plugins().join("pixiv/plugin.json"),
        plugin_dir.join("pixiv/plugin.json"),
    )
    .unwrap();
    let hub = WorkspaceHub::with_plugin_dir(&root, &plugin_dir, || {}).unwrap();
    let state = hub.bootstrap(None).unwrap();
    assert_eq!(state.snapshot.drivers.len(), 2);
    assert_eq!(
        state.workspace.config.enabled_driver_ids,
        vec!["local-archive"]
    );
    let engine = hub.engine("default").unwrap();
    assert_eq!(
        engine
            .create_target("pixiv".into(), "新账号".into())
            .unwrap_err()
            .code,
        "workspace_plugin_disabled"
    );
    hub.update_workspace(
        "default",
        WorkspaceConfig {
            enabled_driver_ids: vec!["pixiv".into()],
            ..config("默认工作区", false)
        },
    )
    .unwrap();
    assert!(engine
        .create_target("pixiv".into(), "新账号".into())
        .is_ok());
}
