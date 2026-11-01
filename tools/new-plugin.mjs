import { cp, readFile, writeFile, access } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [id, name = id] = process.argv.slice(2);
if (
  !id ||
  !/^[a-z0-9][a-z0-9-]{0,63}$/.test(id) ||
  /^(con|prn|aux|nul|com[0-9]|lpt[0-9])$/.test(id) ||
  id === "local-archive"
) {
  throw new Error(
    '用法：npm run plugin:new -- my-platform "我的平台"；ID 使用小写字母、数字和短横线。',
  );
}
const destination = path.join(root, "plugins", id);
let exists = false;
try {
  await access(destination);
  exists = true;
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
if (exists) throw new Error(`插件目录已存在，不会覆盖：${destination}`);
const template = path.join(root, "plugins", "template");
await cp(template, destination, {
  recursive: true,
  force: false,
  errorOnExist: true,
  filter: (source) =>
    !["target", "plugin.exe", "Cargo.lock"].includes(path.basename(source)),
});
const manifest = JSON.parse(
  await readFile(path.join(destination, "plugin.json"), "utf8"),
);
Object.assign(manifest, { id, name, enabled: true });
await writeFile(
  path.join(destination, "plugin.json"),
  JSON.stringify(manifest, null, 2) + "\n",
);
for (const file of ["Cargo.toml", "src/main.rs"]) {
  const filename = path.join(destination, file);
  const text = (await readFile(filename, "utf8"))
    .replaceAll("example-uploader", id)
    .replaceAll("upload-plugin-example", `upload-plugin-${id}`);
  await writeFile(filename, text);
}
console.log(
  `已创建 ${destination}\n下一步：powershell -File "${path.join(destination, "build-plugin.ps1")}"\n模板执行本地导出；接入平台后修改 validate / upload。编译完成后重启 Upload Hub。`,
);
