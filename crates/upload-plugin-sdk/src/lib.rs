//! Implement `UploadDriver`, then call `run_plugin` once from main.
use std::{
    io::{BufRead, Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
};
pub use upload_core::{
    drivers::{LocalArchive, UploadContext, UploadDriver},
    model::*,
    plugins::protocol::*,
};

/// A process handles exactly one request. Logs belong on stderr, never stdout.
pub fn run_plugin(plugin: impl UploadDriver) -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() != Some(ENTRY_ARGUMENT) {
        eprintln!("这是 Upload Hub 插件，请由主程序通过 plugins 文件夹加载。");
        return std::process::ExitCode::FAILURE;
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(&plugin)))
        .unwrap_or_else(|_| Err(AppError::new("plugin_panic", "插件执行发生异常")));
    let message = outcome.unwrap_or_else(|error| PluginMessage::Error { error });
    match emit(message) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn emit(message: PluginMessage) -> Result<()> {
    let bytes = serde_json::to_vec(&PluginResponse::new(message))?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(AppError::new("plugin_protocol", "插件响应超过 2 MiB"));
    }
    let mut output = std::io::stdout().lock();
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn handle(plugin: &dyn UploadDriver) -> Result<PluginMessage> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(AppError::new("plugin_protocol", "插件请求超过 2 MiB"));
    }
    let request: PluginRequest = serde_json::from_slice(&bytes)?;
    if request.protocol_version != PROTOCOL_VERSION || request.plugin_id != plugin.id() {
        return Err(AppError::new(
            "plugin_protocol",
            "协议版本或插件 ID 与入口实现不匹配",
        ));
    }
    match request.operation {
        Operation::Validate => Ok(PluginMessage::Validation {
            issues: plugin.validate(&request.publication),
        }),
        Operation::Upload => {
            let issues = plugin.validate(&request.publication);
            if !issues.is_empty() {
                return Err(AppError::new("incompatible", issues.join("；")));
            }
            let job_id = request
                .job_id
                .as_deref()
                .ok_or_else(|| AppError::new("plugin_protocol", "缺少 jobId"))?;
            let output_dir = request
                .output_dir
                .as_deref()
                .ok_or_else(|| AppError::new("plugin_protocol", "缺少 outputDir"))?;
            if !Path::new(output_dir).is_absolute()
                || request
                    .target
                    .as_ref()
                    .is_none_or(|target| target.driver_id != request.plugin_id)
            {
                return Err(AppError::new("plugin_protocol", "上传目标或输出目录无效"));
            }
            let cancelled = AtomicBool::new(false);
            let progress = |completed_bytes| emit(PluginMessage::Progress { completed_bytes });
            let context = UploadContext {
                job_id,
                operation_id: request.operation_id.as_deref().unwrap_or(job_id),
                archive_root: Path::new(output_dir),
                cancelled: &cancelled,
                progress: &progress,
                target: request.target.as_ref(),
                workspace: request.workspace.as_ref(),
            };
            let receipt = plugin.upload(&request.publication, &context)?;
            Ok(PluginMessage::Result { receipt })
        }
    }
}
