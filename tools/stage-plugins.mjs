import { cp, mkdir, readdir, lstat, access } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const profile = process.argv[2];
if (!["debug", "release"].includes(profile))
  throw new Error("profile 必须为 debug 或 release");
const target = process.env.CARGO_TARGET_DIR
  ? path.resolve(root, process.env.CARGO_TARGET_DIR)
  : path.join(root, "target");
const destination = path.join(target, profile, "plugins");
await mkdir(destination, { recursive: true });
const source = path.join(root, "plugins");
for (const entry of await readdir(source, { withFileTypes: true })) {
  if (!entry.isDirectory() || entry.name === "template") continue;
  const folder = path.join(source, entry.name);
  try {
    await access(path.join(folder, "plugin.json"));
  } catch (error) {
    if (error.code === "ENOENT") continue;
    throw error;
  }
  await cp(folder, path.join(destination, entry.name), {
    recursive: true,
    filter: async (item) =>
      !["target", ".git"].includes(path.basename(item)) &&
      // Browser publishing/login helpers need their pinned Node dependencies.
      (path.basename(item) !== "node_modules" || ["x", "pixiv", "xiaohongshu", "bilibili"].includes(entry.name)) &&
      !(await lstat(item)).isSymbolicLink(),
  });
}
console.log(
  `插件已复制到 ${destination}。分发时请将 plugins 与主程序放在一起。`,
);
