import { saveSession } from "./session.mjs";
import { readFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";
import { validate, validateSettings } from "./validation.mjs";
import { accountDirectory, atomicJson, readConfig, prepareJournal, PluginError, safeId, usernameValid, uncertain } from "./storage.mjs";
import { openBrowser, identity, verifyIdentity, composeAndPost, ensureCreator } from "./browser.mjs";

const writeProtocol = (value) => process.stdout.write(`${JSON.stringify({ protocolVersion: 1, ...value })}\n`);
// Third-party log output must never be mistaken for JSON Lines protocol.
console.log = console.info = (...values) => console.error(...values);

export async function upload(request, { launch = openBrowser, progress = () => {} } = {}) {
  const issues = validate(request.publication);
  if (issues.length) throw new PluginError("incompatible", issues.join("；"));
  const directory = accountDirectory(request);
  const config = await readConfig(directory);
  if (request.workspace.contentRating === "r18") throw new PluginError("incompatible", "哔哩哔哩插件不接受 R18 工作区投稿");
  const sensitive = false;
  const journal = await prepareJournal(request, config, directory, sensitive);
  if (journal.value.stage === "complete") {
    await progress(request.publication.assets.reduce((total, asset) => total + asset.size, 0));
    return journal.value.receipt;
  }
  let context;
  try {
    context = await launch(path.join(directory, "profile"), config.headless);
    const page = context.pages()[0] ?? await context.newPage();

    await verifyIdentity(page, context, config);
    let settings;try{settings=JSON.parse(await readFile(path.join(directory,"posting.json"),"utf8"));}catch{throw new PluginError("bilibili_settings","请先填写哔哩哔哩投稿设置中的分区、标签及自制/转载");}
    const problems=validateSettings(settings);if(problems.length)throw new PluginError("bilibili_settings",problems.join("；"));
    return await composeAndPost(page, request, {...config,...settings}, journal, { progress });
  } catch (error) {
    if (journal.value.stage === "submitting") throw new PluginError("bilibili_uncertain", `${error instanceof PluginError ? error.message : "哔哩哔哩浏览器发布中断"}；${uncertain(journal.file).message}`);
    if (error instanceof PluginError) throw error;
    throw new PluginError("bilibili_browser", "哔哩哔哩网页加载或媒体处理失败，请检查网络并点击平台登录中的登录核对登录状态");
  } finally { await context?.close().catch(() => {}); }
}

export async function checkLogin(directory, { launch = openBrowser, config: suppliedConfig } = {}) {
  const config = suppliedConfig ?? await readConfig(directory);
  const context = await launch(path.join(directory, "profile"), true);
  try {
    const page = context.pages()[0] ?? await context.newPage();
    await page.goto("https://member.bilibili.com/platform/home", { waitUntil: "domcontentloaded" });
    await verifyIdentity(page, context, config);
    await ensureCreator(context);
    return { connected: true, username: config.username };
  } finally { await context.close().catch(() => {}); }
}

async function login(args) {
 const {values}=parseArgs({args,options:{"data-dir":{type:"string"},"target-id":{type:"string",default:"bilibili"}}});
 if(!path.isAbsolute(values["data-dir"]??"")||!safeId(values["target-id"]))throw new PluginError("bilibili_config","账号目录无效");
 const directory=path.join(values["data-dir"],"plugin-data/bilibili",values["target-id"]);await mkdir(directory,{recursive:true});
 const context=await openBrowser(path.join(directory,"profile"),false,{restore:false});
 try{const page=context.pages()[0]??await context.newPage();await page.goto("https://passport.bilibili.com/login",{waitUntil:"domcontentloaded"});
 console.error("请在浏览器扫码登录哔哩哔哩，随后在创作者中心完成登录与验证。不会发布笔记。");
 let actual;const deadline=Date.now()+600000;
 while(Date.now()<deadline){if(page.isClosed())throw new PluginError("bilibili_login","登录窗口已关闭");try{const cookies=await context.cookies("https://www.bilibili.com/");if(!cookies.some(c=>c.name==="SESSDATA"&&c.value))throw Error("pending");if(new URL(page.url()).hostname!=="www.bilibili.com")await page.goto("https://www.bilibili.com/",{waitUntil:"domcontentloaded"});actual=await identity(page,context);break;}catch{await sleep(1000);}}
 if(!actual)throw new PluginError("bilibili_login","等待扫码登录超时");
 let previous;try{previous=JSON.parse(await readFile(path.join(directory,"config.json"),"utf8"));}catch(e){if(e.code!=="ENOENT")throw new PluginError("bilibili_config","原账号配置损坏，请先备份检查");}
 if(previous&&previous.userId!==actual.userId)throw new PluginError("bilibili_account","当前登录账号与保存账号不一致，未覆盖配置");
 await page.goto("https://member.bilibili.com/platform/home",{waitUntil:"domcontentloaded"});let creator=false;
 while(Date.now()<deadline){if(page.isClosed())throw new PluginError("bilibili_login","登录窗口已关闭");try{await ensureCreator(context);creator=true;break;}catch{await sleep(1000);}}
 if(!creator)throw new PluginError("bilibili_login","创作者中心登录未完成");
 const saved={...actual,headless:true,sensitive:false};await saveSession(context,path.join(directory,"profile"));
 await context.close();await checkLogin(directory,{config:saved});await atomicJson(path.join(directory,"config.json"),saved);console.error("哔哩哔哩登录成功，已验证登录态可以重新打开复用。");
 }finally{await context.close().catch(()=>{});}
}

async function readRequest() {
  const chunks = [];
  let length = 0;
  for await (const chunk of process.stdin) {
    chunks.push(chunk); length += chunk.length;
    if (length > 2 * 1024 * 1024) throw new PluginError("bilibili_protocol", "插件请求超过 2 MiB");
  }
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8")); }
  catch { throw new PluginError("bilibili_protocol", "插件请求 JSON 无效"); }
}
export async function main(args) {
  if (args[0] === "--check") {
    try {
      const { values } = parseArgs({ args: args.slice(1), options: { "data-dir": { type: "string" }, "target-id": { type: "string", default: "bilibili" } } });
      if (!path.isAbsolute(values["data-dir"] ?? "") || !safeId(values["target-id"])) throw new PluginError("bilibili_config", "账号目录无效");
      const result = await checkLogin(path.join(values["data-dir"], "plugin-data/bilibili", values["target-id"]));
      process.stdout.write(JSON.stringify(result) + "\n");
    } catch (error) { console.error(error instanceof PluginError ? error.message : "哔哩哔哩登录检查失败，请检查网络或重新登录"); process.exitCode = 1; }
    return;
  }
  if (args[0] === "--login") {
    try { await login(args.slice(1)); }
    catch (error) { console.error(error instanceof PluginError ? error.message : "哔哩哔哩登录配置失败，请检查运行依赖和网络"); process.exitCode = 1; }
    return;
  }
  if (args[0] !== "--upload-hub-plugin") { console.error("请由 Upload Hub 加载插件，或从平台登录打开登录窗口。"); process.exitCode = 1; return; }
  try {
    const request = await readRequest();
    if (request.protocolVersion !== 1 || request.pluginId !== "bilibili") throw new PluginError("bilibili_protocol", "插件协议版本或 ID 无效");
    if (request.operation === "validate") writeProtocol({ type: "validation", issues: validate(request.publication) });
    else if (request.operation === "upload") writeProtocol({ type: "result", receipt: await upload(request, { progress: (completedBytes) => writeProtocol({ type: "progress", completedBytes }) }) });
    else throw new PluginError("bilibili_protocol", "不支持的插件操作");
  } catch (error) { writeProtocol({ type: "error", error: { code: error instanceof PluginError ? error.code : "bilibili_runtime", message: error instanceof PluginError ? error.message : "哔哩哔哩插件执行失败，请检查运行依赖和插件配置" } }); }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main(process.argv.slice(2));
