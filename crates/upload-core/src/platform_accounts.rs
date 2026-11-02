use crate::{model::*, Engine};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAccount {
    pub driver_id: String,
    pub target_id: String,
    pub connected: bool,
    pub name: String,
}
pub fn configured(root: &Path, target: &UploadTarget) -> bool {
    let folder = root
        .join("plugin-data")
        .join(&target.driver_id)
        .join(&target.id);
    match target.driver_id.as_str() {
        "local-archive" => true,
        "pixiv" => folder.join("config.json").is_file() && folder.join("session.dpapi").is_file(),
        "xiaohongshu" => {
            folder.join("config.json").is_file() && folder.join("session.dpapi.json").is_file()
        }
        "x" => folder.join("config.json").is_file() && folder.join("profile").is_dir(),
        _ => folder.join("config.json").is_file(),
    }
}
pub fn accounts(root: &Path, targets: &[UploadTarget]) -> Vec<PlatformAccount> {
    let mut drivers = std::collections::BTreeSet::new();
    for target in targets {
        drivers.insert(target.driver_id.clone());
    }
    drivers
        .into_iter()
        .filter_map(|driver_id| {
            // Preserve the authenticated legacy account and its submission journal.
            // No credential copying, target deletion, or historical rewrite is needed.
            let target = targets
                .iter()
                .filter(|t| t.driver_id == driver_id)
                .max_by_key(|t| {
                    (
                        configured(root, t),
                        t.id == driver_id,
                        std::cmp::Reverse(t.created_at),
                    )
                })?;
            Some(PlatformAccount {
                driver_id,
                target_id: target.id.clone(),
                connected: configured(root, target),
                name: target.name.clone(),
            })
        })
        .collect()
}
impl Engine {
    pub fn platform_accounts(&self) -> Result<Vec<PlatformAccount>> {
        Ok(accounts(
            &self.inner.data_dir,
            &self.inner.store.history()?.targets,
        ))
    }
    pub(crate) fn platform_target(&self, id: &str) -> Result<UploadTarget> {
        let old = self.inner.store.target(id)?;
        if !matches!(old.driver_id.as_str(), "pixiv" | "x" | "xiaohongshu") {
            return Ok(old);
        }
        let primary = self
            .platform_accounts()?
            .into_iter()
            .find(|a| a.driver_id == old.driver_id)
            .ok_or_else(|| AppError::new("unknown_driver", "平台不存在"))?;
        self.inner.store.target(&primary.target_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::{UploadContext, UploadDriver};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    struct Pixiv;
    impl UploadDriver for Pixiv {
        fn id(&self) -> &str {
            "pixiv"
        }
        fn validate(&self, _: &Publication) -> Vec<String> {
            vec![]
        }
        fn upload(&self, _: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
            if context.target.unwrap().id == "pixiv" {
                return Err(AppError::new("pixiv_config", "missing config"));
            }
            Ok(Receipt {
                location: "https://www.pixiv.net/artworks/123".into(),
                files: vec![],
            })
        }
    }
    fn wait(engine: &Engine) {
        let end = Instant::now() + Duration::from_secs(5);
        while engine.jobs().unwrap().iter().any(|j| j.status.is_active()) {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn existing_login_is_used_by_new_posts_and_configuration_failure_retries() {
        let root = tempfile::tempdir().unwrap();
        let engine = Engine::with_drivers(root.path(), vec![Arc::new(Pixiv)], || {}).unwrap();
        let draft = Draft {
            title: "test".into(),
            body: "body".into(),
            asset_ids: vec![],
        };
        let original = engine
            .enqueue(PublishRequest {
                draft: draft.clone(),
                target_ids: vec!["pixiv".into()],
            })
            .unwrap();
        wait(&engine);
        let failed = engine
            .jobs()
            .unwrap()
            .into_iter()
            .find(|j| j.kind == JobKind::Upload)
            .unwrap();
        assert_eq!(failed.error.as_ref().unwrap().code, "pixiv_config");
        let logged_in = engine
            .create_target("pixiv".into(), "CyanAutumn".into())
            .unwrap();
        let folder = root.path().join("plugin-data/pixiv").join(&logged_in.id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("config.json"), "{}").unwrap();
        std::fs::write(folder.join("session.dpapi"), "fake encrypted credential").unwrap();
        let primary = engine
            .platform_accounts()
            .unwrap()
            .into_iter()
            .find(|a| a.driver_id == "pixiv")
            .unwrap();
        assert_eq!(primary.target_id, logged_in.id);
        assert!(primary.connected);
        let uncertain_folder = root.path().join("plugin-data/pixiv/pixiv/submissions");
        std::fs::create_dir_all(&uncertain_folder).unwrap();
        let uncertain = uncertain_folder.join(format!("{}.json", failed.id));
        std::fs::write(&uncertain, "{}").unwrap();
        assert_eq!(
            engine.retry(failed.id.clone()).unwrap_err().code,
            "submission_uncertain"
        );
        std::fs::remove_file(uncertain).unwrap();
        let retried = engine.retry(failed.id.clone()).unwrap();
        assert_eq!(retried[0].target_id.as_deref(), Some(logged_in.id.as_str()));
        wait(&engine);
        assert_eq!(
            engine.inner.store.job(&retried[0].id).unwrap().status,
            JobStatus::Succeeded
        );
        assert!(engine.retry(failed.id).is_err());
        let archive = engine
            .inner
            .store
            .job(&original[0].id)
            .unwrap()
            .archive_id
            .unwrap();
        let preview = engine
            .preview_replay(ReplayRequest {
                archive_ids: vec![archive],
                target_id: "pixiv".into(),
                start_at: now_ms(),
                interval_minutes: 0,
                force: false,
            })
            .unwrap();
        assert_eq!(preview.skipped_count, 1);
        let jobs = engine
            .enqueue(PublishRequest {
                draft,
                target_ids: vec!["pixiv".into(), logged_in.id],
            })
            .unwrap();
        assert_eq!(
            jobs.len(),
            2,
            "one archive plus one platform upload, even for duplicate legacy aliases"
        );
        wait(&engine);
    }
}
