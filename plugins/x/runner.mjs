import { readFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";
import { validate } from "./validation.mjs";
import { accountDirectory, atomicJson, readConfig, prepareJournal, PluginError, safeId, usernameValid, uncertain } from "./storage.mjs";
import { openBrowser, identity, verifyIdentity, composeAndPost } from "./browser.mjs";

const writeProtocol = (value) => process.stdout.write(`${JSON.stringify({ protocolVersion: 1, ...value })}\n`);
// Third-party log output must never be mistaken for JSON Lines protocol.
console.log = console.info = (...values) => console.error(...values);

export async function upload(request, { launch = openBrowser, progress = () => {} } = {}) {
  const issues = validate(request.publication);
  if (issues.length) throw new PluginError("incompatible", issues.join("；"));
  const directory = accountDirectory(request);
  const config = await readConfig(directory);
  const sensitive = request.workspace.contentRating === "r18" || config.sensitive;
  const journal = await prepareJournal(request, config, directory, sensitive);
  if (journal.value.stage === "complete") {
    await progress(request.publication.assets.reduce((total, asset) => total + asset.size, 0));
    return journal.value.receipt;
  }
  let context;
  try {
    context = await launch(path.join(directory, "profile"), config.headless);
    const page = context.pages()[0] ?? await context.newPage();
    await page.goto("https://x.com/compose/post", { waitUntil: "domcontentloaded" });
    await verifyIdentity(page, context, config);
    return await composeAndPost(page, request, config, journal, { sensitive, progress });
  } catch (error) {
    if (journal.value.stage === "submitting") throw new PluginError("x_uncertain", `${error instanceof PluginError ? error.message : "X 浏览器发布中断"}；${uncertain(journal.file).message}`);
    if (error instanceof PluginError) throw error;
    throw new PluginError("x_browser", "X 网页加载或媒体处理失败，请检查网络并运行账号配置命令核对登录状态");
  } finally { await context?.close().catch(() => {}); }
}

export async function checkLogin(directory, { launch = openBrowser } = {}) {
  const config = await readConfig(directory);
  const context = await launch(path.join(directory, "profile"), true);
  try {
    const page = context.pages()[0] ?? await context.newPage();
    await page.goto("https://x.com/home", { waitUntil: "domcontentloaded" });
    await verifyIdentity(page, context, config);
    return { connected: true, username: config.username };
  } finally { await context.close().catch(() => {}); }
}

async function login(args) {
  const { values } = parseArgs({ args, options: {
    "data-dir": { type: "string" }, "target-id": { type: "string", default: "x" },
    username: { type: "string" }, headed: { type: "boolean", default: false },
  }});
  const username = values.username?.replace(/^@/, "");
  if (!path.isAbsolute(values["data-dir"] ?? "") || !safeId(values["target-id"]) || (username && !usernameValid(username))) throw new PluginError("x_config", "工作区目录、目标 ID 或 X 用户名无效");
  const directory = path.join(values["data-dir"], "plugin-data/x", values["target-id"]);
  await mkdir(directory, { recursive: true });
  console.error("首次运行可能下载约 200 MB 的 CloakBrowser 组件。请在打开的浏览器中手动登录 X 并完成验证；本向导不会发帖。");
  const context = await openBrowser(path.join(directory, "profile"), false);
  try {
    const page = context.pages()[0] ?? await context.newPage();
    await page.goto("https://x.com/home", { waitUntil: "domcontentloaded" });
    const deadline = Date.now() + 600_000;
    while (Date.now() < deadline) {
      if (page.isClosed()) throw new PluginError("x_login", "登录窗口已关闭，尚未保存账号配置");
      let actual;
      try { actual = await identity(page, context, 1_000); } catch { await sleep(1_000); continue; }
      if (username && actual.username.toLowerCase() !== username.toLowerCase()) throw new PluginError("x_account", `当前登录的是 @${actual.username}，与指定的 @${username} 不一致；请在浏览器切换账号后重新配置`);
      let previous;
      try { previous = JSON.parse(await readFile(path.join(directory, "config.json"), "utf8")); } catch (error) { if (error.code !== "ENOENT") throw new PluginError("x_config", "现有 X 配置损坏；请备份并检查后再配置"); }
      if (previous && previous.userId !== actual.userId) throw new PluginError("x_account", "此目标原来属于另一个 X 账号，请在主程序新建目标后配置，避免混用发布记录");
      await atomicJson(path.join(directory, "config.json"), { ...actual, headless: !values.headed, sensitive: previous?.sensitive ?? false });
      console.error(`已保存 @${actual.username} 的独立浏览器登录态。默认${values.headed ? "显示浏览器窗口" : "无头发布"}。回到主程序启用 X 插件并选择此账号。`);
      return;
    }
    throw new PluginError("x_login", "等待登录超过 10 分钟，请重新运行配置命令");
  } finally { await context.close().catch(() => {}); }
}

async function readRequest() {
  const chunks = [];
  let length = 0;
  for await (const chunk of process.stdin) {
    chunks.push(chunk); length += chunk.length;
    if (length > 2 * 1024 * 1024) throw new PluginError("x_protocol", "插件请求超过 2 MiB");
  }
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8")); }
  catch { throw new PluginError("x_protocol", "插件请求 JSON 无效"); }
}
export async function main(args) {
  if (args[0] === "--check") {
    try {
      const { values } = parseArgs({ args: args.slice(1), options: { "data-dir": { type: "string" }, "target-id": { type: "string", default: "x" } } });
      if (!path.isAbsolute(values["data-dir"] ?? "") || !safeId(values["target-id"])) throw new PluginError("x_config", "账号目录无效");
      const result = await checkLogin(path.join(values["data-dir"], "plugin-data/x", values["target-id"]));
      process.stdout.write(JSON.stringify(result) + "\n");
    } catch (error) { console.error(error instanceof PluginError ? error.message : "X 登录检查失败，请检查网络或重新登录"); process.exitCode = 1; }
    return;
  }
  if (args[0] === "--login") {
    try { await login(args.slice(1)); }
    catch (error) { console.error(error instanceof PluginError ? error.message : "X 登录配置失败，请检查运行依赖和网络"); process.exitCode = 1; }
    return;
  }
  if (args[0] !== "--upload-hub-plugin") { console.error("请由 Upload Hub 加载插件，或运行 configure.ps1 登录。"); process.exitCode = 1; return; }
  try {
    const request = await readRequest();
    if (request.protocolVersion !== 1 || request.pluginId !== "x") throw new PluginError("x_protocol", "插件协议版本或 ID 无效");
    if (request.operation === "validate") writeProtocol({ type: "validation", issues: validate(request.publication) });
    else if (request.operation === "upload") writeProtocol({ type: "result", receipt: await upload(request, { progress: (completedBytes) => writeProtocol({ type: "progress", completedBytes }) }) });
    else throw new PluginError("x_protocol", "不支持的插件操作");
  } catch (error) { writeProtocol({ type: "error", error: { code: error instanceof PluginError ? error.code : "x_runtime", message: error instanceof PluginError ? error.message : "X 插件执行失败，请检查运行依赖和插件配置" } }); }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main(process.argv.slice(2));
