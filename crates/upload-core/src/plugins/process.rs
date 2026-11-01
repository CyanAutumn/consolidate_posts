use super::{protocol::*, PluginManifest};
use crate::{
    drivers::{UploadContext, UploadDriver},
    model::*,
};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub(super) struct ProcessPlugin {
    id: String,
    directory: PathBuf,
    entry: PathBuf,
    timeout: Duration,
}
impl ProcessPlugin {
    pub fn new(manifest: &PluginManifest, directory: PathBuf, entry: PathBuf) -> Self {
        Self {
            id: manifest.driver.id.clone(),
            directory,
            entry,
            timeout: Duration::from_secs(manifest.timeout_seconds),
        }
    }
    fn invoke(
        &self,
        request: PluginRequest,
        context: Option<&UploadContext<'_>>,
    ) -> Result<PluginMessage> {
        let mut input = serde_json::to_vec(&request)?;
        if input.len() > MAX_MESSAGE_BYTES {
            return Err(AppError::new("plugin_protocol", "插件请求超过协议大小限制"));
        }
        input.push(b'\n');
        if let Some(context) = context {
            context.check_cancelled()?;
        }
        let entry = fs::canonicalize(&self.entry)?;
        if !entry.starts_with(&self.directory) {
            return Err(AppError::new("plugin_entry", "插件入口路径已移出插件目录"));
        }
        let mut command = Command::new(entry);
        command
            .arg(ENTRY_ARGUMENT)
            .current_dir(&self.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: plugins never steal focus.
        }
        let mut child = ChildGuard(command.spawn().map_err(|error| {
            AppError::new("plugin_start", format!("无法启动插件 {}：{error}", self.id))
        })?);
        let mut stdin = child.0.stdin.take().expect("piped stdin");
        let stdout = child.0.stdout.take().expect("piped stdout");
        let mut stderr = child.0.stderr.take().expect("piped stderr");
        let (sender, receiver) = mpsc::sync_channel(32);
        let writer_sender = sender.clone();
        thread::spawn(move || {
            if let Err(error) = stdin.write_all(&input) {
                let _ = writer_sender.send(Output::Error(format!("无法向插件传入请求：{error}")));
            }
            // Closing stdin tells the plugin there is exactly one request per process.
        });
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match (&mut reader)
                    .take((MAX_MESSAGE_BYTES + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => {
                        let _ = sender.send(Output::End);
                        break;
                    }
                    Ok(_) if line.len() > MAX_MESSAGE_BYTES => {
                        let _ = sender.send(Output::Error("插件响应超过 2 MiB".into()));
                        break;
                    }
                    Ok(_) => match serde_json::from_slice::<PluginResponse>(&line) {
                        Ok(response) => {
                            if sender.send(Output::Message(response)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Output::Error(format!(
                                "插件 stdout 必须仅输出协议 JSON：{error}"
                            )));
                            break;
                        }
                    },
                    Err(error) => {
                        let _ = sender.send(Output::Error(error.to_string()));
                        break;
                    }
                }
            }
        });
        let diagnostic = Arc::new(Mutex::new(Vec::new()));
        let diagnostic_writer = diagnostic.clone();
        thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(count) = stderr.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                if let Ok(mut output) = diagnostic_writer.lock() {
                    let remaining = 8192_usize.saturating_sub(output.len());
                    output.extend_from_slice(&bytes[..count.min(remaining)]);
                }
            }
        });
        let started = Instant::now();
        let timeout = if context.is_some() {
            self.timeout
        } else {
            Duration::from_secs(10)
        };
        let total: u64 = request
            .publication
            .assets
            .iter()
            .map(|asset| asset.size)
            .sum();
        let mut completed = 0;
        let mut last_progress = Instant::now();
        let mut terminal = None;
        let mut ended = false;
        let mut exited = None;
        loop {
            if let Some(context) = context {
                context.check_cancelled()?;
            }
            if started.elapsed() >= timeout {
                return Err(AppError::new(
                    "plugin_timeout",
                    format!("插件 {} 超时，进程已终止", self.id),
                ));
            }
            if ended {
                thread::sleep(Duration::from_millis(25));
            }
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(Output::Message(response)) => {
                    if response.protocol_version != PROTOCOL_VERSION {
                        return Err(AppError::new("plugin_protocol", "插件响应协议版本不兼容"));
                    }
                    if terminal.is_some() {
                        return Err(AppError::new(
                            "plugin_protocol",
                            "插件在最终响应之后继续输出",
                        ));
                    }
                    match response.message {
                        PluginMessage::Progress { completed_bytes } => {
                            let context = context.ok_or_else(|| {
                                AppError::new("plugin_protocol", "校验请求不允许上传进度")
                            })?;
                            if completed_bytes < completed || completed_bytes > total {
                                return Err(AppError::new("plugin_protocol", "插件进度无效"));
                            }
                            if completed_bytes == completed {
                                continue;
                            }
                            completed = completed_bytes;
                            if last_progress.elapsed() >= Duration::from_millis(120)
                                || completed == total
                            {
                                (context.progress)(completed)?;
                                last_progress = Instant::now();
                            }
                        }
                        message => terminal = Some(message),
                    }
                }
                Ok(Output::End) => ended = true,
                Ok(Output::Error(message)) => {
                    return Err(AppError::new("plugin_protocol", message))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => ended = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(status) = child.0.try_wait()? {
                let exit_time = exited.get_or_insert_with(Instant::now);
                if ended {
                    if !status.success() {
                        let detail = diagnostic
                            .lock()
                            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
                            .unwrap_or_default();
                        return Err(AppError::new(
                            "plugin_exit",
                            format!("插件 {} 异常退出（{status}）{detail}", self.id),
                        ));
                    }
                    break;
                }
                if exit_time.elapsed() > Duration::from_secs(1) {
                    return Err(AppError::new(
                        "plugin_protocol",
                        "插件退出后未关闭标准输出；插件不得遗留继承输出管道的子进程",
                    ));
                }
            }
        }
        match terminal {
            Some(PluginMessage::Error { error }) => Err(error),
            Some(message) => Ok(message),
            None => Err(AppError::new("plugin_protocol", "插件退出时未返回结果")),
        }
    }
}
impl UploadDriver for ProcessPlugin {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, publication: &Publication) -> Vec<String> {
        match self.invoke(
            PluginRequest {
                protocol_version: PROTOCOL_VERSION,
                plugin_id: self.id.clone(),
                operation: Operation::Validate,
                publication: publication.clone(),
                target: None,
                job_id: None,
                operation_id: None,
                output_dir: None,
                workspace: None,
            },
            None,
        ) {
            Ok(PluginMessage::Validation { issues }) => issues,
            Ok(_) => vec!["插件未返回 validation 响应".into()],
            Err(error) => vec![error.message],
        }
    }
    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        let message = self.invoke(
            PluginRequest {
                protocol_version: PROTOCOL_VERSION,
                plugin_id: self.id.clone(),
                operation: Operation::Upload,
                publication: publication.clone(),
                target: context.target.cloned(),
                job_id: Some(context.job_id.into()),
                operation_id: Some(context.operation_id.into()),
                output_dir: Some(context.archive_root.to_string_lossy().into()),
                workspace: context.workspace.cloned(),
            },
            Some(context),
        )?;
        match message {
            PluginMessage::Result { receipt } if !receipt.location.trim().is_empty() => Ok(receipt),
            _ => Err(AppError::new("plugin_protocol", "插件未返回有效的上传结果")),
        }
    }
}
enum Output {
    Message(PluginResponse),
    Error(String),
    End,
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
