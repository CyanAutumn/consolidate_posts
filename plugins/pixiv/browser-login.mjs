import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const USER_ID = /^[0-9]{1,25}$/;
export class LoginError extends Error {}

export function sessionFor(cookies) {
  const matches = cookies.filter((cookie) => cookie.name === "PHPSESSID"
    && ["pixiv.net", ".pixiv.net", "www.pixiv.net"].includes(cookie.domain)
    && cookie.path === "/" && typeof cookie.value === "string"
    && /^[0-9]{1,25}_[A-Za-z0-9_-]+$/.test(cookie.value)
    && cookie.value.length <= 4096);
  const values = [...new Set(matches.map((cookie) => cookie.value))];
  if (values.length > 1) throw new LoginError("检测到多个不同的 Pixiv 登录凭据，请重新打开登录窗口后只登录一个账号");
  return values[0] ?? null;
}

export async function loggedInUser(page) {
  let url;
  try { url = new URL(page.url()); } catch { return null; }
  if (url.protocol !== "https:" || url.hostname !== "www.pixiv.net") return null;
  // Inspect first-party bootstrap state only. Do not infer login from an avatar
  // or someone else's /users/ link, which could identify the wrong account.
  const user = await page.evaluate(() => {
    try {
      const script = document.getElementById("__NEXT_DATA__");
      if (script) {
        const props = JSON.parse(script.textContent).props?.pageProps;
        if (props?.isLoggedIn === false) return null;
        if (props?.isLoggedIn === true) {
          const raw = props.serverSerializedPreloadedState;
          const state = typeof raw === "string" ? JSON.parse(raw) : raw;
          return props.gaUserData?.userId ?? state?.userData?.self?.id ?? null;
        }
      }
      const meta = document.getElementById("meta-global-data");
      const data = meta ? JSON.parse(meta.getAttribute("content")) : null;
      return data?.userData?.id ?? null;
    } catch { return null; }
  });
  const value = typeof user === "number" && Number.isSafeInteger(user) ? String(user) : user;
  return typeof value === "string" && USER_ID.test(value) ? value : null;
}

export async function openLoginContext({ headless = false } = {}) {
  process.env.CLOAKBROWSER_AUTO_UPDATE ??= "false";
  let launchContext;
  try { ({ launchContext } = await import("cloakbrowser")); }
  catch { throw new LoginError("缺少 Pixiv 浏览器依赖，请先运行 npm run plugin:build:pixiv"); }
  try {
    // An ephemeral context keeps the browser separate from every other account.
    // Only the DPAPI credential written by the native parent is retained.
    const context = await launchContext({ headless, locale: "zh-CN" });
    context.setDefaultNavigationTimeout(60_000);
    return context;
  } catch {
    throw new LoginError("无法启动 CloakBrowser，请检查网络、浏览器组件及 Node.js 安装");
  }
}

export async function acquireLogin({
  expectedUserId = "", launch = openLoginContext,
  timeoutMs = 600_000, pollMs = 500,
} = {}) {
  if (expectedUserId && !USER_ID.test(expectedUserId)) throw new LoginError("Pixiv 用户 ID 无效");
  const context = await launch();
  try {
    const page = await context.newPage();
    await page.goto("https://accounts.pixiv.net/login?return_to=https%3A%2F%2Fwww.pixiv.net%2F", { waitUntil: "domcontentloaded" });
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const pages = context.pages().filter((candidate) => !candidate.isClosed());
      if (!pages.length) throw new LoginError("登录窗口已关闭，未保存新凭据");
      const session = sessionFor(await context.cookies("https://www.pixiv.net/"));
      if (session) {
        for (const candidate of pages) {
          let userId;
          try { userId = await loggedInUser(candidate); } catch { continue; }
          if (!userId) continue;
          if (session.split("_", 1)[0] !== userId) throw new LoginError("浏览器账号与 Pixiv Cookie 不一致，未保存凭据");
          if (expectedUserId && userId !== expectedUserId) throw new LoginError(`登录的是 Pixiv 用户 ${userId}，与目标账号 ${expectedUserId} 不一致；请为另一个账号新建上传目标`);
          // Re-read the cookie after inspecting the page to avoid a stale sample
          // during an account switch or session renewal.
          const current = sessionFor(await context.cookies("https://www.pixiv.net/"));
          if (current === session) return { userId, session };
        }
      }
      await sleep(pollMs);
    }
    throw new LoginError("等待 Pixiv 登录超过 10 分钟，未保存新凭据；请重新运行配置命令");
  } finally { await context.close().catch(() => {}); }
}

export async function main(args) {
  // stdout is a private pipe to the native parent, never a console or log.
  console.log = console.info = (...values) => console.error(...values);
  try {
    const { values } = parseArgs({ args, options: { "expected-user-id": { type: "string", default: "" }, "native-pipe": { type: "boolean", default: false } } });
    if (!values["native-pipe"] || process.stdout.isTTY) throw new LoginError("请通过 configure.ps1 启动浏览器登录，不要直接运行内部登录脚本");
    const result = await acquireLogin({ expectedUserId: values["expected-user-id"] });
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    console.error(error instanceof LoginError ? error.message : "Pixiv 浏览器登录未完成，请检查网络或重新打开窗口登录");
    process.exitCode = 1;
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main(process.argv.slice(2));
