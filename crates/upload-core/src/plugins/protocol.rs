//! Versioned, language-neutral JSON Lines interface for folder plugins.
use crate::model::*;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
pub const ENTRY_ARGUMENT: &str = "--upload-hub-plugin";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Validate,
    Upload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRequest {
    pub protocol_version: u32,
    pub plugin_id: String,
    pub operation: Operation,
    pub publication: Publication,
    pub target: Option<UploadTarget>,
    pub job_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    pub output_dir: Option<String>,
    #[serde(default)]
    pub workspace: Option<PluginWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResponse {
    pub protocol_version: u32,
    #[serde(flatten)]
    pub message: PluginMessage,
}
impl PluginResponse {
    pub fn new(message: PluginMessage) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PluginMessage {
    Validation { issues: Vec<String> },
    Progress { completed_bytes: u64 },
    Result { receipt: Receipt },
    Error { error: AppError },
}
