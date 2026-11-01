mod process;
pub mod protocol;

use crate::{
    drivers::{LocalArchive, UploadDriver},
    model::*,
};
use process::ProcessPlugin;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub schema_version: u32,
    pub version: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub driver: DriverDescriptor,
    pub entry: PluginEntry,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    7200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginEntry {
    Builtin { name: String },
    Process { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReport {
    pub folder: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub state: String,
    pub message: String,
}

#[derive(Clone)]
pub(crate) struct Registry {
    pub directory: PathBuf,
    pub catalog: Vec<DriverDescriptor>,
    pub drivers: HashMap<String, Arc<dyn UploadDriver>>,
    pub reports: Vec<PluginReport>,
}

pub fn default_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("UPLOAD_HUB_PLUGIN_DIR") {
        return path.into();
    }
    // Development uses the source tree; distributed builds use the executable's sibling folder.
    if cfg!(debug_assertions) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        if source.is_dir() {
            return source;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("plugins")))
        .unwrap_or_else(|| PathBuf::from("plugins"))
}

/// Used only by injected-driver tests; the desktop discovers manifests on disk.
pub fn bundled_catalog() -> Vec<DriverDescriptor> {
    [
        include_str!("../../../../plugins/local-archive/plugin.json"),
        include_str!("../../../../plugins/bilibili/plugin.json"),
        include_str!("../../../../plugins/xiaohongshu/plugin.json"),
        include_str!("../../../../plugins/pixiv/plugin.json"),
        include_str!("../../../../plugins/huggingface/plugin.json"),
        include_str!("../../../../plugins/civitai/plugin.json"),
    ]
    .into_iter()
    .map(|text| {
        serde_json::from_str::<PluginManifest>(text)
            .expect("bundled manifest")
            .driver
    })
    .collect()
}

impl Registry {
    pub fn load(directory: &Path) -> Self {
        let directory = directory
            .canonicalize()
            .unwrap_or_else(|_| directory.to_path_buf());
        let mut result = Self {
            directory: directory.clone(),
            catalog: vec![],
            drivers: HashMap::new(),
            reports: vec![],
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                result.reports.push(report(
                    &directory,
                    None,
                    "error",
                    format!("无法读取插件目录：{error}"),
                ));
                return result;
            }
        };
        let mut folders = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => match entry.file_type() {
                    Ok(kind) if kind.is_dir() => folders.push(entry.path()),
                    Ok(kind) if kind.is_symlink() => result.reports.push(report(
                        &entry.path(),
                        None,
                        "error",
                        "插件目录不支持符号链接".into(),
                    )),
                    Ok(_) => {}
                    Err(error) => {
                        result
                            .reports
                            .push(report(&entry.path(), None, "error", error.to_string()))
                    }
                },
                Err(error) => {
                    result
                        .reports
                        .push(report(&directory, None, "error", error.to_string()))
                }
            }
        }
        folders.sort();
        let mut manifests = Vec::new();
        for folder in folders {
            match read_manifest(&folder) {
                Ok(manifest) => manifests.push((folder, manifest)),
                Err(error) => result
                    .reports
                    .push(report(&folder, None, "error", error.message)),
            }
        }
        let mut ids = HashSet::new();
        let mut duplicates = HashSet::new();
        for (_, manifest) in &manifests {
            if manifest.enabled && !ids.insert(manifest.driver.id.clone()) {
                duplicates.insert(manifest.driver.id.clone());
            }
        }
        for (folder, manifest) in manifests {
            if !manifest.enabled {
                result.reports.push(report(
                    &folder,
                    Some(&manifest),
                    "disabled",
                    "插件已禁用".into(),
                ));
                continue;
            }
            let loaded = if duplicates.contains(&manifest.driver.id) {
                Err(AppError::new(
                    "plugin_duplicate",
                    "插件 ID 重复，所有同 ID 插件均未加载",
                ))
            } else {
                load_driver(&folder, &manifest)
            };
            match loaded {
                Ok(driver) => {
                    if let Some(driver) = driver {
                        result.drivers.insert(manifest.driver.id.clone(), driver);
                    }
                    result.catalog.push(manifest.driver.clone());
                    result.reports.push(report(
                        &folder,
                        Some(&manifest),
                        &manifest.driver.status,
                        if manifest.driver.status == "ready" {
                            "已加载"
                        } else {
                            "接口实现待接入"
                        }
                        .into(),
                    ));
                }
                Err(error) => {
                    let mut descriptor = manifest.driver.clone();
                    descriptor.status = "unavailable".into();
                    // Repeated IDs must never create duplicate UI keys or seed ambiguous accounts.
                    if !result
                        .catalog
                        .iter()
                        .any(|driver| driver.id == descriptor.id)
                    {
                        result.catalog.push(descriptor);
                    }
                    result
                        .reports
                        .push(report(&folder, Some(&manifest), "error", error.message));
                }
            }
        }
        result
            .catalog
            .sort_by_key(|driver| (driver.id != "local-archive", driver.id.clone()));
        result
    }
}

fn report(
    folder: &Path,
    manifest: Option<&PluginManifest>,
    state: &str,
    message: String,
) -> PluginReport {
    PluginReport {
        folder: folder.to_string_lossy().into(),
        id: manifest.map(|m| m.driver.id.clone()),
        name: manifest.map(|m| m.driver.name.clone()),
        version: manifest.map(|m| m.version.clone()),
        state: state.into(),
        message,
    }
}
fn read_manifest(folder: &Path) -> Result<PluginManifest> {
    let path = folder.join("plugin.json");
    if !path.is_file() {
        return Err(AppError::new("plugin_manifest", "缺少统一清单 plugin.json"));
    }
    if path.metadata()?.len() > 64 * 1024 {
        return Err(AppError::new("plugin_manifest", "plugin.json 超过 64 KiB"));
    }
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(&path)?)?;
    let id = &manifest.driver.id;
    if manifest.schema_version != 1 {
        return Err(AppError::new(
            "plugin_version",
            "不支持的插件协议版本，请使用 schemaVersion: 1",
        ));
    }
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(AppError::new(
            "plugin_manifest",
            "插件 ID 必须为 1–64 位小写字母、数字或短横线",
        ));
    }
    if manifest.version.trim().is_empty()
        || manifest.driver.name.trim().is_empty()
        || !["ready", "planned"].contains(&manifest.driver.status.as_str())
        || !(1..=86_400).contains(&manifest.timeout_seconds)
    {
        return Err(AppError::new(
            "plugin_manifest",
            "插件名称、版本、状态或超时时间无效",
        ));
    }
    Ok(manifest)
}
fn load_driver(folder: &Path, manifest: &PluginManifest) -> Result<Option<Arc<dyn UploadDriver>>> {
    match &manifest.entry {
        PluginEntry::Builtin { name } => {
            if name != "local-archive" || manifest.driver.id != "local-archive" {
                return Err(AppError::new("plugin_entry", "未知的内置插件入口"));
            }
            Ok((manifest.driver.status == "ready")
                .then(|| Arc::new(LocalArchive) as Arc<dyn UploadDriver>))
        }
        PluginEntry::Process { path } => {
            if manifest.driver.id == "local-archive" {
                return Err(AppError::new(
                    "plugin_entry",
                    "local-archive 为内置插件保留 ID",
                ));
            }
            let relative = Path::new(path);
            if path.is_empty()
                || !relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
            {
                return Err(AppError::new(
                    "plugin_entry",
                    "入口必须是插件文件夹内的相对文件路径",
                ));
            }
            #[cfg(windows)]
            if !relative
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            {
                return Err(AppError::new(
                    "plugin_entry",
                    "Windows 插件入口必须是 .exe 文件",
                ));
            }
            if manifest.driver.status == "planned" {
                return Ok(None);
            }
            let folder = folder.canonicalize()?;
            let entry = folder.join(relative).canonicalize().map_err(|_| {
                AppError::new(
                    "plugin_entry",
                    format!("入口文件不存在：{path}；请先编译插件"),
                )
            })?;
            if !entry.starts_with(&folder) || !entry.is_file() {
                return Err(AppError::new("plugin_entry", "插件入口不在当前插件目录内"));
            }
            Ok(Some(Arc::new(ProcessPlugin::new(manifest, folder, entry))))
        }
    }
}
