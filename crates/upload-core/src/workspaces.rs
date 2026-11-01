//! Workspace identity is explicit at every API boundary. Existing data stays in the default root.
use crate::{model::*, plugins, Engine, Notifier};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentRating {
    Unspecified,
    AllAges,
    R18,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    pub name: String,
    pub content_rating: ContentRating,
    pub enabled_driver_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProfile {
    pub id: String,
    pub created_at: u64,
    #[serde(flatten)]
    pub config: WorkspaceConfig,
    pub selected_target_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    #[serde(flatten)]
    pub snapshot: Snapshot,
    pub workspace: WorkspaceProfile,
    pub workspaces: Vec<WorkspaceProfile>,
}

impl Engine {
    pub fn workspace_profile(&self) -> Result<Option<WorkspaceProfile>> {
        self.inner.store.setting("workspace")
    }
    pub(crate) fn require_enabled(&self, driver_id: &str) -> Result<()> {
        if self.workspace_profile()?.is_some_and(|profile| {
            !profile
                .config
                .enabled_driver_ids
                .iter()
                .any(|id| id == driver_id)
        }) {
            return Err(AppError::new(
                "workspace_plugin_disabled",
                "此插件未在当前工作区启用",
            ));
        }
        Ok(())
    }
    fn initialize_workspace(&self, id: &str, config: WorkspaceConfig) -> Result<()> {
        if self.workspace_profile()?.is_none() {
            let selected_target_ids = if id == "default"
                && config
                    .enabled_driver_ids
                    .iter()
                    .any(|id| id == "local-archive")
            {
                vec!["local-archive".into()]
            } else {
                vec![]
            };
            self.inner.store.save_setting(
                "workspace",
                &WorkspaceProfile {
                    id: id.into(),
                    created_at: now_ms(),
                    config,
                    selected_target_ids,
                },
            )?;
        }
        if self
            .workspace_profile()?
            .is_none_or(|profile| profile.id != id)
        {
            return Err(AppError::new(
                "workspace_identity",
                "工作区身份与索引不匹配",
            ));
        }
        Ok(())
    }
    fn update_workspace(&self, config: WorkspaceConfig) -> Result<()> {
        let _submission = self
            .inner
            .submission
            .lock()
            .map_err(|_| AppError::new("queue", "提交锁异常"))?;
        let mut profile = self
            .workspace_profile()?
            .ok_or_else(|| AppError::new("workspace_missing", "工作区不存在"))?;
        if config.content_rating != profile.config.content_rating
            && self
                .jobs()?
                .iter()
                .any(|job| job.kind != JobKind::Archive && job.status.is_active())
        {
            return Err(AppError::new(
                "workspace_rating_busy",
                "工作区仍有上传或补传任务，请先完成或取消，再修改年龄分类",
            ));
        }
        // New submissions use this same lock. Existing scheduled and paused jobs count as active.
        if self.jobs()?.iter().any(|job| {
            job.kind != JobKind::Archive
                && job.status.is_active()
                && !config.enabled_driver_ids.contains(&job.driver_id)
        }) {
            return Err(AppError::new(
                "workspace_plugin_busy",
                "待停用的插件仍有上传或补传任务，请先完成或取消这些任务",
            ));
        }
        let targets = self.history()?.targets;
        profile.selected_target_ids.retain(|id| {
            targets.iter().any(|target| {
                &target.id == id && config.enabled_driver_ids.contains(&target.driver_id)
            })
        });
        profile.config = config;
        self.inner.store.save_setting("workspace", &profile)?;
        self.inner.changed();
        Ok(())
    }
    pub fn select_targets(&self, ids: Vec<String>) -> Result<()> {
        let _submission = self
            .inner
            .submission
            .lock()
            .map_err(|_| AppError::new("queue", "提交锁异常"))?;
        let mut profile = self
            .workspace_profile()?
            .ok_or_else(|| AppError::new("workspace_missing", "工作区不存在"))?;
        let mut selected = Vec::new();
        for id in ids {
            let target = self.inner.store.target(&id)?;
            self.require_enabled(&target.driver_id)?;
            if !selected.contains(&id) {
                selected.push(id);
            }
        }
        profile.selected_target_ids = selected;
        self.inner.store.save_setting("workspace", &profile)
    }
}

struct HubState {
    engines: HashMap<String, Engine>,
    order: Vec<String>,
    index: Connection,
    root: PathBuf,
    registry: plugins::Registry,
    notify: Notifier,
}
#[derive(Clone)]
pub struct WorkspaceHub {
    state: Arc<Mutex<HubState>>,
}

impl WorkspaceHub {
    pub fn open(root: impl AsRef<Path>, notify: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        Self::with_plugin_dir(root, plugins::default_directory(), notify)
    }
    pub fn with_plugin_dir(
        root: impl AsRef<Path>,
        plugin_dir: impl AsRef<Path>,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let registry = plugins::Registry::load(plugin_dir.as_ref());
        let notify: Notifier = Arc::new(notify);
        // Acquire the existing root's process lease before touching the shared workspace index.
        let callback = notify.clone();
        let default = Engine::with_registry(&root, registry.clone(), move || callback())?;
        default.initialize_workspace(
            "default",
            WorkspaceConfig {
                name: "默认工作区".into(),
                content_rating: ContentRating::Unspecified,
                enabled_driver_ids: registry
                    .catalog
                    .iter()
                    .map(|driver| driver.id.clone())
                    .collect(),
            },
        )?;
        let root = root.as_ref().canonicalize()?;
        let index = Connection::open(root.join("workspaces.sqlite3"))?;
        index.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = index.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 1 {
            return Err(AppError::new("database_version", "工作区索引来自更新版本"));
        }
        index.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS workspaces(id TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS preferences(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT OR IGNORE INTO workspaces(id) VALUES('default');
            INSERT OR IGNORE INTO preferences(key,value) VALUES('active','default');
            PRAGMA user_version=1;",
        )?;
        let order: Vec<String> = index
            .prepare("SELECT id FROM workspaces ORDER BY rowid")?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut engines = HashMap::from([("default".into(), default)]);
        for id in order.iter().filter(|id| *id != "default") {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(AppError::new("workspace_identity", "无效的工作区索引"));
            }
            let path = root.join("workspaces").join(id);
            if !path.join("upload-hub.sqlite3").is_file() {
                return Err(AppError::new(
                    "workspace_missing",
                    format!("工作区数据库丢失：{id}"),
                ));
            }
            // Reject junctions/symlinks escaping the data root.
            if !path.canonicalize()?.starts_with(&root) {
                return Err(AppError::new("workspace_path", "工作区路径超出数据目录"));
            }
            let callback = notify.clone();
            let engine = Engine::with_registry(path, registry.clone(), move || callback())?;
            if engine
                .workspace_profile()?
                .is_none_or(|profile| profile.id != *id)
            {
                return Err(AppError::new(
                    "workspace_identity",
                    "工作区身份与索引不匹配",
                ));
            }
            engines.insert(id.clone(), engine);
        }
        Ok(Self {
            state: Arc::new(Mutex::new(HubState {
                engines,
                order,
                index,
                root,
                registry,
                notify,
            })),
        })
    }
    fn state(&self) -> Result<MutexGuard<'_, HubState>> {
        self.state
            .lock()
            .map_err(|_| AppError::new("workspace_lock", "工作区管理锁异常"))
    }
    pub fn engine(&self, id: &str) -> Result<Engine> {
        self.state()?
            .engines
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new("workspace_missing", "工作区不存在"))
    }
    pub fn bootstrap(&self, id: Option<&str>) -> Result<WorkspaceSnapshot> {
        let state = self.state()?;
        let active: String = match id {
            Some(id) => id.into(),
            None => state.index.query_row(
                "SELECT value FROM preferences WHERE key='active'",
                [],
                |row| row.get(0),
            )?,
        };
        state.snapshot(&active)
    }
    pub fn switch_workspace(&self, id: &str) -> Result<WorkspaceSnapshot> {
        let state = self.state()?;
        let snapshot = state.snapshot(id)?;
        state
            .index
            .execute("UPDATE preferences SET value=?1 WHERE key='active'", [id])?;
        Ok(snapshot)
    }
    pub fn create_workspace(&self, mut config: WorkspaceConfig) -> Result<WorkspaceSnapshot> {
        let mut state = self.state()?;
        state.validate_config(&mut config, None)?;
        if state.order.len() >= 32 {
            return Err(AppError::new("workspace_limit", "目前最多支持 32 个工作区"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let callback = state.notify.clone();
        let engine = Engine::with_registry(
            state.root.join("workspaces").join(&id),
            state.registry.clone(),
            move || callback(),
        )?;
        engine.initialize_workspace(&id, config)?;
        let tx = state.index.transaction()?;
        tx.execute("INSERT INTO workspaces(id) VALUES(?1)", [&id])?;
        tx.execute("UPDATE preferences SET value=?1 WHERE key='active'", [&id])?;
        tx.commit()?;
        state.engines.insert(id.clone(), engine);
        state.order.push(id.clone());
        state.snapshot(&id)
    }
    pub fn update_workspace(
        &self,
        id: &str,
        mut config: WorkspaceConfig,
    ) -> Result<WorkspaceSnapshot> {
        let state = self.state()?;
        state.validate_config(&mut config, Some(id))?;
        state
            .engines
            .get(id)
            .ok_or_else(|| AppError::new("workspace_missing", "工作区不存在"))?
            .update_workspace(config)?;
        state.snapshot(id)
    }
}
impl HubState {
    fn profiles(&self) -> Result<Vec<WorkspaceProfile>> {
        self.order
            .iter()
            .map(|id| {
                self.engines[id]
                    .workspace_profile()?
                    .ok_or_else(|| AppError::new("workspace_missing", "工作区信息缺失"))
            })
            .collect()
    }
    fn snapshot(&self, id: &str) -> Result<WorkspaceSnapshot> {
        let engine = self
            .engines
            .get(id)
            .ok_or_else(|| AppError::new("workspace_missing", "工作区不存在"))?;
        Ok(WorkspaceSnapshot {
            snapshot: engine.snapshot()?,
            workspace: engine
                .workspace_profile()?
                .ok_or_else(|| AppError::new("workspace_missing", "工作区信息缺失"))?,
            workspaces: self.profiles()?,
        })
    }
    fn validate_config(&self, config: &mut WorkspaceConfig, current: Option<&str>) -> Result<()> {
        config.name = config.name.trim().into();
        if config.name.is_empty() || config.name.chars().count() > 80 {
            return Err(AppError::new(
                "invalid_name",
                "工作区名称请填写 1–80 个字符",
            ));
        }
        let profiles = self.profiles()?;
        if profiles.iter().any(|profile| {
            Some(profile.id.as_str()) != current
                && profile.config.name.to_lowercase() == config.name.to_lowercase()
        }) {
            return Err(AppError::new("workspace_duplicate", "工作区名称已存在"));
        }
        let retained = profiles
            .iter()
            .find(|profile| Some(profile.id.as_str()) == current);
        let mut seen = HashSet::new();
        config
            .enabled_driver_ids
            .retain(|id| seen.insert(id.clone()));
        if config.enabled_driver_ids.iter().any(|id| {
            !self.registry.catalog.iter().any(|driver| &driver.id == id)
                && !retained.is_some_and(|profile| profile.config.enabled_driver_ids.contains(id))
        }) {
            return Err(AppError::new("unknown_driver", "选择的插件未安装"));
        }
        Ok(())
    }
}
