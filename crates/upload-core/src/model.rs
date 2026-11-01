use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}
impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn cancelled() -> Self {
        Self::new("cancelled", "任务已取消")
    }
}
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for AppError {}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("database", e.to_string())
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("serialization", e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Image,
    Video,
    Text,
    Model,
    File,
}
impl AssetKind {
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tiff" | "avif" | "heic" => {
                Self::Image
            }
            "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => Self::Video,
            "txt" | "md" | "markdown" => Self::Text,
            "safetensors" | "ckpt" | "pt" | "pth" | "onnx" | "gguf" | "bin" | "obj" | "fbx"
            | "glb" | "gltf" | "stl" => Self::Model,
            _ => Self::File,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub path: String,
    pub name: String,
    pub kind: AssetKind,
    pub size: u64,
    pub modified_ms: u64,
    #[serde(default)]
    pub sha256: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub title: String,
    pub body: String,
    pub asset_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Publication {
    pub title: String,
    pub body: String,
    pub assets: Vec<Asset>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverDescriptor {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub status: String,
    pub kinds: Vec<AssetKind>,
    pub accepts_title: bool,
    pub accepts_body: bool,
    pub max_files: Option<usize>,
    pub max_file_bytes: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishRequest {
    pub draft: Draft,
    #[serde(default, alias = "driverIds")]
    pub target_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Validation {
    pub driver_id: String,
    pub eligible: bool,
    pub issues: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportFailure {
    pub path: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub assets: Vec<Asset>,
    pub errors: Vec<ImportFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Scheduled,
    WaitingArchive,
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
impl JobStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Scheduled
                | Self::WaitingArchive
                | Self::Queued
                | Self::Running
                | Self::Cancelling
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedFile {
    pub asset_id: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub location: String,
    pub files: Vec<ArchivedFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub batch_id: String,
    pub driver_id: String,
    pub status: JobStatus,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub created_at: u64,
    pub attempt: u32,
    pub retry_of: Option<String>,
    pub error: Option<AppError>,
    pub receipt: Option<Receipt>,
    pub publication: Publication,
    #[serde(default)]
    pub kind: JobKind,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub archive_id: Option<String>,
    #[serde(default)]
    pub depends_on: Option<String>,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub scheduled_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    #[default]
    Legacy,
    Archive,
    Upload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadTarget {
    pub id: String,
    pub driver_id: String,
    pub name: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginWorkspace {
    pub id: String,
    pub name: String,
    pub content_rating: crate::workspaces::ContentRating,
    /// Per-workspace, per-plugin storage. Account-specific state should use target.id below it.
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRecord {
    pub id: String,
    pub source_job_id: String,
    pub created_at: u64,
    pub publication: Publication,
    pub receipt: Receipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRequest {
    pub archive_ids: Vec<String>,
    pub target_id: String,
    /// UTC Unix milliseconds. The UI converts the user's local calendar time.
    pub start_at: u64,
    pub interval_minutes: u32,
    #[serde(default)]
    pub force: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayItem {
    pub archive_id: String,
    pub title: String,
    pub eligible: bool,
    pub reason: Option<String>,
    pub scheduled_at: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPreview {
    pub items: Vec<ReplayItem>,
    pub eligible_count: usize,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Active,
    Paused,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPlan {
    pub id: String,
    pub target_id: String,
    pub created_at: u64,
    pub start_at: u64,
    pub interval_minutes: u32,
    pub status: PlanStatus,
    /// Actual dispatch time gates the next item after sleep, pause, or downtime.
    pub next_release_at: u64,
    pub archive_ids: Vec<String>,
    pub force: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    pub plan: ReplayPlan,
    pub preview: ReplayPreview,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryState {
    pub archives: Vec<ArchiveRecord>,
    pub targets: Vec<UploadTarget>,
    pub plans: Vec<ReplayPlan>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub platform_accounts: Vec<crate::platform_accounts::PlatformAccount>,
    pub plugin_dir: String,
    pub plugin_reports: Vec<crate::plugins::PluginReport>,
    pub drivers: Vec<DriverDescriptor>,
    pub draft: Draft,
    pub assets: Vec<Asset>,
    pub jobs: Vec<Job>,
    pub data_dir: String,
    pub archive_dir: String,
    pub concurrency: usize,
    pub history: HistoryState,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn modified_ms(meta: &std::fs::Metadata) -> Result<u64> {
    Ok(meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64)
}
pub fn verify_asset(asset: &Asset) -> Result<()> {
    let metadata = std::fs::metadata(&asset.path)?;
    if !metadata.is_file()
        || metadata.len() != asset.size
        || modified_ms(&metadata)? != asset.modified_ms
    {
        return Err(AppError::new(
            "source_changed",
            format!("文件已改变，请重新导入：{}", asset.name),
        ));
    }
    Ok(())
}
