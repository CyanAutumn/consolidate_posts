use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

fn lock_account(directory: &Path) -> Result<File, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("browser.lock"))
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if start.elapsed() < Duration::from_secs(600) => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(TryLockError::WouldBlock) => {
                return Err("等待同账号浏览器超过 10 分钟；请先关闭该账号登录窗口或稍后重试".into())
            }
            Err(TryLockError::Error(e)) => return Err(e.to_string()),
        }
    }
}

// The host kills the launcher on cancellation. Keep the whole Node/Chromium tree
// in a Windows job so no child survives to publish after the task was cancelled.
#[cfg(windows)]
fn contain_children() -> Result<(), String> {
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
            return Err("无法创建 X 浏览器进程组".into());
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
            return Err("无法隔离 X 浏览器进程，已停止启动".into());
        }
        // Deliberately retain the non-inheritable handle until OS process teardown.
        // Closing it here would terminate this process as well as its children.
    }
    Ok(())
}
#[cfg(not(windows))]
fn contain_children() -> Result<(), String> {
    Err("当前 X 插件入口需要 Windows，以保证取消时关闭完整浏览器进程树".into())
}

fn run() -> Result<i32, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let protocol = args.first().is_some_and(|v| v == "--upload-hub-plugin");
    let login = args.first().is_some_and(|v| v == "--login");
    if !protocol && !login {
        return Err("这是 Upload Hub 的 X 插件；请从主程序上传，或运行 configure.ps1 登录".into());
    }
    contain_children()?;
    let mut input = Vec::new();
    let directory = if protocol {
        io::stdin()
            .lock()
            .take(2 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 2 * 1024 * 1024 {
            return Err("插件请求超过 2 MiB".into());
        }
        let request: Value = serde_json::from_slice(&input).map_err(|_| "插件请求 JSON 无效")?;
        if request["protocolVersion"] != 1 || request["pluginId"] != "x" {
            return Err("插件协议版本或 ID 无效".into());
        }
        match request["operation"].as_str() {
            Some("validate") => None,
            Some("upload") => {
                let data_dir = request["workspace"]["dataDir"]
                    .as_str()
                    .ok_or("缺少 X 工作区上下文")?;
                let target = request["target"]["id"].as_str().ok_or("缺少 X 上传目标")?;
                if !Path::new(data_dir).is_absolute()
                    || !safe_id(target)
                    || request["target"]["driverId"] != "x"
                {
                    return Err("X 账号配置路径无效".into());
                }
                Some(Path::new(data_dir).join(target))
            }
            _ => return Err("不支持的插件操作".into()),
        }
    } else {
        let option = |name: &str| args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone());
        let data_dir = option("--data-dir").ok_or("缺少 --data-dir")?;
        let target = option("--target-id").unwrap_or_else(|| "x".into());
        if !Path::new(&data_dir).is_absolute() || !safe_id(&target) {
            return Err("X 账号配置路径无效".into());
        }
        Some(Path::new(&data_dir).join("plugin-data/x").join(target))
    };
    let _account_lock = directory.as_deref().map(lock_account).transpose()?;
    let root: PathBuf = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("无法确定插件目录")?
        .into();
    if !root.join("runner.mjs").is_file() {
        return Err("缺少 X 插件 runner.mjs，请重新构建插件".into());
    }
    let mut command = Command::new("node.exe");
    command
        .arg(root.join("runner.mjs"))
        .args(&args)
        .current_dir(&root)
        .stdin(if protocol {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "无法启动 Node.js，请安装 Node.js 22.12+ 或 24 并重新打开主程序")?;
    if protocol {
        let mut stdin = child.stdin.take().ok_or("无法传递插件请求")?;
        stdin.write_all(&input).map_err(|e| e.to_string())?;
        // Close the request pipe before waiting for the worker.
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(1))
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(0) => std::process::ExitCode::SUCCESS,
        Ok(_) => std::process::ExitCode::FAILURE,
        Err(message) => {
            if std::env::args().nth(1).as_deref() == Some("--upload-hub-plugin") {
                println!(
                    "{}",
                    json!({"protocolVersion":1,"type":"error","error":{"code":"x_runtime","message":message}})
                );
                std::process::ExitCode::SUCCESS
            } else {
                eprintln!("{message}");
                std::process::ExitCode::FAILURE
            }
        }
    }
}
