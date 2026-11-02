use tauri::{Emitter, Manager, State};
mod pixiv_login;
use upload_core::workspaces::{WorkspaceConfig, WorkspaceHub, WorkspaceSnapshot};
use upload_core::{
    AppError, Draft, HistoryState, ImportReport, Job, PlanStatus, PublishRequest, ReplayPreview,
    ReplayRequest, ReplayResult, UploadTarget, Validation,
};

// All commands carry an immutable workspace ID, so delayed requests cannot affect a new selection.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> upload_core::Result<T> + Send + 'static,
) -> upload_core::Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::new("runtime", error.to_string()))?
}
#[tauri::command]
async fn bootstrap(
    hub: State<'_, WorkspaceHub>,
    workspace_id: Option<String>,
) -> upload_core::Result<WorkspaceSnapshot> {
    let hub = hub.inner().clone();
    blocking(move || hub.bootstrap(workspace_id.as_deref())).await
}
#[tauri::command]
async fn create_workspace(
    hub: State<'_, WorkspaceHub>,
    config: WorkspaceConfig,
) -> upload_core::Result<WorkspaceSnapshot> {
    let hub = hub.inner().clone();
    blocking(move || hub.create_workspace(config)).await
}
#[tauri::command]
async fn switch_workspace(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
) -> upload_core::Result<WorkspaceSnapshot> {
    let hub = hub.inner().clone();
    blocking(move || hub.switch_workspace(&workspace_id)).await
}
#[tauri::command]
async fn update_workspace(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    config: WorkspaceConfig,
) -> upload_core::Result<WorkspaceSnapshot> {
    let hub = hub.inner().clone();
    blocking(move || hub.update_workspace(&workspace_id, config)).await
}
#[tauri::command]
async fn import_files(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    paths: Vec<String>,
) -> upload_core::Result<ImportReport> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.import_paths(paths)).await
}

#[tauri::command]
async fn save_draft(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    draft: Draft,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.save_draft(draft)).await
}

#[tauri::command]
async fn preflight(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    draft: Draft,
) -> upload_core::Result<Vec<Validation>> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.preflight(draft)).await
}

#[tauri::command]
async fn enqueue_upload(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    request: PublishRequest,
) -> upload_core::Result<Vec<Job>> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.enqueue(request)).await
}

#[tauri::command]
async fn list_jobs(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
) -> upload_core::Result<Vec<Job>> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.jobs()).await
}

#[tauri::command]
async fn cancel_job(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    id: String,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.cancel(id)).await
}

#[tauri::command]
async fn retry_job(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    id: String,
) -> upload_core::Result<Vec<Job>> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.retry(id)).await
}

#[tauri::command]
async fn list_history(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
) -> upload_core::Result<HistoryState> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.history()).await
}

#[tauri::command]
async fn create_target(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    driver_id: String,
    name: String,
) -> upload_core::Result<UploadTarget> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.create_target(driver_id, name)).await
}

#[tauri::command]
async fn preview_replay(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    request: ReplayRequest,
) -> upload_core::Result<ReplayPreview> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.preview_replay(request)).await
}

#[tauri::command]
async fn schedule_replay(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    request: ReplayRequest,
) -> upload_core::Result<ReplayResult> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.schedule_replay(request)).await
}

#[tauri::command]
async fn set_plan_status(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    id: String,
    status: PlanStatus,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.set_plan_status(id, status)).await
}

#[tauri::command]
async fn select_targets(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    ids: Vec<String>,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.select_targets(ids)).await
}

#[tauri::command]
async fn pixiv_settings(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    target_id: String,
) -> upload_core::Result<pixiv_login::Settings> {
    let hub = hub.inner().clone();
    blocking(move || pixiv_login::settings(&hub.engine(&workspace_id)?.snapshot()?, &target_id))
        .await
}
#[tauri::command]
async fn platform_accounts(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
) -> upload_core::Result<Vec<upload_core::platform_accounts::PlatformAccount>> {
    let hub = hub.inner().clone();
    blocking(move || hub.engine(&workspace_id)?.platform_accounts()).await
}
#[tauri::command]
async fn save_pixiv_settings(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    target_id: String,
    settings: pixiv_login::Settings,
) -> upload_core::Result<pixiv_login::Settings> {
    let hub = hub.inner().clone();
    blocking(move || {
        pixiv_login::save_settings(
            &hub.engine(&workspace_id)?.snapshot()?,
            &target_id,
            settings,
        )
    })
    .await
}
#[tauri::command]
async fn login_x(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    target_id: String,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || pixiv_login::login_x(&hub.engine(&workspace_id)?.snapshot()?, &target_id))
        .await
}
#[tauri::command]
async fn login_xiaohongshu(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    target_id: String,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || {
        pixiv_login::login_xiaohongshu(&hub.engine(&workspace_id)?.snapshot()?, &target_id)
    })
    .await
}
#[tauri::command]
async fn login_pixiv(
    hub: State<'_, WorkspaceHub>,
    workspace_id: String,
    target_id: String,
    settings: pixiv_login::Settings,
) -> upload_core::Result<()> {
    let hub = hub.inner().clone();
    blocking(move || {
        pixiv_login::login(
            &hub.engine(&workspace_id)?.snapshot()?,
            &target_id,
            &settings,
        )
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let directory = std::env::var_os("UPLOAD_HUB_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            let handle = app.handle().clone();
            app.manage(WorkspaceHub::open(directory, move || {
                let _ = handle.emit("jobs-changed", ());
            })?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            create_workspace,
            switch_workspace,
            update_workspace,
            import_files,
            save_draft,
            preflight,
            enqueue_upload,
            list_jobs,
            cancel_job,
            retry_job,
            list_history,
            create_target,
            preview_replay,
            schedule_replay,
            set_plan_status,
            select_targets,
            pixiv_settings,
            login_pixiv,
            platform_accounts,
            login_x,
            login_xiaohongshu,
            save_pixiv_settings
        ])
        .run(tauri::generate_context!())
        .expect("Upload Hub failed to start");
}
