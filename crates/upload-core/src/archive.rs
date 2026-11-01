use crate::model::*;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
};

/// Rebuilds references from the copies, never from the original import paths.
/// Also migrates v1 successful local-archive jobs without recopying their files.
pub fn record_from_receipt(job: &Job, receipt: Receipt) -> Result<ArchiveRecord> {
    let mut assets = Vec::new();
    for source in &job.publication.assets {
        let file = receipt
            .files
            .iter()
            .find(|file| file.asset_id == source.id)
            .ok_or_else(|| AppError::new("archive_invalid", "归档清单缺少附件"))?;
        let components: Vec<_> = Path::new(&file.file_name).components().collect();
        if components.len() != 1 || !matches!(components[0], Component::Normal(_)) {
            return Err(AppError::new("archive_invalid", "归档文件名无效"));
        }
        let path = Path::new(&receipt.location).join(&file.file_name);
        // Missing old copies remain visible in history; preflight will explain why they cannot replay.
        let modified = path
            .metadata()
            .ok()
            .and_then(|meta| modified_ms(&meta).ok())
            .unwrap_or(0);
        assets.push(Asset {
            id: source.id.clone(),
            path: path.to_string_lossy().into(),
            name: source.name.clone(),
            kind: source.kind,
            size: file.size,
            modified_ms: modified,
            sha256: Some(file.sha256.clone()),
        });
    }
    Ok(ArchiveRecord {
        id: job.id.clone(),
        source_job_id: job.id.clone(),
        created_at: job.created_at,
        publication: Publication {
            title: job.publication.title.clone(),
            body: job.publication.body.clone(),
            assets,
        },
        receipt,
    })
}

pub fn verify_contents(publication: &Publication, cancelled: &AtomicBool) -> Result<()> {
    let mut buffer = vec![0u8; 1024 * 1024];
    for asset in &publication.assets {
        verify_asset(asset)?;
        let Some(expected) = &asset.sha256 else {
            return Err(AppError::new("archive_invalid", "归档缺少文件校验值"));
        };
        let mut file = File::open(&asset.path)?;
        let mut hash = Sha256::new();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(AppError::cancelled());
            }
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if format!("{:x}", hash.finalize()) != *expected {
            return Err(AppError::new(
                "archive_corrupt",
                format!("归档校验失败：{}", asset.name),
            ));
        }
        verify_asset(asset)?;
    }
    Ok(())
}
