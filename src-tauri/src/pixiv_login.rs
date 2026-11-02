use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use upload_core::{AppError, Result, Snapshot};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub user_id: String,
    pub tags: Vec<String>,
    pub ai_type: String,
    pub visibility: String,
    pub rating: Option<String>,
    #[serde(default)]
    pub original: bool,
    #[serde(default)]
    pub allow_tag_edit: bool,
    #[serde(default = "yes")]
    pub allow_comment: bool,
}
fn yes() -> bool {
    true
}
fn error(message: &str) -> AppError {
    AppError::new("pixiv_login", message)
}
fn directory(snapshot: &Snapshot, target_id: &str) -> Result<PathBuf> {
    if target_id.is_empty()
        || target_id.len() > 128
        || !target_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || !snapshot
            .history
            .targets
            .iter()
            .any(|t| t.id == target_id && t.driver_id == "pixiv")
    {
        return Err(error("当前工作区没有这个 Pixiv 账号"));
    }
    Ok(Path::new(&snapshot.data_dir)
        .join("plugin-data/pixiv")
        .join(target_id))
}
pub fn settings(snapshot: &Snapshot, target_id: &str) -> Result<Settings> {
    let file = directory(snapshot, target_id)?.join("config.json");
    match fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| error("Pixiv 配置损坏，请检查已有配置")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings {
            user_id: String::new(),
            tags: vec![],
            ai_type: String::new(),
            visibility: "private".into(),
            rating: None,
            original: false,
            allow_tag_edit: false,
            allow_comment: true,
        }),
        Err(_) => Err(error("无法读取 Pixiv 账号设置")),
    }
}
pub fn save_settings(
    snapshot: &Snapshot,
    target_id: &str,
    mut value: Settings,
) -> Result<Settings> {
    validate(&value)?;
    if value.tags.is_empty() || value.ai_type.is_empty() {
        return Err(error("请填写标签并明确选择 AI 标记"));
    }
    let folder = directory(snapshot, target_id)?;
    fs::create_dir_all(&folder)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(folder.join("upload.lock"))?;
    lock.try_lock()
        .map_err(|_| error("平台正在登录或上传，请稍后保存"))?;
    value.user_id = settings(snapshot, target_id)?.user_id;
    let file = folder.join("config.json");
    let temporary = folder.join("settings.tmp");
    let mut output = fs::File::create(&temporary)?;
    output.write_all(&serde_json::to_vec_pretty(&value)?)?;
    output.sync_all()?;
    drop(output);
    fs::rename(temporary, file)?;
    Ok(value)
}
fn validate(settings: &Settings) -> Result<()> {
    if settings.tags.len() > 10
        || settings.tags.iter().any(|tag| {
            tag.is_empty()
                || tag.encode_utf16().count() > 32
                || tag.chars().any(char::is_whitespace)
        })
        || !matches!(
            settings.ai_type.as_str(),
            "" | "aiGenerated" | "notAiGenerated"
        )
        || !matches!(
            settings.visibility.as_str(),
            "private" | "public" | "mypixiv"
        )
        || settings
            .rating
            .as_deref()
            .is_some_and(|r| !matches!(r, "general" | "r18"))
        || settings.user_id.len() > 25
        || !settings.user_id.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(error(
            "请填写 1–10 个有效标签，并选择 AI 标记、可见性和年龄限制",
        ));
    }
    Ok(())
}
fn entry_for(snapshot: &Snapshot, driver: &str) -> Result<PathBuf> {
    let report = snapshot
        .plugin_reports
        .iter()
        .find(|p| p.id.as_deref() == Some(driver) && p.state == "ready")
        .ok_or_else(|| error("平台插件不可用，请先启用并构建插件"))?;
    let root = fs::canonicalize(&snapshot.plugin_dir)?;
    let folder = fs::canonicalize(&report.folder)?;
    let entry = fs::canonicalize(folder.join("plugin.exe"))?;
    if !folder.starts_with(&root) || !entry.starts_with(&folder) {
        return Err(error("平台插件入口路径无效"));
    }
    Ok(entry)
}
struct ChildGuard(Child);
#[cfg(windows)]
struct LoginJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl LoginJob {
    fn attach(child: &Child) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(error("无法创建登录进程组"));
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle().cast()) == 0
            {
                CloseHandle(job);
                return Err(error("无法隔离登录进程"));
            }
            Ok(Self(job))
        }
    }
}
#[cfg(windows)]
impl Drop for LoginJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
pub fn launch(entry: &Path, data_dir: &Path, target_id: &str, settings: &Settings) -> Result<()> {
    validate(settings)?;
    let mut command = Command::new(entry);
    command
        .args(["--browser-login", "--data-dir"])
        .arg(data_dir)
        .args(["--target-id", target_id]);
    run_command(&mut command, Some(serde_json::to_vec(settings)?))
}
fn run_command(command: &mut Command, input: Option<Vec<u8>>) -> Result<()> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = ChildGuard(
        command
            .spawn()
            .map_err(|_| error("无法启动平台登录组件，请重新构建插件"))?,
    );
    #[cfg(windows)]
    let _job = LoginJob::attach(&child.0)?;
    let mut stdin = child
        .0
        .stdin
        .take()
        .ok_or_else(|| error("无法传递账号设置"))?;
    if let Some(input) = input {
        stdin.write_all(&input)?;
    }
    drop(stdin);
    let mut stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| error("无法读取登录状态"))?;
    let diagnostics = thread::spawn(move || {
        let mut saved = Vec::new();
        let mut buf = [0; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let remaining = 8192_usize.saturating_sub(saved.len());
            saved.extend_from_slice(&buf[..n.min(remaining)]);
        }
        saved
    });
    let start = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait()? {
            let bytes = diagnostics.join().unwrap_or_default();
            if status.success() {
                return Ok(());
            }
            let text = String::from_utf8_lossy(&bytes);
            return Err(error(
                text.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("登录未完成，请重新打开浏览器登录"),
            ));
        }
        if start.elapsed() > Duration::from_secs(920) {
            return Err(error("登录超时，请重新打开浏览器登录"));
        }
        thread::sleep(Duration::from_millis(100));
    }
}
pub fn login(snapshot: &Snapshot, target_id: &str, settings: &Settings) -> Result<()> {
    directory(snapshot, target_id)?;
    launch(
        &entry_for(snapshot, "pixiv")?,
        Path::new(&snapshot.data_dir),
        target_id,
        settings,
    )
}

pub fn login_x(snapshot: &Snapshot, target_id: &str) -> Result<()> {
    if !snapshot
        .history
        .targets
        .iter()
        .any(|t| t.id == target_id && t.driver_id == "x")
    {
        return Err(error("X 平台不存在"));
    }
    let mut command = Command::new(entry_for(snapshot, "x")?);
    command
        .args(["--login", "--data-dir"])
        .arg(&snapshot.data_dir)
        .args(["--target-id", target_id]);
    run_command(&mut command, None)
}

pub fn login_xiaohongshu(snapshot: &Snapshot, target_id: &str) -> Result<()> {
    if !snapshot
        .history
        .targets
        .iter()
        .any(|t| t.id == target_id && t.driver_id == "xiaohongshu")
    {
        return Err(error("小红书平台不存在"));
    }
    let mut command = Command::new(entry_for(snapshot, "xiaohongshu")?);
    command
        .args(["--login", "--data-dir"])
        .arg(&snapshot.data_dir)
        .args(["--target-id", target_id]);
    run_command(&mut command, None)
}

pub fn login_bilibili(snapshot: &Snapshot, target_id: &str) -> Result<()> {
    if !snapshot
        .history
        .targets
        .iter()
        .any(|t| t.id == target_id && t.driver_id == "bilibili")
    {
        return Err(error("哔哩哔哩平台不存在"));
    }
    let mut command = Command::new(entry_for(snapshot, "bilibili")?);
    command
        .args(["--login", "--data-dir"])
        .arg(&snapshot.data_dir)
        .args(["--target-id", target_id]);
    run_command(&mut command, None)
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BilibiliPosting {
    pub category: String,
    pub tags: Vec<String>,
    pub copyright: u8,
    pub source: String,
}
fn bilibili_directory(snapshot: &Snapshot, target_id: &str) -> Result<PathBuf> {
    if !snapshot
        .history
        .targets
        .iter()
        .any(|t| t.id == target_id && t.driver_id == "bilibili")
        || target_id.is_empty()
        || !target_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(error("哔哩哔哩平台不存在"));
    }
    Ok(Path::new(&snapshot.data_dir)
        .join("plugin-data/bilibili")
        .join(target_id))
}
pub fn bilibili_settings(snapshot: &Snapshot, target_id: &str) -> Result<BilibiliPosting> {
    let file = bilibili_directory(snapshot, target_id)?.join("posting.json");
    match fs::read_to_string(file) {
        Ok(s) => serde_json::from_str(s.trim_start_matches('\u{feff}'))
            .map_err(|_| error("哔哩哔哩投稿设置损坏")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BilibiliPosting::default()),
        Err(e) => Err(e.into()),
    }
}
pub fn save_bilibili_settings(
    snapshot: &Snapshot,
    target_id: &str,
    settings: BilibiliPosting,
) -> Result<()> {
    if settings.category.trim().is_empty()
        || settings.category.len() > 300
        || !matches!(settings.copyright, 1 | 2)
        || settings.tags.is_empty()
        || settings.tags.len() > 10
        || settings
            .tags
            .iter()
            .any(|t| t.trim().is_empty() || t.encode_utf16().count() > 20 || t.contains(','))
        || settings.source.encode_utf16().count() > 200
        || (settings.copyright == 2 && settings.source.trim().is_empty())
    {
        return Err(error(
            "请填写分区、1–10 个标签、自制/转载；转载还需填写来源",
        ));
    }
    let directory = bilibili_directory(snapshot, target_id)?;
    fs::create_dir_all(&directory)?;
    let file = directory.join("posting.json");
    let temp = directory.join("posting-settings.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&settings)?)?;
    fs::rename(temp, file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(root: &Path) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "pluginDir":root,"pluginReports":[],"drivers":[],
            "draft":{"title":"","body":"","assetIds":[]},"assets":[],"jobs":[],
            "dataDir":root,"archiveDir":root,"concurrency":2,
            "history":{"archives":[],"plans":[],"targets":[{"id":"pixiv","driverId":"pixiv","name":"test","createdAt":0}]}
        })).unwrap()
    }
    #[test]
    fn bilibili_posting_settings_validate_target_and_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let mut view = snapshot(root.path());
        view.history.targets.push(serde_json::from_value(serde_json::json!({"id":"bilibili","driverId":"bilibili","name":"test","createdAt":0})).unwrap());
        assert!(bilibili_settings(&view, "../outside").is_err());
        assert!(save_bilibili_settings(&view, "bilibili", BilibiliPosting::default()).is_err());
        let mut value = BilibiliPosting {
            category: "生活/日常".into(),
            tags: vec!["测试".into()],
            copyright: 1,
            source: String::new(),
        };
        save_bilibili_settings(&view, "bilibili", value.clone()).unwrap();
        assert_eq!(bilibili_settings(&view, "bilibili").unwrap().copyright, 1);
        value.copyright = 2;
        assert!(save_bilibili_settings(&view, "bilibili", value.clone()).is_err());
        value.source = "https://example.com/source".into();
        save_bilibili_settings(&view, "bilibili", value).unwrap();
        assert_eq!(bilibili_settings(&view, "bilibili").unwrap().copyright, 2);
    }
    #[test]
    fn settings_are_bound_to_existing_workspace_targets() {
        let root = tempfile::tempdir().unwrap();
        let view = snapshot(root.path());
        assert!(settings(&view, "../other").is_err());
        assert!(settings(&view, "unknown").is_err());
        let mut config = settings(&view, "pixiv").unwrap();
        assert_eq!(config.visibility, "private");
        assert!(validate(&config).is_ok());
        config.tags = vec!["test".into()];
        config.ai_type = "notAiGenerated".into();
        assert!(validate(&config).is_ok());
        let folder = directory(&view, "pixiv").unwrap();
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("config.json"), b"broken").unwrap();
        assert!(settings(&view, "pixiv").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn desktop_login_bridge_uses_native_encryption_and_reports_cancelled_login() {
        let root = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/pixiv/plugin.exe");
        let executable = root.path().join("plugin.exe");
        fs::copy(source, &executable).unwrap();
        let helper = root.path().join("browser-login.mjs");
        fs::write(&helper, "process.stdout.write(JSON.stringify({userId:'123',session:'123_fake_bridge_session'}));").unwrap();
        let view = snapshot(root.path());
        let mut config = settings(&view, "pixiv").unwrap();
        config.tags = vec!["test".into()];
        config.ai_type = "notAiGenerated".into();
        launch(&executable, root.path(), "pixiv", &config).unwrap();
        assert_eq!(settings(&view, "pixiv").unwrap().user_id, "123");
        let encrypted = fs::read(directory(&view, "pixiv").unwrap().join("session.dpapi")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("fake_bridge_session"));
        fs::write(
            helper,
            "console.error('登录窗口已关闭');process.exitCode=1;",
        )
        .unwrap();
        assert!(launch(&executable, root.path(), "pixiv", &config)
            .unwrap_err()
            .message
            .contains("未完成"));
        assert_eq!(
            fs::read(directory(&view, "pixiv").unwrap().join("session.dpapi")).unwrap(),
            encrypted
        );
    }
}
