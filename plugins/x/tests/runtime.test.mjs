import assert from "node:assert/strict";
import { test } from "node:test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { access, copyFile, mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { setTimeout as sleep } from "node:timers/promises";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const executable = fileURLToPath(new URL("../plugin.exe", import.meta.url));
const request = { protocolVersion: 1, pluginId: "x", operation: "validate", publication: { title: "", body: "native 入口测试", assets: [] } };
function start(entry, input) {
  const child = spawn(entry, ["--upload-hub-plugin"], { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  let stdout = "", stderr = "";
  child.stdout.on("data", (bytes) => { stdout += bytes.toString("utf8"); });
  child.stderr.on("data", (bytes) => { stderr += bytes.toString("utf8"); });
  const ended = once(child, "close");
  child.stdin.end(`${JSON.stringify(input)}\n`);
  return { child, ended, output: () => ({ stdout, stderr }) };
}
async function until(predicate) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) { if (await predicate()) return; await sleep(50); }
  assert.fail("native process probe timed out");
}
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };

test("native .exe entry returns exactly one JSON validation response", { skip: process.platform !== "win32" }, async () => {
  await access(executable);
  const run = start(executable, request);
  assert.equal((await run.ended)[0], 0, run.output().stderr);
  const lines = run.output().stdout.trim().split("\n");
  assert.equal(lines.length, 1);
  assert.deepEqual(JSON.parse(lines[0]), { protocolVersion: 1, type: "validation", issues: [] });
});

test("native Windows job can launch and close actual headless CloakBrowser", { skip: process.platform !== "win32", timeout: 30_000 }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "x-native-browser-"));
  const entry = path.join(directory, "plugin.exe");
  let run;
  try {
    await copyFile(executable, entry);
    await writeFile(path.join(directory, "runner.mjs"), `
      import path from 'node:path';
      const {openBrowser} = await import(${JSON.stringify(new URL("../browser.mjs", import.meta.url).href)});
      const context = await openBrowser(path.join(process.cwd(),'profile'),true);
      try {
        const page = await context.newPage();
        await page.goto('data:text/html,<h1>local native browser</h1>');
        const content = await page.locator('h1').textContent();
        console.log(JSON.stringify({protocolVersion:1,type:'validation',issues:content==='local native browser'?[]:['browser failure']}));
      } finally {await context.close();}
    `);
    run = start(entry, request);
    assert.equal((await run.ended)[0], 0, run.output().stderr);
    assert.deepEqual(JSON.parse(run.output().stdout.trim()), { protocolVersion: 1, type: "validation", issues: [] });
  } finally {
    run?.child.kill();
    await rm(directory, { recursive: true, maxRetries: 10, retryDelay: 100 });
  }
});

test("killing the native launcher terminates its worker and grandchildren", { skip: process.platform !== "win32", timeout: 20_000 }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "x-runtime-tree-"));
  const entry = path.join(directory, "plugin.exe");
  let run, pids;
  try {
    await copyFile(executable, entry);
    await writeFile(path.join(directory, "runner.mjs"), `
      import {spawn} from 'node:child_process';
      import {writeFile} from 'node:fs/promises';
      const nested = spawn(process.execPath, ['-e','setInterval(()=>{},1000)'], {windowsHide:true,stdio:'ignore'});
      await writeFile('pids.json', JSON.stringify([process.pid,nested.pid]));
      setInterval(()=>{},1000);
    `);
    run = start(entry, request);
    await until(async () => { try { pids = JSON.parse(await readFile(path.join(directory, "pids.json"), "utf8")); return true; } catch { return false; } });
    assert.ok(pids.every(alive));
    run.child.kill();
    await run.ended;
    await until(() => pids.every((pid) => !alive(pid)));
  } finally {
    run?.child.kill();
    for (const pid of pids ?? []) { if (alive(pid)) process.kill(pid); }
    await rm(directory, { recursive: true, maxRetries: 10, retryDelay: 100 });
  }
});

test("same-account native locks serialize workers and release on cancellation", { skip: process.platform !== "win32", timeout: 20_000 }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "x-runtime-lock-"));
  const entry = path.join(directory, "plugin.exe");
  const input = { ...request, operation: "upload", target: { id: "x", driverId: "x" }, workspace: { dataDir: path.join(directory, "account-root") } };
  let first, second;
  try {
    await copyFile(executable, entry);
    await writeFile(path.join(directory, "runner.mjs"), `
      import {appendFile} from 'node:fs/promises';
      await appendFile('started.txt',process.pid+'\\n');
      setInterval(()=>{},1000);
    `);
    const started = async () => { try { return (await readFile(path.join(directory, "started.txt"), "utf8")).trim().split("\n"); } catch { return []; } };
    first = start(entry, input);
    await until(async () => (await started()).length === 1);
    second = start(entry, input);
    await sleep(250);
    assert.equal((await started()).length, 1);
    first.child.kill(); await first.ended;
    await until(async () => (await started()).length === 2);
    second.child.kill(); await second.ended;
  } finally {
    first?.child.kill(); second?.child.kill();
    await rm(directory, { recursive: true, maxRetries: 10, retryDelay: 100 });
  }
});
