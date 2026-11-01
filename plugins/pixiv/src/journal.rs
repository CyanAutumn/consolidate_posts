use crate::{
    client::{PixivClient, Progress},
    config::{safe_id, Config},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::Write,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use upload_plugin_sdk::{AppError, Publication, Receipt, Result, UploadContext};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    fingerprint: String,
    user_id: String,
    stage: String,
    convert_key: Option<String>,
    receipt: Option<Receipt>,
}
fn persist(path: &Path, journal: &Journal) -> Result<()> {
    // Never overwrite in place: a partial write must not erase a previous conversion key.
    let temp = path.with_extension("tmp");
    let mut file = File::create(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(journal)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    Ok(())
}
pub fn upload(
    client: &PixivClient,
    publication: &Publication,
    context: &UploadContext<'_>,
    config: &Config,
    rating: &str,
    directory: &Path,
) -> Result<Receipt> {
    fs::create_dir_all(directory)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("upload.lock"))?;
    let waiting = Instant::now();
    loop {
        context.check_cancelled()?;
        match lock.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if waiting.elapsed() < Duration::from_secs(600) => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(TryLockError::WouldBlock) => {
                return Err(AppError::new(
                    "pixiv_busy",
                    "等待同账号投稿完成超过 10 分钟，请稍后重试",
                ))
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    if !safe_id(context.operation_id) {
        return Err(AppError::new("pixiv_protocol", "无效的投稿操作 ID"));
    }
    let history = directory.join("submissions");
    fs::create_dir_all(&history)?;
    let path = history.join(format!("{}.json", context.operation_id));
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&(
        publication.title.as_str(),
        publication.body.as_str(),
        publication
            .assets
            .iter()
            .map(|a| (&a.sha256, a.size))
            .collect::<Vec<_>>(),
    ))?);
    let fingerprint = format!("{:x}", hash.finalize());
    let mut journal = if path.exists() {
        let existing: Journal = serde_json::from_slice(&fs::read(&path)?).map_err(|_| {
            AppError::new(
                "pixiv_uncertain",
                "投稿记录损坏，请先在 Pixiv 网页检查作品，禁止直接重复投稿",
            )
        })?;
        if existing.user_id != config.user_id || existing.fingerprint != fingerprint {
            return Err(AppError::new(
                "pixiv_uncertain",
                "重试的账号或内容与原投稿不一致，请检查投稿记录",
            ));
        }
        existing
    } else {
        Journal {
            fingerprint,
            user_id: config.user_id.clone(),
            stage: "new".into(),
            convert_key: None,
            receipt: None,
        }
    };
    if let Some(receipt) = &journal.receipt {
        return Ok(receipt.clone());
    }
    if journal.stage == "submitting" {
        return Err(uncertain(&path));
    }
    if journal.stage == "failed" {
        return Err(AppError::new(
            "pixiv_conversion",
            "Pixiv 已报告图片处理失败；检查文件后创建新任务",
        ));
    }
    if matches!(journal.stage.as_str(), "new" | "rejected") {
        // An unknown older submission blocks new uploads to this account until checked manually.
        for entry in fs::read_dir(&history)? {
            let entry = entry?;
            if entry.path() == path || entry.path().extension().is_none_or(|s| s != "json") {
                continue;
            }
            let previous: Journal = serde_json::from_slice(&fs::read(entry.path())?)
                .map_err(|_| uncertain(&entry.path()))?;
            if matches!(previous.stage.as_str(), "submitting" | "processing") {
                return Err(uncertain(&entry.path()));
            }
        }
        context.check_cancelled()?;
        let init = client.init()?;
        journal.stage = "submitting".into();
        persist(&path, &journal)?;
        // Only a structured, explicit rejection permits a later manual POST retry.
        // Transport failures and unrecognized responses remain ambiguous.
        let key = match client.submit(publication, context, config, rating, &init) {
            Ok(key) => key,
            Err(error) if error.code == "pixiv_rejected" => {
                journal.stage = "rejected".into();
                persist(&path, &journal)?;
                return Err(error);
            }
            Err(error) => {
                return Err(AppError::new(
                    "pixiv_uncertain",
                    format!("{}；{}", error.message, uncertain(&path).message),
                ))
            }
        };
        journal.convert_key = Some(key);
        journal.stage = "processing".into();
        persist(&path, &journal)?;
    }
    let key = journal
        .convert_key
        .clone()
        .ok_or_else(|| uncertain(&path))?;
    for _ in 0..90 {
        context.check_cancelled()?;
        match client.progress(&key)? {
            Progress::Running => thread::sleep(Duration::from_secs(2)),
            Progress::Failed => {
                journal.stage = "failed".into();
                persist(&path, &journal)?;
                return Err(AppError::new(
                    "pixiv_conversion",
                    "Pixiv 图片处理失败，请在网页检查后重新投稿",
                ));
            }
            Progress::Complete(id) => {
                let receipt = Receipt {
                    location: format!("https://www.pixiv.net/artworks/{id}"),
                    files: Vec::new(),
                };
                journal.stage = "complete".into();
                journal.receipt = Some(receipt.clone());
                persist(&path, &journal)?;
                (context.progress)(publication.assets.iter().map(|a| a.size).sum())?;
                return Ok(receipt);
            }
        }
    }
    Err(AppError::new(
        "pixiv_processing",
        "Pixiv 仍在处理图片，稍后重试将继续查询此次投稿，不会重新上传",
    ))
}
fn uncertain(path: &Path) -> AppError {
    AppError::new("pixiv_uncertain", format!("已有结果待确认的投稿。请先重试原任务查询；若无法查询，在 Pixiv 作品管理中核对后按插件 README 处理。记录：{}", path.display()))
}
