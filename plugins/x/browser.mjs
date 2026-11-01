import { restoreSession } from "./session.mjs";
import { setTimeout as sleep } from "node:timers/promises";
import { PluginError, numericId } from "./storage.mjs";
import { textFor } from "./validation.mjs";

export async function openBrowser(profile, headless) {
  // The Chromium binary stays in the upstream per-user cache, outside the plugin.
  process.env.CLOAKBROWSER_AUTO_UPDATE ??= "false";
  let launchPersistentContext;
  try { ({ launchPersistentContext } = await import("cloakbrowser")); }
  catch { throw new PluginError("x_runtime", "缺少 X 浏览器依赖，请重新运行 npm run plugin:build:x"); }
  try {
    const context = await launchPersistentContext({ userDataDir: profile, headless, locale: "en-US" });
    try { await restoreSession(context, profile); } catch(error) { await context.close().catch(()=>{}); throw error; }
    context.setDefaultTimeout(30_000);
    context.setDefaultNavigationTimeout(60_000);
    return context;
  } catch {
    throw new PluginError("x_browser", "无法启动 CloakBrowser；请检查网络、浏览器组件和账号窗口是否已关闭。可在插件目录运行 npm run install:browser");
  }
}

export async function identity(page, context, timeout = 10_000) {
  const cookies = await context.cookies("https://x.com");
  const twid = cookies.find((cookie) => cookie.name === "twid")?.value ?? "";
  let decoded;
  try { decoded = decodeURIComponent(twid).replaceAll('"', ""); } catch { decoded = ""; }
  const userId = /^u=([0-9]{1,25})$/.exec(decoded)?.[1];
  if (!userId || !cookies.some((cookie) => cookie.name === "auth_token" && cookie.value)) throw new PluginError("x_login", "X 登录已失效，请重新运行该账号的配置命令完成登录");
  let href;
  try { href = await page.locator('a[data-testid="AppTabBar_Profile_Link"]').first().getAttribute("href", { timeout }); }
  catch { throw new PluginError("x_login", "X 未显示已登录账号；请运行配置命令，在浏览器完成登录或账号验证"); }
  const username = /^\/([A-Za-z0-9_]{1,15})\/?$/.exec(href ?? "")?.[1];
  if (!username) throw new PluginError("x_layout", "X 账号菜单结构已变化，请更新插件");
  return { userId, username };
}

export async function verifyIdentity(page, context, config) {
  const actual = await identity(page, context);
  if (actual.userId !== config.userId || actual.username.toLowerCase() !== config.username.toLowerCase()) throw new PluginError("x_account", "当前 X 登录账号与配置不一致，已停止发布；请核对账号并重新配置");
}

function variablesFor(request) {
  try {
    const body = request.postDataJSON();
    return typeof body?.variables === "string" ? JSON.parse(body.variables) : body?.variables;
  } catch { return null; }
}
function creationRequest(request) {
  try {
    const url = new URL(request.url());
    return request.method() === "POST" && ["x.com", "api.x.com"].includes(url.hostname)
      && /^\/i\/api\/graphql\/[^/]+\/CreateTweet$/.test(url.pathname);
  } catch { return false; }
}
function matchesPublication(variables, text, count) {
  return variables?.tweet_text === text && Array.isArray(variables.media?.media_entities)
    && variables.media.media_entities.length === count;
}

export function receiptFrom(body, config, count) {
  if (Array.isArray(body?.errors) && body.errors.length) throw new PluginError("x_rejected", "X 拒绝发布，可能是登录验证、频率限制、重复内容或媒体不符合要求；请到网页查看");
  const result = body?.data?.create_tweet?.tweet_results?.result;
  const tweet = result?.tweet ?? result;
  const id = tweet?.rest_id;
  const userId = tweet?.legacy?.user_id_str ?? tweet?.core?.user_results?.result?.rest_id;
  const media = tweet?.legacy?.extended_entities?.media ?? [];
  if (!numericId(id) || userId !== config.userId || !Array.isArray(media) || media.length !== count) throw new PluginError("x_response", "X 未返回与当前账号及附件数量一致的帖文回执，请先在网页核对发布结果");
  return { location: `https://x.com/${config.username}/status/${id}`, files: [] };
}

export async function composeAndPost(page, request, config, journal, { progress = () => {}, sensitive = false, responseTimeout = 90_000 } = {}) {
  const publication = request.publication;
  const text = textFor(publication);
  const count = publication.assets.length;
  const editor = page.locator('[data-testid="tweetTextarea_0"][contenteditable="true"]').first();
  try { await editor.waitFor({ state: "visible", timeout: 30_000 }); }
  catch { throw new PluginError("x_layout", "X 未显示发布编辑器；请检查登录状态、账号验证或网页改版"); }
  const dialogs = page.getByRole("dialog").filter({ has: page.locator('[data-testid="tweetTextarea_0"]') });
  const form = await dialogs.count() ? dialogs.last() : page.locator("body");
  if (text) {
    await editor.fill("");
    // Soft line breaks avoid the extra empty paragraph Chromium can insert when
    // filling a multiline contenteditable field in a single operation.
    for (const [index, line] of text.split("\n").entries()) {
      if (index) await editor.press("Shift+Enter");
      if (line) await editor.pressSequentially(line);
    }
  }
  if (count) {
    await form.locator('input[data-testid="fileInput"]').first().setInputFiles(publication.assets.map((asset) => asset.path));
    const deadline = Date.now() + 180_000;
    let ready = false;
    while (Date.now() < deadline) {
      const previews = await form.locator('[data-testid="attachments"] [data-testid="removeMedia"], [data-testid="attachments"] [aria-label="Remove media"], [data-testid="attachments"] [aria-label="移除媒体"], [data-testid="attachments"] [aria-label="移除媒體"], [data-testid="attachments"] [aria-label="メディアを削除"]').count();
      if (previews === count) { ready = true; break; }
      if (page.isClosed()) break;
      await sleep(500);
    }
    if (!ready) throw new PluginError("x_media", "X 未完整接受所有附件，已停止发布；请检查图片、GIF、视频格式与处理提示");
  }
  const button = form.locator('[data-testid="tweetButton"], [data-testid="tweetButtonInline"]').filter({ visible: true }).last();
  try {
    await button.waitFor({ state: "visible" });
    const deadline = Date.now() + 180_000;
    while (!await button.isEnabled()) {
      if (Date.now() > deadline) throw new Error("disabled");
      await sleep(500);
    }
  } catch { throw new PluginError("x_media", "X 发布按钮未就绪；请检查文字长度、附件处理状态或账号限制"); }
  await verifyIdentity(page, page.context(), config);

  // Let the website generate tokens, query IDs and upload requests. Only inspect
  // its final mutation and, when requested, set the existing sensitive flag.
  const routePattern = "**/i/api/graphql/**/CreateTweet";
  let blocked = false;
  let forwarded = false;
  const routeHandler = async (route) => {
    const outgoing = route.request();
    const variables = variablesFor(outgoing);
    if (forwarded || !creationRequest(outgoing) || !matchesPublication(variables, text, count)
        || (sensitive && typeof variables.possibly_sensitive !== "boolean")) {
      blocked = true;
      await route.abort();
      return;
    }
    forwarded = true;
    if (sensitive) {
      const body = outgoing.postDataJSON();
      variables.possibly_sensitive = true;
      body.variables = typeof body.variables === "string" ? JSON.stringify(variables) : variables;
      await route.fallback({ postData: JSON.stringify(body) });
    } else await route.fallback();
  };
  await page.route(routePattern, routeHandler);
  try {
    await journal.submitting();
    // Install the listener before clicking, and attach a rejection handler at once.
    const responsePromise = page.waitForResponse((response) => creationRequest(response.request())
      && matchesPublication(variablesFor(response.request()), text, count), { timeout: responseTimeout });
    responsePromise.catch(() => {});
    await button.click({ timeout: 30_000 });
    const response = await responsePromise;
    if ([400, 401, 403, 422, 429].includes(response.status())) {
      await journal.rejected();
      throw new PluginError("x_rejected", `X 拒绝发布（HTTP ${response.status()}）；请在网页检查账号状态、频率限制及媒体要求后重试`);
    }
    if (!response.ok()) throw new PluginError("x_uncertain", "X 发布请求异常，结果待确认；请先查看个人主页");
    const body = await response.json();
    if (Array.isArray(body?.errors) && body.errors.length && !body?.data?.create_tweet?.tweet_results?.result) {
      await journal.rejected();
    }
    const receipt = receiptFrom(body, config, count);
    await journal.complete(receipt);
    await progress(publication.assets.reduce((total, asset) => total + asset.size, 0));
    return receipt;
  } catch (error) {
    if (blocked) {
      if (!forwarded) await journal.rejected();
      throw new PluginError("x_layout", "X 最终发布请求与文字、附件或敏感标记不一致，或网页尝试重复提交，已阻止请求；请检查发布状态或更新插件");
    }
    if (error instanceof PluginError) throw error;
    throw new PluginError("x_uncertain", "点击发布后未取得确认回执；请先在 X 个人主页检查，避免重复发布");
  } finally { await page.unroute(routePattern, routeHandler).catch(() => {}); }
}
