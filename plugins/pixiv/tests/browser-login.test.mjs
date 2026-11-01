import assert from "node:assert/strict";
import { test } from "node:test";
import { acquireLogin, openLoginContext, loggedInUser, sessionFor } from "../browser-login.mjs";

const cookie = (value = "123_local_test", domain = ".pixiv.net") => ({ name: "PHPSESSID", value, domain, path: "/" });
const bootstrap = (userId = "123", isLoggedIn = true) => `<script id="__NEXT_DATA__" type="application/json">${JSON.stringify({ props: { pageProps: { isLoggedIn, gaUserData: { userId }, serverSerializedPreloadedState: "{}" } } })}</script>`;

test("only a unique Pixiv session from the correct cookie domain is accepted", () => {
  assert.equal(sessionFor([cookie()]), "123_local_test");
  assert.equal(sessionFor([cookie("123_local_test", "evil.example")]), null);
  assert.equal(sessionFor([{ ...cookie(), name: "other" }]), null);
  assert.equal(sessionFor([cookie("bad\nvalue")]), null);
  assert.equal(sessionFor([cookie(), cookie()]), "123_local_test");
  assert.throws(() => sessionFor([cookie(), cookie("456_other_session")]), /多个不同/);
});

test("browser bootstrap is read only on the genuine HTTPS Pixiv origin", async () => {
  const fake = { url: () => "https://www.pixiv.net.evil.example/", evaluate: () => { throw new Error("must not evaluate"); } };
  assert.equal(await loggedInUser(fake), null);
});

async function fixture({ userId = "123", cookieUser = "123", loggedIn = true, expected = "", missingCookie = false, closeEarly = false } = {}) {
  let closed = false;
  const result = await acquireLogin({
    expectedUserId: expected, timeoutMs: missingCookie || !loggedIn ? 300 : 3_000, pollMs: 20,
    launch: async () => {
      const context = await openLoginContext({ headless: true });
      await context.route("**/*", async (route) => {
        const url = new URL(route.request().url());
        // A separate navigation remains intercepted; a fulfilled HTTP redirect
        // can make Chromium follow the redirect chain outside Playwright routing.
        if (url.hostname === "accounts.pixiv.net") return route.fulfill({ contentType: "text/html", body: '<!doctype html><script>location.replace("https://www.pixiv.net/")</script>' });
        if (url.origin === "https://www.pixiv.net") return route.fulfill({ contentType: "text/html", body: `<!doctype html>${bootstrap(userId, loggedIn)}` });
        return route.abort();
      });
      if (!missingCookie) await context.addCookies([{ ...cookie(`${cookieUser}_local_test`), httpOnly: true, secure: true }]);
      return {
        newPage: () => context.newPage(),
        pages: () => closeEarly ? [] : context.pages(),
        cookies: (...args) => context.cookies(...args),
        close: async () => { closed = true; await context.close(); },
      };
    },
  }).then((login) => ({ login }), (error) => ({ error }));
  assert.equal(closed, true, "browser must close on success and failure");
  return result;
}

test("real headless browser extracts the logged-in account and HttpOnly cookie", async () => {
  const result = await fixture();
  assert.deepEqual(result.login, { userId: "123", session: "123_local_test" });
});
test("a logged-out page with a leftover cookie never counts as successful login", async () => {
  assert.match((await fixture({ loggedIn: false })).error.message, /超过 10 分钟/);
});
test("browser identity and the session prefix must match", async () => {
  assert.match((await fixture({ cookieUser: "456" })).error.message, /Cookie 不一致/);
});
test("an existing upload target cannot silently switch to another Pixiv account", async () => {
  assert.match((await fixture({ expected: "456" })).error.message, /不一致/);
});
test("closing the login window or not obtaining a cookie does not return credentials", async () => {
  assert.match((await fixture({ closeEarly: true })).error.message, /窗口已关闭/);
  assert.match((await fixture({ missingCookie: true })).error.message, /超过 10 分钟/);
});
