import { mkdir, open, readFile, rename, readdir } from "node:fs/promises";
import path from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { textFor } from "./validation.mjs";

export class PluginError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
export const safeId = (value) => typeof value === "string" && /^[A-Za-z0-9-]{1,128}$/.test(value);
export const usernameValid = v => typeof v === "string" && v.length > 0 && v.length <= 100;
export const numericId = v => typeof v === "string" && /^[0-9]{1,25}$/.test(v);


export async function atomicJson(file, data) {
  await mkdir(path.dirname(file), { recursive: true });
  const temp = `${file}.${randomUUID()}.tmp`;
  const output = await open(temp, "wx", 0o600);
  try { await output.writeFile(JSON.stringify(data, null, 2)); await output.sync(); }
  finally { await output.close(); }
  await rename(temp, file);
}
export function accountDirectory(request) {
  if (!request.workspace || !path.isAbsolute(request.workspace.dataDir ?? "") || !safeId(request.target?.id) || request.target.driverId !== "bilibili") throw new PluginError("bilibili_config", "哔哩哔哩需要有效的工作区上下文和上传目标");
  if (!safeId(request.operationId ?? request.jobId) || !safeId(request.jobId) || !path.isAbsolute(request.outputDir ?? "")) throw new PluginError("bilibili_protocol", "哔哩哔哩上传操作 ID 或输出目录无效");
  return path.join(request.workspace.dataDir, request.target.id);
}
export async function readConfig(directory) {
  try {
    const config = JSON.parse((await readFile(path.join(directory, "config.json"), "utf8")).replace(/^\uFEFF/, ""));
    if (!usernameValid(config.username) || !numericId(config.userId) || typeof config.headless !== "boolean" || typeof config.sensitive !== "boolean") throw new Error("invalid");
    return config;
  } catch {
    throw new PluginError("bilibili_config", "请在“平台登录”点击哔哩哔哩的“登录”，在打开的浏览器中完成登录后再发布");
  }
}
export function uncertain(file) {
  return new PluginError("bilibili_uncertain", `哔哩哔哩发布结果待确认，已阻止重复发布。请先在哔哩哔哩创作者中心核对，再按插件 README 恢复。记录：${file}`);
}
function validJournal(value) {
  return value && /^[a-f0-9]{64}$/.test(value.fingerprint ?? "") && numericId(value.userId)
    && ["prepared", "submitting", "complete"].includes(value.stage)
    && (value.stage !== "complete" || /^https:\/\/www\.bilibili\.com\/video\/(?:BV[A-Za-z0-9]{10}|av[0-9]+)$/.test(value.receipt?.location ?? ""));
}
export async function prepareJournal(request, config, directory, sensitive) {
  const folder = path.join(directory, "submissions");
  await mkdir(folder, { recursive: true });
  const file = path.join(folder, `${request.operationId ?? request.jobId}.json`);
  const fingerprint = createHash("sha256").update(JSON.stringify({
    text: textFor(request.publication), sensitive,
    assets: request.publication.assets.map((asset) => [asset.sha256, asset.size, asset.name]),
  })).digest("hex");
  let value;
  try { value = JSON.parse(await readFile(file, "utf8")); }
  catch (error) { if (error.code !== "ENOENT") throw uncertain(file); }
  if (value && (!validJournal(value) || value.fingerprint !== fingerprint || value.userId !== config.userId)) throw uncertain(file);
  if (value?.stage === "submitting") throw uncertain(file);
  if (!value) {
    for (const name of await readdir(folder)) {
      if (!name.endsWith(".json")) continue;
      let previous;
      try { previous = JSON.parse(await readFile(path.join(folder, name), "utf8")); } catch { throw uncertain(path.join(folder, name)); }
      if (!validJournal(previous) || previous.stage === "submitting") throw uncertain(path.join(folder, name));
    }
    value = { fingerprint, userId: config.userId, stage: "prepared", receipt: null };
    await atomicJson(file, value);
  }
  return {
    value, file,
    async submitting() { value.stage = "submitting"; await atomicJson(file, value); },
    async rejected() { value.stage = "prepared"; await atomicJson(file, value); },
    async complete(receipt) { value.stage = "complete"; value.receipt = receipt; await atomicJson(file, value); },
  };
}
