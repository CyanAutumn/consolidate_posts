use crate::config::{self, Config};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use upload_plugin_sdk::{AppError, Result};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserLogin {
    user_id: String,
    session: String,
}

#[cfg(windows)]
fn contain_children() -> Result<()> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::GetCurrentProcess,
        },
    };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(error("无法创建浏览器进程组"));
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&info) as u32,
        ) == 0
            || AssignProcessToJobObject(job, GetCurrentProcess()) == 0
        {
            CloseHandle(job);
            return Err(error("无法隔离浏览器进程，已停止启动"));
        }
        // Keep this non-inheritable handle until process teardown. Closing it
        // terminates this launcher and all of its Node/Chromium descendants.
    }
    Ok(())
}
#[cfg(not(windows))]
fn contain_children() -> Result<()> {
    Err(error("浏览器登录凭据存储需要 Windows"))
}
fn error(message: &str) -> AppError {
    AppError::new("pixiv_browser_login", message)
}

fn account_dir(args: &[String]) -> Result<PathBuf> {
    let mut root = None;
    let mut target = "pixiv".to_owned();
    let (options, remainder) = args.as_chunks::<2>();
    for pair in options {
        match pair[0].as_str() {
            "--data-dir" if root.is_none() => root = Some(PathBuf::from(&pair[1])),
            "--target-id" => target = pair[1].clone(),
            _ => return Err(error("浏览器登录参数无效")),
        }
    }
    let root = root
        .filter(|root| root.is_absolute() && root.is_dir())
        .ok_or_else(|| error("工作区数据目录无效，请从平台账号复制配置命令"))?;
    if !remainder.is_empty() || !config::safe_id(&target) {
        return Err(error("上传目标 ID 或参数无效"));
    }
    Ok(root.join("plugin-data/pixiv").join(target))
}
fn lock_account(directory: &Path) -> Result<File> {
    fs::create_dir_all(directory)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("upload.lock"))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(error(
            "此 Pixiv 账号正在上传或登录，请等待任务结束后重新配置",
        )),
        Err(TryLockError::Error(e)) => Err(e.into()),
    }
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

fn run() -> Result<String> {
    let args = std::env::args().skip(2).collect::<Vec<_>>();
    let directory = account_dir(&args)?;
    let input = Zeroizing::new({
        let mut bytes = Vec::new();
        std::io::stdin().take(65537).read_to_end(&mut bytes)?;
        bytes
    });
    if input.len() > 65536 {
        return Err(error("投稿设置超过大小限制"));
    }
    let mut settings: Config =
        serde_json::from_slice(&input).map_err(|_| error("投稿设置格式错误"))?;
    let mut expected_user = settings.user_id.clone();
    if expected_user.is_empty() {
        settings.user_id = "0".into();
    }
    settings.validate_for_login()?;
    let _lock = lock_account(&directory)?;
    let existing_path = directory.join("config.json");
    if existing_path.exists() {
        let existing = Config::load_for_login(&directory)?;
        if !expected_user.is_empty() && existing.user_id != expected_user {
            return Err(error("此目标已绑定其他 Pixiv 账号，请新建上传目标后配置"));
        }
        expected_user = existing.user_id;
    }
    contain_children()?;
    let root = std::env::current_exe()?
        .parent()
        .ok_or_else(|| error("无法确定插件目录"))?
        .to_path_buf();
    let script = root.join("browser-login.mjs");
    if !script.is_file() {
        return Err(error(
            "缺少浏览器登录组件，请运行 npm run plugin:build:pixiv",
        ));
    }
    let mut command = Command::new("node.exe");
    command
        .arg(script)
        .arg("--native-pipe")
        .arg("--expected-user-id")
        .arg(&expected_user)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    eprintln!(
        "请在打开的浏览器中手动登录 Pixiv 并完成验证。首次运行可能下载浏览器组件；本向导不会投稿。"
    );
    let mut child = ChildGuard(
        command
            .spawn()
            .map_err(|_| error("无法启动 Node.js，请安装 Node.js 22.12+ / 24 后重试"))?,
    );
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| error("无法读取浏览器登录结果"))?;
    let reader = thread::spawn(move || {
        let mut output = Zeroizing::new(Vec::new());
        stdout.take(16385).read_to_end(&mut output).map(|_| output)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(900) {
            return Err(error("浏览器登录启动或等待超时，请重新配置"));
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        return Err(error(
            "浏览器登录未完成，原配置和凭据保持不变；请查看上方提示后重试",
        ));
    }
    let output = reader
        .join()
        .map_err(|_| error("无法读取浏览器登录结果"))??;
    if output.len() > 16384 {
        return Err(error("浏览器登录结果格式无效"));
    }
    let result: BrowserLogin =
        serde_json::from_slice(&output).map_err(|_| error("浏览器未返回有效登录结果"))?;
    let session = Zeroizing::new(result.session);
    config::validate_session(&session)?;
    if result.user_id.is_empty()
        || !result.user_id.bytes().all(|c| c.is_ascii_digit())
        || session.split_once('_').map(|(id, _)| id) != Some(result.user_id.as_str())
        || (!expected_user.is_empty() && expected_user != result.user_id)
    {
        return Err(error("浏览器登录账号与目标不一致，未保存凭据"));
    }
    settings.user_id = result.user_id;
    settings.validate_for_login()?;
    config::save_session(&directory, &session)?;
    config::atomic_write(
        &directory.join("config.json"),
        &serde_json::to_vec_pretty(&settings)?,
    )?;
    Ok(settings.user_id)
}

pub fn main() -> std::process::ExitCode {
    match run() {
        Ok(user) => {
            println!("Pixiv 用户 {user} 登录成功，投稿设置和加密凭据已保存。可以回到主程序上传。");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.message);
            std::process::ExitCode::FAILURE
        }
    }
}
pub fn diagnose() -> std::process::ExitCode {
    let result = (|| -> Result<serde_json::Value> {
        let directory = account_dir(&std::env::args().skip(2).collect::<Vec<_>>())?;
        let settings = Config::load(&directory)?;
        let session = config::read_session(&directory)?;
        let client = crate::client::PixivClient::connect(&session, &settings.user_id)?;
        let init = client.init()?;
        let keys = init
            .as_object()
            .map(|v| v.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        Ok(
            serde_json::json!({"loginValid":true,"initKeys":keys,"postingScripts":client.posting_scripts()?}),
        )
    })();
    match result {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.message);
            std::process::ExitCode::FAILURE
        }
    }
}
