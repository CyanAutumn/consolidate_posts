use crate::{model::*, new_job, Engine};
use std::{collections::HashSet, sync::atomic::Ordering};

impl Engine {
    pub fn preview_replay(&self, request: ReplayRequest) -> Result<ReplayPreview> {
        self.replay_preview(&request)
    }
    fn replay_preview(&self, request: &ReplayRequest) -> Result<ReplayPreview> {
        if request.archive_ids.is_empty() || request.archive_ids.len() > 10_000 {
            return Err(AppError::new("limit", "请选择 1–10000 条历史归档"));
        }
        if request.interval_minutes > 525_600
            || request.start_at > now_ms() + 10 * 366 * 24 * 60 * 60 * 1000
        {
            return Err(AppError::new(
                "invalid_schedule",
                "计划时间或间隔超出允许范围",
            ));
        }
        let target = self.platform_target(&request.target_id)?;
        let selected: HashSet<_> = request.archive_ids.iter().collect();
        let mut archives = self.history()?.archives;
        archives.reverse();
        if selected
            .iter()
            .any(|id| !archives.iter().any(|archive| &archive.id == *id))
        {
            return Err(AppError::new(
                "archive_missing",
                "选中的历史归档不存在，请刷新后重试",
            ));
        }
        let jobs = self.jobs()?;
        let mut items = Vec::new();
        let mut count = 0;
        let start = request.start_at.max(now_ms());
        for archive in archives
            .into_iter()
            .filter(|archive| selected.contains(&archive.id))
        {
            let mut reason = self
                .validate_target(&target, &archive.publication)
                .err()
                .map(|error| error.message);
            let deliveries: Vec<_> = jobs
                .iter()
                .filter(|job| {
                    job.kind == JobKind::Upload
                        && job.archive_id.as_deref() == Some(&archive.id)
                        && (job.target_id.as_deref() == Some(&target.id)
                            || (matches!(
                                target.driver_id.as_str(),
                                "pixiv" | "x" | "xiaohongshu" | "bilibili"
                            ) && job.driver_id == target.driver_id))
                })
                .collect();
            if reason.is_none() && deliveries.iter().any(|job| job.status.is_active()) {
                reason = Some("该账号已有排队或执行中的任务".into());
            }
            if reason.is_none()
                && !request.force
                && deliveries
                    .iter()
                    .any(|job| job.status == JobStatus::Succeeded)
            {
                reason = Some("已成功上传到该账号".into());
            }
            if reason.is_none() {
                reason = archive
                    .publication
                    .assets
                    .iter()
                    .find_map(|asset| verify_asset(asset).err().map(|error| error.message));
            }
            let scheduled_at = if reason.is_none() {
                let time = start + count as u64 * u64::from(request.interval_minutes) * 60_000;
                count += 1;
                Some(time)
            } else {
                None
            };
            items.push(ReplayItem {
                archive_id: archive.id,
                title: archive.publication.title,
                eligible: reason.is_none(),
                reason,
                scheduled_at,
            });
        }
        Ok(ReplayPreview {
            skipped_count: items.len() - count,
            eligible_count: count,
            items,
        })
    }
    pub fn schedule_replay(&self, request: ReplayRequest) -> Result<ReplayResult> {
        let _submission = self
            .inner
            .submission
            .lock()
            .map_err(|_| AppError::new("queue", "提交锁异常"))?;
        // Recompute under a submission lock; a stale preview cannot create duplicate jobs.
        let preview = self.replay_preview(&request)?;
        if preview.eligible_count == 0 {
            return Err(AppError::new(
                "nothing_to_replay",
                "没有可补传的历史，请查看检查结果",
            ));
        }
        let target = self.platform_target(&request.target_id)?;
        let start_at = preview
            .items
            .iter()
            .find_map(|item| item.scheduled_at)
            .unwrap_or(request.start_at);
        let plan = ReplayPlan {
            id: uuid::Uuid::new_v4().to_string(),
            target_id: target.id.clone(),
            created_at: now_ms(),
            start_at,
            interval_minutes: request.interval_minutes,
            status: PlanStatus::Active,
            next_release_at: start_at,
            archive_ids: preview
                .items
                .iter()
                .filter(|item| item.eligible)
                .map(|item| item.archive_id.clone())
                .collect(),
            force: request.force,
        };
        let mut jobs = Vec::new();
        for item in preview.items.iter().filter(|item| item.eligible) {
            let archive = self.inner.store.archive(&item.archive_id)?;
            let mut job = new_job(
                archive.publication,
                &plan.id,
                &target.driver_id,
                JobKind::Upload,
            );
            job.status = JobStatus::Scheduled;
            job.target_id = Some(target.id.clone());
            job.archive_id = Some(archive.id);
            job.plan_id = Some(plan.id.clone());
            job.scheduled_at = item.scheduled_at;
            jobs.push(job);
        }
        self.inner.store.insert_plan(&plan, &jobs)?;
        self.inner.changed();
        Ok(ReplayResult { plan, preview })
    }
    pub fn set_plan_status(&self, id: String, status: PlanStatus) -> Result<()> {
        let tokens = self
            .inner
            .tokens
            .lock()
            .map_err(|_| AppError::new("queue", "队列锁异常"))?;
        for id in self.inner.store.set_plan_status(&id, status)? {
            if let Some(token) = tokens.get(&id) {
                token.store(true, Ordering::Release);
            }
        }
        self.inner.changed();
        Ok(())
    }
}
