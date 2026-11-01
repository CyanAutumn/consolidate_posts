import type { Asset, AssetKind, Draft, Driver, JobStatus } from "../types";
export const kindNames: Record<AssetKind, string> = {
  image: "图片",
  video: "视频",
  text: "文本",
  model: "模型",
  file: "其他文件",
};
export const statusNames: Record<JobStatus, string> = {
  scheduled: "待定时执行",
  waiting_archive: "等待归档",
  queued: "排队中",
  running: "上传中",
  cancelling: "正在取消",
  succeeded: "已完成",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断",
};
export const activeJob = (status: JobStatus) =>
  ["scheduled", "waiting_archive", "queued", "running", "cancelling"].includes(
    status,
  );
export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), 4);
  return `${(bytes / 1024 ** unit).toFixed(unit > 0 ? 1 : 0)} ${["B", "KB", "MB", "GB", "TB"][unit]}`;
}
// UI hints only. Rust performs authoritative validation against registered assets.
export function compatibility(
  driver: Driver,
  draft: Draft,
  assets: Asset[],
): string[] {
  const issues: string[] = [];
  const unsupported = [
    ...new Set(
      assets
        .filter((asset) => !driver.kinds.includes(asset.kind))
        .map((asset) => kindNames[asset.kind]),
    ),
  ];
  if (unsupported.length) issues.push(`不支持${unsupported.join("、")}`);
  if (draft.title && !driver.acceptsTitle) issues.push("不支持标题");
  if (draft.body && !driver.acceptsBody) issues.push("不支持正文");
  if (driver.maxFiles !== null && assets.length > driver.maxFiles)
    issues.push(`最多 ${driver.maxFiles} 个附件`);
  if (
    driver.maxFileBytes !== null &&
    assets.some((asset) => asset.size > driver.maxFileBytes!)
  )
    issues.push("附件超出大小限制");
  return issues;
}
