import assert from "node:assert/strict";
import { test, before, after } from "node:test";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { openBrowser, composeAndPost, verifyIdentity } from "../browser.mjs";
import { atomicJson, prepareJournal } from "../storage.mjs";
import { upload } from "../runner.mjs";

let directory, context;
const config = { username: "test_user", userId: "123", headless: true, sensitive: false };
const requests = [];
const mode = { localized: false, status: 200, invalid: false, mediaCount: null, dropOutgoing: false };
const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==", "base64");

function fixture() {
  return `<!doctype html><html lang="en"><body>
    <a data-testid="AppTabBar_Profile_Link" href="/test_user">Profile</a>
    <section role="dialog">
      <div contenteditable="true" style="white-space:pre-wrap" role="textbox" data-testid="tweetTextarea_0"></div>
      <input type="file" multiple data-testid="fileInput"><div data-testid="attachments"></div>
      <button data-testid="tweetButton">Post</button>
    </section>
    <script>
      const input = document.querySelector('input');
      input.onchange = () => {
        document.querySelector('[data-testid="attachments"]').innerHTML = Array.from(input.files).map(() => '${mode.localized ? '<button aria-label="移除媒体">移除</button>' : '<button data-testid="removeMedia">Remove</button>'}').join('');
      };
      document.querySelector('[data-testid="tweetButton"]').onclick = async () => {
        const variables = {
          tweet_text: document.querySelector('[contenteditable]').innerText,
          possibly_sensitive: false,
          media: {media_entities: ${mode.dropOutgoing ? "[]" : "Array.from(input.files).map((_, i) => ({media_id:String(i + 1)}))"}}
        };
        await fetch('/i/api/graphql/fixture/CreateTweet', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({variables})});
      };
    </script></body></html>`;
}
before(async () => {
  directory = await mkdtemp(path.join(os.tmpdir(), "upload-hub-x-browser-"));
  context = await openBrowser(path.join(directory, "profile"), true);
  await context.addCookies([
    { name: "auth_token", value: "fake-local-test-only", domain: ".x.com", path: "/", secure: true },
    { name: "twid", value: "u%3D123", domain: ".x.com", path: "/", secure: true },
  ]);
  // Every page request is intercepted. No real X traffic or account is used.
  await context.route("**/*", async (route) => {
    if (route.request().url() === "https://x.com/compose/post") return route.fulfill({ contentType: "text/html; charset=utf-8", body: fixture() });
    if (route.request().url() === "https://x.com/i/api/graphql/fixture/CreateTweet") {
      const body = route.request().postDataJSON();
      requests.push(body);
      const count = mode.mediaCount ?? body.variables.media.media_entities.length;
      const payload = mode.invalid ? { data: {} } : { data: { create_tweet: { tweet_results: { result: {
        rest_id: "456", legacy: { user_id_str: "123", extended_entities: { media: Array.from({ length: count }, () => ({})) } },
      } } } } };
      return route.fulfill({ status: mode.status, contentType: "application/json", body: JSON.stringify(payload) });
    }
    return route.abort();
  });
});
after(async () => {
  await context?.close();
  if (directory) await rm(directory, { recursive: true });
});
async function posting(name, assets, options = {}) {
  const account = path.join(directory, name);
  const request = {
    publication: { title: "Title", body: "正文", assets }, jobId: "job-1", operationId: "operation-1",
    workspace: { dataDir: directory, contentRating: "all_ages" }, target: { id: name, driverId: "x" }, outputDir: directory,
  };
  const journal = await prepareJournal(request, config, account, false);
  const page = await context.newPage();
  try {
    await page.goto("https://x.com/compose/post");
    await verifyIdentity(page, context, config);
    return await composeAndPost(page, request, config, journal, { responseTimeout: 5_000, ...options });
  } finally { await page.close(); }
}

test("real headless CloakBrowser fills the editor and confirms a text post", async () => {
  mode.status = 200; mode.invalid = false; mode.mediaCount = null;
  assert.equal((await posting("text", [])).location, "https://x.com/test_user/status/456");
  assert.equal(requests.at(-1).variables.tweet_text, "Title\n\n正文");
});

test("four image attachments are sent together and the sensitive flag reaches the mutation", async () => {
  const assets = [];
  for (let i = 0; i < 4; i++) {
    const file = path.join(directory, `photo${i}.png`);
    await writeFile(file, png);
    assets.push({ name: `photo${i}.png`, path: file, size: png.length, kind: "image" });
  }
  let completed = 0;
  await posting("photos", assets, { sensitive: true, progress: (bytes) => { completed = bytes; } });
  assert.equal(requests.at(-1).variables.media.media_entities.length, 4);
  assert.equal(requests.at(-1).variables.possibly_sensitive, true);
  assert.equal(completed, png.length * 4);
});

test("a single GIF and a single MP4 both use the actual browser file chooser", async () => {
  for (const [extension, kind, bytes] of [["gif", "image", Buffer.from("GIF89a")], ["mp4", "video", Buffer.from([0,0,0,24,102,116,121,112,105,115,111,109])]]) {
    const file = path.join(directory, `media.${extension}`);
    await writeFile(file, bytes);
    await posting(extension, [{ name: `media.${extension}`, path: file, size: bytes.length, kind }]);
    assert.equal(requests.at(-1).variables.media.media_entities.length, 1);
  }
});

test("HTTP rejection permits a retry and an unconfirmed response blocks duplicates", async () => {
  mode.status = 429;
  await assert.rejects(posting("limited", []), { code: "x_rejected" });
  const record = JSON.parse(await (await import("node:fs/promises")).readFile(path.join(directory, "limited/submissions/operation-1.json"), "utf8"));
  assert.equal(record.stage, "prepared");
  mode.status = 200; mode.invalid = true;
  await assert.rejects(posting("uncertain", []), { code: "x_response" });
  await assert.rejects(posting("uncertain", []), { code: "x_uncertain" });
  mode.invalid = false;
});

test("account mismatch stops before any post request", async () => {
  const page = await context.newPage();
  try {
    await page.goto("https://x.com/compose/post");
    await assert.rejects(verifyIdentity(page, context, { ...config, userId: "999" }), { code: "x_account" });
  } finally { await page.close(); }
});

test("an outgoing mutation that drops an attachment is blocked before it reaches X", async () => {
  mode.dropOutgoing = true;
  const count = requests.length;
  const file = path.join(directory, "guard.png");
  await writeFile(file, png);
  try {
    await assert.rejects(posting("guard", [{ name: "guard.png", path: file, size: png.length, kind: "image" }], { responseTimeout: 500 }), { code: "x_layout" });
    assert.equal(requests.length, count);
  } finally { mode.dropOutgoing = false; }
});

test("upload orchestration forces sensitive media for an R18 workspace", async () => {
  mode.status = 200; mode.invalid = false; mode.mediaCount = null;
  const target = "rating";
  await atomicJson(path.join(directory, target, "config.json"), config);
  const file = path.join(directory, "rating.png");
  await writeFile(file, png);
  let requestedHeadless;
  const receipt = await upload({
    publication: { title: "", body: "Rating test", assets: [{ name: "rating.png", path: file, size: png.length, kind: "image", sha256: "test" }] },
    jobId: "job-1", operationId: "rating-operation", outputDir: directory,
    target: { id: target, driverId: "x" }, workspace: { dataDir: directory, contentRating: "r18" },
  }, { launch: async (_profile, headless) => {
    requestedHeadless = headless;
    // Exercise orchestration without closing the shared test context.
    return { pages: () => [], newPage: () => context.newPage(), cookies: (...args) => context.cookies(...args), close: async () => {} };
  }});
  assert.equal(requestedHeadless, true);
  assert.equal(receipt.location, "https://x.com/test_user/status/456");
  assert.equal(requests.at(-1).variables.possibly_sensitive, true);
});

test("Chinese media removal buttons without a test ID count as accepted attachments", async () => {
  mode.status=200;mode.invalid=false;mode.mediaCount=null;mode.dropOutgoing=false;mode.localized=true;
  try {
    const file=path.join(directory,"localized.png");await writeFile(file,png);
    assert.equal((await posting("localized",[{name:"localized.png",path:file,size:png.length,kind:"image"}])).location,"https://x.com/test_user/status/456");
  } finally { mode.localized=false; }
});
