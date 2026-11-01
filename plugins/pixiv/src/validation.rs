use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{fs::File, io::BufReader};
use upload_plugin_sdk::Publication;

pub const FILE_LIMIT: u64 = 32 * 1024 * 1024;
pub const TOTAL_LIMIT: u64 = 200 * 1024 * 1024;
pub fn validate(publication: &Publication) -> Vec<String> {
    let mut issues = Vec::new();
    if publication.title.trim().is_empty() || publication.title.encode_utf16().count() > 32 {
        issues.push("Pixiv 标题需要 1–32 字".into());
    }
    if publication.body.encode_utf16().count() > 3000 {
        issues.push("Pixiv 作品说明最多 3000 字".into());
    }
    if publication.assets.is_empty() || publication.assets.len() > 200 {
        issues.push("此插件支持每篇作品 1–200 张图片".into());
    }
    if publication.assets.iter().map(|a| a.size).sum::<u64>() > TOTAL_LIMIT {
        issues.push("Pixiv 每篇作品的图片总大小不能超过 200 MiB".into());
    }
    for asset in &publication.assets {
        if asset.size > FILE_LIMIT || asset.size == 0 {
            issues.push(format!(
                "{}：每张图片必须大于 0 且不超过 32 MiB",
                asset.name
            ));
            continue;
        }
        if inspect(&asset.path).is_err() {
            issues.push(format!(
                "{}：需要有效的 PNG、JPEG 或静态 GIF；解码内存上限 256 MiB",
                asset.name
            ));
        }
    }
    issues
}
pub fn mime(path: &str) -> std::io::Result<&'static str> {
    let format = ImageReader::open(path)?.with_guessed_format()?.format();
    match format {
        Some(ImageFormat::Png) => Ok("image/png"),
        Some(ImageFormat::Jpeg) => Ok("image/jpeg"),
        Some(ImageFormat::Gif) => Ok("image/gif"),
        _ => Err(std::io::Error::other("unsupported image")),
    }
}
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits
}
fn inspect(path: &str) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mime = mime(path)?;
    if mime == "image/gif" {
        let mut decoder = image::codecs::gif::GifDecoder::new(BufReader::new(File::open(path)?))?;
        decoder.set_limits(limits())?;
        let mut frames = decoder.into_frames();
        frames.next().ok_or("empty GIF")??;
        if frames.next().is_some() {
            return Err("animated GIF".into());
        }
    } else {
        let mut reader = ImageReader::open(path)?.with_guessed_format()?;
        reader.limits(limits());
        reader.decode()?;
    }
    Ok(())
}
