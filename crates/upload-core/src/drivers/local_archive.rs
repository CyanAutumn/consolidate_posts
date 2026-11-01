use super::{UploadContext, UploadDriver};
use crate::model::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    time::{Duration, Instant},
};

pub struct LocalArchive;
impl UploadDriver for LocalArchive {
    fn id(&self) -> &'static str {
        "local-archive"
    }
    fn validate(&self, _: &Publication) -> Vec<String> {
        Vec::new()
    }
    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        context.check_cancelled()?;
        // Only engine-generated UUIDs are accepted as output directory components.
        uuid::Uuid::parse_str(context.job_id)
            .map_err(|_| AppError::new("invalid_job", "无效任务 ID"))?;
        fs::create_dir_all(context.archive_root)?;
        let staging = context
            .archive_root
            .join(format!(".partial-{}", context.job_id));
        let destination = context.archive_root.join(context.job_id);
        fs::create_dir(&staging)?;
        let result = (|| {
            let mut copied = 0;
            let mut files = Vec::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut last_report = Instant::now();
            for (index, asset) in publication.assets.iter().enumerate() {
                context.check_cancelled()?;
                verify_asset(asset)?;
                let safe_name: String = asset
                    .name
                    .chars()
                    .map(|c| {
                        if c.is_control() || "<>:\"/\\|?*".contains(c) {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect();
                let file_name = format!("{:03}_{}", index + 1, safe_name);
                let mut input = File::open(&asset.path)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(staging.join(&file_name))?;
                let mut hash = Sha256::new();
                let mut file_size = 0;
                loop {
                    context.check_cancelled()?;
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    output.write_all(&buffer[..count])?;
                    hash.update(&buffer[..count]);
                    copied += count as u64;
                    file_size += count as u64;
                    if last_report.elapsed() >= Duration::from_millis(120) {
                        (context.progress)(copied)?;
                        last_report = Instant::now();
                    }
                }
                output.sync_all()?;
                verify_asset(asset)?;
                if file_size != asset.size {
                    return Err(AppError::new("source_changed", "读取过程中源文件发生变化"));
                }
                let checksum = format!("{:x}", hash.finalize());
                if asset
                    .sha256
                    .as_ref()
                    .is_some_and(|expected| expected != &checksum)
                {
                    return Err(AppError::new(
                        "archive_corrupt",
                        format!("归档校验失败：{}", asset.name),
                    ));
                }
                files.push(ArchivedFile {
                    asset_id: asset.id.clone(),
                    file_name,
                    size: file_size,
                    sha256: checksum,
                });
            }
            context.check_cancelled()?;
            // Portable manifest deliberately excludes original absolute source paths.
            let manifest = serde_json::json!({ "version": 1, "jobId": context.job_id, "title": publication.title, "body": publication.body, "files": files });
            let mut output = File::create(staging.join("manifest.json"))?;
            output.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
            output.sync_all()?;
            drop(output); // Windows cannot rename the directory while this file is open.
            (context.progress)(copied)?;
            context.check_cancelled()?;
            fs::rename(&staging, &destination)?;
            Ok(Receipt {
                location: destination.to_string_lossy().into(),
                files,
            })
        })();
        if result.is_err() {
            // This directory was created above, under our archive root, for this job only.
            if let Err(error) = fs::remove_dir_all(&staging) {
                eprintln!("清理临时归档失败 {}: {error}", staging.display());
            }
        }
        result
    }
}
