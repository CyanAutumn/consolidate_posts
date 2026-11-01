use crate::config::Config;
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use upload_plugin_sdk::{AppError, Publication, Result, UploadContext};
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub fn submit(
    session: &str,
    publication: &Publication,
    context: &UploadContext<'_>,
    config: &Config,
    rating: &str,
) -> Result<String> {
    crate::browser_login::contain_children()?;
    let root = std::env::current_exe()?
        .parent()
        .ok_or_else(|| AppError::new("pixiv_not_submitted", "无法确定插件目录"))?
        .to_owned();
    let mut command = Command::new("node.exe");
    command
        .arg(root.join("browser-submit.mjs"))
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = Guard(
        command
            .spawn()
            .map_err(|_| AppError::new("pixiv_not_submitted", "无法启动 Node.js，尚未提交"))?,
    );
    let mut input = child.0.stdin.take().unwrap();
    let payload = zeroize::Zeroizing::new(serde_json::to_vec(
        &serde_json::json!({"session":session,"publication":publication,"config":config,"rating":rating}),
    )?);
    input.write_all(&payload)?;
    drop(input);
    let output = child.0.stdout.take().unwrap();
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        output.take(65537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    loop {
        context.check_cancelled()?;
        if child.0.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(600) {
            return Err(AppError::new(
                "pixiv_uncertain",
                "浏览器投稿超时，请先核对作品状态",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    let bytes = reader
        .join()
        .map_err(|_| AppError::new("pixiv_uncertain", "无法读取浏览器回执"))??;
    let result: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::new("pixiv_uncertain", "浏览器未返回有效回执，请先核对作品状态"))?;
    if let Some(error) = result.get("error") {
        let code = match error["code"].as_str() {
            Some("pixiv_not_submitted") => "pixiv_not_submitted",
            Some("pixiv_rejected") => "pixiv_rejected",
            _ => "pixiv_uncertain",
        };
        return Err(AppError::new(
            code,
            error["message"].as_str().unwrap_or("浏览器投稿失败"),
        ));
    }
    result["convertKey"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() < 512)
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::new(
                "pixiv_uncertain",
                "浏览器缺少投稿处理编号，请先核对作品状态",
            )
        })
}
