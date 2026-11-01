import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, readFile, writeFile, mkdir, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { validate, textFor } from "../validation.mjs";
import { atomicJson, accountDirectory, prepareJournal } from "../storage.mjs";
import { receiptFrom } from "../browser.mjs";
import { upload } from "../runner.mjs";

const publication = (body = "hello") => ({ title: "", body, assets: [] });
const config = { username: "test_user", userId: "123", headless: true, sensitive: false };
const request = (directory, operationId = "operation-1") => ({
  publication: publication(), jobId: "job-1", operationId,
  outputDir: directory, target: { id: "x", driverId: "x" },
  workspace: { dataDir: directory, contentRating: "all_ages" },
});
async function temporary(fn) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "upload-hub-x-"));
  try { await fn(directory); } finally { await rm(directory, { recursive: true }); }
}

test("normal posts use X weighting for Chinese, emoji and shortened URLs", () => {
  assert.deepEqual(validate(publication("a".repeat(280))), []);
  assert.ok(validate(publication("a".repeat(281))).length);
  assert.deepEqual(validate(publication("中".repeat(140))), []);
  assert.ok(validate(publication("中".repeat(141))).length);
  assert.deepEqual(validate(publication("😀".repeat(140))), []);
  assert.deepEqual(validate(publication(`see https://example.com/${"a".repeat(500)}`)), []);
  assert.ok(validate(publication("")).length);
  assert.equal(textFor({ title: " title ", body: " body " }), "title\n\nbody");
});

test("media validation checks signatures, counts, actual size, kinds and mixed GIFs", async () => temporary(async (directory) => {
  const image = path.join(directory, "photo.png");
  await writeFile(image, Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==", "base64"));
  const bytes = (await readFile(image)).length;
  const asset = { name: "photo.png", path: image, size: bytes, kind: "image" };
  const post = { ...publication(""), assets: [asset] };
  assert.deepEqual(validate(post), []);
  assert.ok(validate({ ...post, assets: [{ ...asset, size: bytes + 1 }] }).length);
  assert.ok(validate({ ...post, assets: [{ ...asset, kind: "file" }] }).length);
  assert.ok(validate({ ...post, assets: Array(5).fill(asset) }).length);
  const gif = path.join(directory, "animation.gif");
  await writeFile(gif, Buffer.from("GIF89a"));
  assert.ok(validate({ ...post, assets: [asset, { name: "animation.gif", path: gif, size: 6, kind: "image" }] }).some((issue) => issue.includes("单独发布")));
  await writeFile(image, "not an image");
  assert.ok(validate(post).length);
}));

test("accounts require an absolute workspace path and safe target/operation IDs", () => {
  const root = path.join(os.tmpdir(), "workspace");
  assert.equal(accountDirectory(request(root)), path.join(root, "x"));
  assert.throws(() => accountDirectory({ ...request(root), workspace: null }));
  assert.throws(() => accountDirectory({ ...request(root), target: { id: "../other", driverId: "x" } }));
  assert.throws(() => accountDirectory({ ...request(root), operationId: "../other" }));
});

test("confirmed receipts are reused; pending submissions block same-operation and new posts", async () => temporary(async (directory) => {
  const first = request(directory);
  const journal = await prepareJournal(first, config, directory, false);
  await journal.submitting();
  await assert.rejects(prepareJournal(first, config, directory, false), { code: "x_uncertain" });
  await assert.rejects(prepareJournal(request(directory, "operation-2"), config, directory, false), { code: "x_uncertain" });
  const receipt = { location: "https://x.com/test_user/status/456", files: [] };
  await journal.complete(receipt);
  const retried = await prepareJournal(first, config, directory, false);
  assert.deepEqual(retried.value.receipt, receipt);
  await assert.rejects(prepareJournal({ ...first, publication: publication("changed") }, config, directory, false), { code: "x_uncertain" });
  await assert.rejects(prepareJournal(first, config, directory, true), { code: "x_uncertain" });
  await assert.rejects(prepareJournal(first, { ...config, userId: "999" }, directory, false), { code: "x_uncertain" });
}));

test("rejected submissions can retry; corrupt journal files fail closed", async () => temporary(async (directory) => {
  const first = request(directory);
  const journal = await prepareJournal(first, config, directory, false);
  await journal.submitting(); await journal.rejected();
  assert.equal((await prepareJournal(first, config, directory, false)).value.stage, "prepared");
  await writeFile(journal.file, "{broken");
  await assert.rejects(prepareJournal(first, config, directory, false), { code: "x_uncertain" });
  await assert.rejects(prepareJournal(request(directory, "operation-2"), config, directory, false), { code: "x_uncertain" });
}));

test("success requires a numeric post ID, matching author and all media", () => {
  const result = { rest_id: "456", legacy: { user_id_str: "123", extended_entities: { media: [{}, {}] } } };
  const envelope = (value) => ({ data: { create_tweet: { tweet_results: { result: value } } } });
  assert.equal(receiptFrom(envelope(result), config, 2).location, "https://x.com/test_user/status/456");
  assert.throws(() => receiptFrom(envelope(result), config, 1));
  assert.throws(() => receiptFrom(envelope(result), { ...config, userId: "999" }, 2));
  assert.throws(() => receiptFrom({ data: {} }, config, 0));
  assert.throws(() => receiptFrom({ errors: [{ message: "refused" }] }, config, 0));
});

test("completed retries return before launching a browser, even if login has expired", async () => temporary(async (directory) => {
  const first = request(directory);
  const account = path.join(directory, "x");
  await atomicJson(path.join(account, "config.json"), config);
  const journal = await prepareJournal(first, config, account, false);
  const receipt = { location: "https://x.com/test_user/status/456", files: [] };
  await journal.complete(receipt);
  assert.deepEqual(await upload(first, { launch: () => { throw new Error("must not launch"); } }), receipt);
}));

test("missing account config never launches the browser", async () => temporary(async (directory) => {
  await mkdir(path.join(directory, "x"));
  await assert.rejects(upload(request(directory), { launch: () => { throw new Error("must not launch"); } }), { code: "x_config" });
}));
