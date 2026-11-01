import { openSync, readSync, closeSync, statSync } from "node:fs";
import twitterText from "twitter-text";

export function textFor(publication) {
  return [publication.title.trim(), publication.body.trim()].filter(Boolean).join("\n\n");
}

export function mediaType(asset) {
  const header = Buffer.alloc(32);
  const file = openSync(asset.path, "r");
  let count;
  try { count = readSync(file, header, 0, header.length, 0); } finally { closeSync(file); }
  if (count >= 8 && header.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]))) return "image/png";
  if (count >= 3 && header[0] === 255 && header[1] === 216 && header[2] === 255) return "image/jpeg";
  if (count >= 6 && ["GIF87a", "GIF89a"].includes(header.toString("ascii", 0, 6))) return "image/gif";
  if (count >= 12 && header.toString("ascii", 0, 4) === "RIFF" && header.toString("ascii", 8, 12) === "WEBP") return "image/webp";
  if (count >= 12 && header.toString("ascii", 4, 8) === "ftyp"
      && /^(isom|iso[2-9]|mp4[12]|avc1|M4V |MSNV)$/.test(header.toString("ascii", 8, 12))) return "video/mp4";
  throw new Error("unsupported media");
}

export function validate(publication) {
  const issues = [];
  if (!publication || typeof publication.title !== "string" || typeof publication.body !== "string" || !Array.isArray(publication.assets)) return ["X 发布内容格式无效"];
  const text = textFor(publication);
  if (!text && !publication.assets.length) issues.push("X 至少需要文字或媒体附件");
  if (text && !twitterText.parseTweet(text).valid) issues.push("X 文字超过普通帖文的 280 加权字符，或含无效字符（中文通常每字计 2，链接计 23）；标题和正文合并计算");
  if (publication.assets.length > 4) issues.push("X 每篇最多四张图片，或单个 GIF / MP4 视频");
  const types = [];
  for (const asset of publication.assets) {
    try {
      const stat = statSync(asset.path);
      if (!stat.isFile() || !Number.isSafeInteger(asset.size) || stat.size !== asset.size || asset.size === 0) throw new Error("changed");
      const type = mediaType(asset);
      types.push(type);
      const video = type === "video/mp4";
      if (asset.kind !== (video ? "video" : "image")) issues.push(`${asset.name}：附件类型与实际媒体不一致`);
      const limit = video ? 512 * 1024 * 1024 : type === "image/gif" ? 15 * 1024 * 1024 : 5 * 1024 * 1024;
      if (asset.size > limit) issues.push(`${asset.name}：图片上限 5 MiB，GIF 上限 15 MiB，本插件 MP4 上限 512 MiB`);
    } catch {
      issues.push(`${asset.name ?? "附件"}：文件不存在、大小已变化，或不是有效的 PNG / JPEG / WebP / GIF / MP4 文件头`);
    }
  }
  if (types.some((type) => ["image/gif", "video/mp4"].includes(type)) && publication.assets.length !== 1) issues.push("GIF 或视频必须单独发布，不能与其他附件混合");
  return issues;
}
