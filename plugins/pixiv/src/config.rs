use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use upload_plugin_sdk::{AppError, Result, UploadContext};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub user_id: String,
    pub tags: Vec<String>,
    pub ai_type: String,
    pub visibility: String,
    #[serde(default)]
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
pub fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
}
pub fn account_dir(context: &UploadContext<'_>) -> Result<PathBuf> {
    let workspace = context
        .workspace
        .ok_or_else(|| AppError::new("pixiv_config", "Pixiv 需要工作区上下文"))?;
    let target = context
        .target
        .ok_or_else(|| AppError::new("pixiv_config", "缺少 Pixiv 上传目标"))?;
    if !safe_id(&target.id) || !Path::new(&workspace.data_dir).is_absolute() {
        return Err(AppError::new("pixiv_config", "账号配置路径无效"));
    }
    Ok(Path::new(&workspace.data_dir).join(&target.id))
}
impl Config {
    // Authentication is independent of per-publication metadata. An empty AI
    // choice stays empty on disk; it must be explicitly set before publishing.
    pub fn validate_for_login(&self) -> Result<()> {
        let mut check = self.clone();
        if check.user_id.is_empty() {
            check.user_id = "0".into();
        }
        if check.tags.is_empty() {
            check.tags = vec!["_".into()];
        }
        if check.ai_type.is_empty() {
            check.ai_type = "notAiGenerated".into();
        }
        check.validate()
    }
    pub fn load_for_login(directory: &Path) -> Result<Self> {
        let text = fs::read_to_string(directory.join("config.json"))?;
        let config: Self = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| AppError::new("pixiv_config", "Pixiv 配置格式错误"))?;
        config.validate_for_login()?;
        Ok(config)
    }
    pub fn load(directory: &Path) -> Result<Self> {
        let path = directory.join("config.json");
        let text = fs::read_to_string(&path).map_err(|_| {
            AppError::new(
                "pixiv_config",
                format!(
                    "请先在平台账号中展开 Pixiv 配置说明，运行浏览器登录向导。配置位置：{}",
                    path.display()
                ),
            )
        })?;
        let value: Self =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| {
                AppError::new("pixiv_config", "Pixiv config.json 格式错误，请检查配置字段")
            })?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.user_id.is_empty() || !self.user_id.bytes().all(|c| c.is_ascii_digit()) {
            return Err(AppError::new(
                "pixiv_config",
                "请填写数字形式的 Pixiv 用户 ID",
            ));
        }
        if self.tags.is_empty()
            || self.tags.len() > 10
            || self.tags.iter().any(|tag| {
                tag.trim().is_empty()
                    || tag.encode_utf16().count() > 32
                    || tag.chars().any(char::is_whitespace)
            })
        {
            return Err(AppError::new(
                "pixiv_config",
                "请配置 1–10 个标签，每个标签最多 32 字且不含空白",
            ));
        }
        if !matches!(self.ai_type.as_str(), "notAiGenerated" | "aiGenerated") {
            return Err(AppError::new(
                "pixiv_config",
                "必须明确设置 aiType 为 notAiGenerated 或 aiGenerated",
            ));
        }
        if !matches!(self.visibility.as_str(), "public" | "private" | "mypixiv") {
            return Err(AppError::new(
                "pixiv_config",
                "visibility 必须为 public、private 或 mypixiv",
            ));
        }
        if self
            .rating
            .as_deref()
            .is_some_and(|x| !matches!(x, "general" | "r18"))
        {
            return Err(AppError::new(
                "pixiv_config",
                "rating 只能为 general、r18 或 null（跟随工作区）",
            ));
        }
        Ok(())
    }
    pub fn rating(&self, context: &UploadContext<'_>) -> Result<&str> {
        let value = context
            .workspace
            .map(|w| serde_json::to_value(&w.content_rating))
            .transpose()?;
        let workspace = value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("unspecified");
        let enforced = match workspace {
            "r18" => Some("r18"),
            "all_ages" => Some("general"),
            _ => None,
        };
        if let Some(enforced) = enforced {
            if self.rating.as_deref().is_some_and(|r| r != enforced) {
                return Err(AppError::new(
                    "pixiv_rating",
                    "Pixiv 年龄限制与工作区分类冲突，请修改账号配置",
                ));
            }
            return Ok(enforced);
        }
        self.rating.as_deref().ok_or_else(|| {
            AppError::new(
                "pixiv_rating",
                "未分类工作区必须在 Pixiv 配置中明确设置 rating（general / r18）",
            )
        })
    }
}

#[cfg(windows)]
pub fn read_session(directory: &Path) -> Result<Zeroizing<String>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let encoded = fs::read_to_string(directory.join("session.dpapi")).map_err(|_| {
        AppError::new(
            "pixiv_login",
            "尚未保存 Pixiv 登录凭据，请运行浏览器登录配置向导",
        )
    })?;
    let encoded = encoded.trim().trim_start_matches('\u{feff}');
    if encoded.len() > 65536 || encoded.len() % 2 != 0 || !encoded.is_ascii() {
        return Err(AppError::new("pixiv_login", "登录凭据格式错误，请重新配置"));
    }
    let mut bytes: Vec<u8> = (0..encoded.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| AppError::new("pixiv_login", "登录凭据格式错误"))?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI uses the current Windows user's key. The decrypted allocation is always wiped and freed.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::new(
            "pixiv_login",
            "无法解密凭据，请使用当前 Windows 用户重新配置登录",
        ));
    }
    let result = unsafe {
        let raw = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let words = Zeroizing::new(
            raw.as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
        let result = String::from_utf16(&words);
        zeroize::Zeroize::zeroize(raw);
        LocalFree(output.pbData.cast());
        result
    }
    .map_err(|_| AppError::new("pixiv_login", "登录凭据编码错误"))?;
    let result = Zeroizing::new(result);
    validate_session(&result)?;
    Ok(result)
}
#[cfg(not(windows))]
pub fn read_session(_: &Path) -> Result<Zeroizing<String>> {
    Err(AppError::new(
        "pixiv_login",
        "此版本的 Pixiv 凭据存储需要 Windows DPAPI",
    ))
}
pub fn validate_session(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 4096
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    {
        return Err(AppError::new(
            "pixiv_login",
            "请填写 PHPSESSID 的值，不要填写整行 Cookie",
        ));
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(windows)]
pub fn save_session(directory: &Path, value: &str) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };
    validate_session(value)?;
    // Match ConvertFrom-SecureString's DPAPI format: UTF-16LE, then hex.
    let mut clear = Zeroizing::new(
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: clear.len() as u32,
        pbData: clear.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::new(
            "pixiv_login",
            "无法加密 Pixiv 登录凭据，未保存新凭据",
        ));
    }
    let encoded = unsafe {
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let result = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        LocalFree(output.pbData.cast());
        result
    };
    atomic_write(&directory.join("session.dpapi"), encoded.as_bytes())
}
#[cfg(not(windows))]
pub fn save_session(_: &Path, _: &str) -> Result<()> {
    Err(AppError::new(
        "pixiv_login",
        "浏览器登录凭据存储需要 Windows DPAPI",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn browser_credentials_use_the_existing_dpapi_format() {
        let directory = tempfile::tempdir().unwrap();
        save_session(directory.path(), "123_browser_session").unwrap();
        assert_eq!(
            &*read_session(directory.path()).unwrap(),
            "123_browser_session"
        );
        let encrypted = fs::read_to_string(directory.path().join("session.dpapi")).unwrap();
        assert!(!encrypted.contains("123_browser_session"));
        assert!(encrypted.bytes().all(|c| c.is_ascii_hexdigit()));
    }
    #[test]
    fn configuration_wizard_encrypts_and_plugin_decrypts_without_plaintext_files() {
        use std::os::windows::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("configure.ps1");
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let command = format!("Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Security'); & {} -DataDir {} -UserId 123 -Tags test -AiType notAiGenerated -Rating general -Session (ConvertTo-SecureString '123_fake_session' -AsPlainText -Force)",quote(&script.to_string_lossy()),quote(&directory.path().to_string_lossy()));
        let result = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &command,
            ])
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let account = directory.path().join("plugin-data/pixiv/pixiv");
        assert_eq!(&*read_session(&account).unwrap(), "123_fake_session");
        assert_eq!(Config::load(&account).unwrap().user_id, "123");
        for name in ["config.json", "session.dpapi"] {
            assert!(!fs::read_to_string(account.join(name))
                .unwrap()
                .contains("123_fake_session"));
        }
        assert!(!String::from_utf8_lossy(&result.stdout).contains("123_fake_session"));
    }
}
