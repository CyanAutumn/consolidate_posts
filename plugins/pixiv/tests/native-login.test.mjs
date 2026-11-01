import assert from "node:assert/strict";
import { test } from "node:test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, writeFile, copyFile, mkdir, rm, readdir } from "node:fs/promises";
import { setTimeout as sleep } from "node:timers/promises";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const source = fileURLToPath(new URL("../plugin.exe", import.meta.url));
const wizard = fileURLToPath(new URL("../configure.ps1", import.meta.url));
const defaultSettings = { userId: "", tags: ["测试标签"], aiType: "notAiGenerated", visibility: "private", rating: "general", original: false, allowTagEdit: false, allowComment: true };
const account = (root) => path.join(root, "plugin-data/pixiv/pixiv");
function start(entry, root, settings = defaultSettings) {
  const child = spawn(entry, ["--browser-login", "--data-dir", root, "--target-id", "pixiv"], { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  let stdout = "", stderr = "";
  child.stdout.on("data", (bytes) => { stdout += bytes.toString("utf8"); });
  child.stderr.on("data", (bytes) => { stderr += bytes.toString("utf8"); });
  const done = once(child, "close");
  child.stdin.end(JSON.stringify(settings));
  return { child, done, output: () => ({ stdout, stderr }) };
}
async function sandbox(fn) {
  const root = await mkdtemp(path.join(os.tmpdir(), "pixiv-native-login-"));
  const entry = path.join(root, "plugin.exe");
  try { await copyFile(source, entry); await fn(root, entry); }
  finally { await rm(root, { recursive: true, maxRetries: 10, retryDelay: 100 }); }
}
const fakeLogin = (userId = "123") => `process.stdout.write(JSON.stringify({userId:${JSON.stringify(userId)},session:${JSON.stringify(`${userId}_fake_browser_secret`)}}));`;
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };
async function until(predicate) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) { if (await predicate()) return; await sleep(50); }
  assert.fail("native browser login probe timed out");
}

test("native browser result creates the expected config and only encrypted credentials", { skip: process.platform !== "win32" }, async () => sandbox(async (root, entry) => {
  await writeFile(path.join(root, "browser-login.mjs"), fakeLogin());
  const run = start(entry, root);
  assert.equal((await run.done)[0], 0, run.output().stderr);
  assert.deepEqual(JSON.parse(await readFile(path.join(account(root), "config.json"), "utf8")), { ...defaultSettings, userId: "123" });
  const encrypted = await readFile(path.join(account(root), "session.dpapi"), "utf8");
  assert.match(encrypted, /^[a-f0-9]+$/);
  assert.ok(!JSON.stringify(run.output()).includes("fake_browser_secret"));
  assert.deepEqual((await readdir(account(root))).sort(), ["config.json", "session.dpapi", "upload.lock"]);
}));

test("the full native-to-browser pipeline extracts a simulated login and encrypts it", { skip: process.platform !== "win32", timeout: 30_000 }, async () => sandbox(async (root, entry) => {
  const state = { props: { pageProps: { isLoggedIn: true, gaUserData: { userId: "123" }, serverSerializedPreloadedState: "{}" } } };
  const html = `<!doctype html><script id="__NEXT_DATA__" type="application/json">${JSON.stringify(state)}</script>`;
  await writeFile(path.join(root, "browser-login.mjs"), `
    const {acquireLogin, openLoginContext} = await import(${JSON.stringify(new URL("../browser-login.mjs", import.meta.url).href)});
    const result = await acquireLogin({timeoutMs:3000,pollMs:20,launch:async()=>{
      const context = await openLoginContext({headless:true});
      await context.route('**/*',route=>{
        const host = new URL(route.request().url()).hostname;
        if(host==='accounts.pixiv.net') return route.fulfill({contentType:'text/html',body:'<script>location.replace("https://www.pixiv.net/")</script>'});
        if(host==='www.pixiv.net') return route.fulfill({contentType:'text/html',body:${JSON.stringify(html)}});
        return route.abort();
      });
      await context.addCookies([{name:'PHPSESSID',value:'123_fake_browser_secret',domain:'.pixiv.net',path:'/',secure:true,httpOnly:true}]);
      return context;
    }});
    process.stdout.write(JSON.stringify(result));
  `);
  const run = start(entry, root);
  try {
    assert.equal((await run.done)[0], 0, run.output().stderr);
    assert.equal(JSON.parse(await readFile(path.join(account(root), "config.json"), "utf8")).userId, "123");
    assert.ok(!JSON.stringify(run.output()).includes("fake_browser_secret"));
    assert.ok(!String(await readFile(path.join(account(root), "session.dpapi"))).includes("fake_browser_secret"));
  } finally { run.child.kill(); }
}));

test("wrong-account or cancelled login cannot replace an existing encrypted session", { skip: process.platform !== "win32" }, async () => sandbox(async (root, entry) => {
  await mkdir(account(root), { recursive: true });
  const original = { ...defaultSettings, userId: "123" };
  await writeFile(path.join(account(root), "config.json"), JSON.stringify(original));
  await writeFile(path.join(account(root), "session.dpapi"), "old-encrypted-value");
  for (const helper of [fakeLogin("456"), "process.exitCode = 1;"]) {
    await writeFile(path.join(root, "browser-login.mjs"), helper);
    const run = start(entry, root, original);
    assert.notEqual((await run.done)[0], 0);
    assert.equal(await readFile(path.join(account(root), "session.dpapi"), "utf8"), "old-encrypted-value");
    assert.deepEqual(JSON.parse(await readFile(path.join(account(root), "config.json"), "utf8")), original);
  }
}));

test("PowerShell browser wizard auto-fills the user ID and preserves settings on re-login", { skip: process.platform !== "win32" }, async () => sandbox(async (root, entry) => {
  await copyFile(wizard, path.join(root, "configure.ps1"));
  await writeFile(path.join(root, "browser-login.mjs"), fakeLogin());
  const runWizard = async (args) => {
    const child = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", path.join(root, "configure.ps1"), "-DataDir", root, ...args], { windowsHide: true });
    let logs = "";
    child.stdout.on("data", (bytes) => { logs += bytes; });
    child.stderr.on("data", (bytes) => { logs += bytes; });
    const [code] = await once(child, "close");
    assert.equal(code, 0, logs);
    assert.ok(!logs.includes("fake_browser_secret"));
  };
  await runWizard(["-Tags", "test", "-AiType", "aiGenerated", "-Visibility", "public", "-Rating", "general", "-Original", "-BrowserLogin"]);
  const before = JSON.parse(await readFile(path.join(account(root), "config.json"), "utf8"));
  assert.equal(before.userId, "123");
  assert.equal(before.visibility, "public");
  assert.equal(before.original, true);
  await runWizard(["-BrowserLogin"]);
  assert.deepEqual(JSON.parse(await readFile(path.join(account(root), "config.json"), "utf8")), before);
}));

test("cancelling the login launcher also closes its Node worker and descendants", { skip: process.platform !== "win32", timeout: 20_000 }, async () => sandbox(async (root, entry) => {
  await writeFile(path.join(root, "browser-login.mjs"), `
    import {spawn} from 'node:child_process';
    import {writeFile} from 'node:fs/promises';
    const nested = spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{windowsHide:true,stdio:'ignore'});
    await writeFile('pids.json',JSON.stringify([process.pid,nested.pid]));
    setInterval(()=>{},1000);
  `);
  const run = start(entry, root);
  let pids;
  try {
    await until(async () => { try { pids = JSON.parse(await readFile(path.join(root, "pids.json"), "utf8")); return true; } catch { return false; } });
    run.child.kill(); await run.done;
    await until(() => pids.every((pid) => !alive(pid)));
    await assert.rejects(readFile(path.join(account(root), "session.dpapi")), { code: "ENOENT" });
  } finally { run.child.kill(); for (const pid of pids ?? []) if (alive(pid)) process.kill(pid); }
}));


test("login is independent of publication tags and never guesses an AI label", {skip:process.platform!=="win32"}, async()=>sandbox(async(root,entry)=>{
  await writeFile(path.join(root,"browser-login.mjs"),fakeLogin());
  const run=start(entry,root,{...defaultSettings,tags:[],aiType:""});
  assert.equal((await run.done)[0],0,run.output().stderr);
  const saved=JSON.parse(await readFile(path.join(account(root),"config.json"),"utf8"));
  assert.equal(saved.userId,"123");assert.equal(saved.aiType,"");assert.deepEqual(saved.tags,[]);
}));
