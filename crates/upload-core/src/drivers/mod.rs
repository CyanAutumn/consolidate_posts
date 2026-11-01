mod local_archive;
use crate::model::*;
pub use local_archive::LocalArchive;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct UploadContext<'a> {
    pub job_id: &'a str,
    /// Stable across retries, different for an explicit new upload or replay.
    pub operation_id: &'a str,
    pub archive_root: &'a Path,
    pub cancelled: &'a AtomicBool,
    pub progress: &'a dyn Fn(u64) -> Result<()>,
    pub target: Option<&'a UploadTarget>,
    pub workspace: Option<&'a PluginWorkspace>,
}
impl UploadContext<'_> {
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(AppError::cancelled())
        } else {
            Ok(())
        }
    }
}

/// Drivers run on bounded worker threads. Network drivers must use streaming I/O,
/// bounded timeouts and cancellation checks; secrets never belong in Publication.
pub trait UploadDriver: Send + Sync {
    fn id(&self) -> &str;
    fn validate(&self, publication: &Publication) -> Vec<String>;
    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt>;
}

pub fn catalog() -> Vec<DriverDescriptor> {
    crate::plugins::bundled_catalog()
}
pub fn capability_issues(driver: &DriverDescriptor, publication: &Publication) -> Vec<String> {
    let mut issues = Vec::new();
    if driver.status != "ready" {
        issues.push("驱动尚未接入".into());
    }
    if !driver.accepts_title && !publication.title.is_empty() {
        issues.push("不支持标题".into());
    }
    if !driver.accepts_body && !publication.body.is_empty() {
        issues.push("不支持正文".into());
    }
    if driver
        .max_files
        .is_some_and(|limit| publication.assets.len() > limit)
    {
        issues.push("附件数量超出限制".into());
    }
    for asset in &publication.assets {
        if !driver.kinds.contains(&asset.kind) {
            issues.push(format!("不支持附件类型：{}", asset.name));
        }
        if driver
            .max_file_bytes
            .is_some_and(|limit| asset.size > limit)
        {
            issues.push(format!("文件超出大小限制：{}", asset.name));
        }
    }
    issues
}
