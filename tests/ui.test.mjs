import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { Window } from "happy-dom";
import { createServer } from "vite";
import { readdir, readFile } from "node:fs/promises";
const pluginRoot = new URL("../plugins/", import.meta.url);
const catalog = (
  await Promise.all(
    (await readdir(pluginRoot, { withFileTypes: true }))
      .filter((entry) => entry.isDirectory())
      .map(async (entry) =>
        JSON.parse(
          await readFile(
            new URL(`${entry.name}/plugin.json`, pluginRoot),
            "utf8",
          ),
        ),
      ),
  )
)
  .filter((manifest) => manifest.enabled)
  .sort(
    (a, b) =>
      Number(b.id === "local-archive") - Number(a.id === "local-archive"),
  );

const window = new Window({ url: "http://localhost:1430" });
for (const key of [
  "window",
  "document",
  "navigator",
  "HTMLElement",
  "HTMLInputElement",
  "HTMLTextAreaElement",
  "HTMLSelectElement",
  "Event",
  "MouseEvent",
]) {
  Object.defineProperty(globalThis, key, {
    configurable: true,
    value: key === "window" ? window : window[key],
  });
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const React = await import("react");
const { createRoot } = await import("react-dom/client");
let server, App;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true },
    appType: "custom",
  });
  App = (await server.ssrLoadModule("/src/App.tsx")).default;
});
after(async () => {
  await server?.close();
  await window.happyDOM.close();
});

const blank = () => ({
  workspace: {
    id: "default",
    name: "默认工作区",
    contentRating: "unspecified",
    enabledDriverIds: catalog.map((driver) => driver.id),
    selectedTargetIds: ["local-archive"],
    createdAt: 0,
  },
  workspaces: [
    {
      id: "default",
      name: "默认工作区",
      contentRating: "unspecified",
      enabledDriverIds: catalog.map((driver) => driver.id),
      selectedTargetIds: ["local-archive"],
      createdAt: 0,
    },
  ],
  pluginDir: "test-plugins",
  pluginReports: [],
  drivers: catalog,
  draft: { title: "", body: "", assetIds: [] },
  assets: [],
  jobs: [],
  history: {
    archives: [],
    plans: [],
    targets: catalog.map((driver) => ({
      id: driver.id,
      driverId: driver.id,
      name:
        driver.id === "local-archive" ? "本机默认目录" : "默认账号（待连接）",
      createdAt: 0,
    })),
  },
  dataDir: "test-data",
  archiveDir: "test-data/archives",
  concurrency: 2,
});

test("plugin folder and failed-entry diagnostics are visible", async () => {
  const { client, state } = makeClient();
  state.pluginReports = [
    {
      folder: "test-plugins/missing",
      id: "missing",
      name: "未编译插件",
      version: "0.1.0",
      state: "error",
      message: "入口文件不存在：plugin.exe；请先编译插件",
    },
  ];
  const view = await mount(client);
  try {
    await click(button(view.element, "平台登录"));
    assert.match(view.element.textContent, /test-plugins/);
    assert.match(
      view.element.querySelector(".plugin-error").textContent,
      /入口文件不存在/,
    );
    assert.match(
      view.element.textContent,
      /新增、更新或启用插件后，重启应用加载/,
    );
  } finally {
    await view.dispose();
  }
});

test("preview tolerates malformed manifests and rejects duplicate plugin IDs", async () => {
  const { parsePluginPreviews } = await server.ssrLoadModule(
    "/src/lib/catalog.ts",
  );
  const valid = JSON.stringify(
    catalog.find((driver) => driver.id === "local-archive"),
  );
  const result = parsePluginPreviews({
    "plugins/a/plugin.json": valid,
    "plugins/b/plugin.json": valid,
    "plugins/broken/plugin.json": "{bad json",
    "plugins/invalid/plugin.json": '{"schemaVersion":1,"kinds":null}',
  });
  assert.equal(result.catalog.length, 1);
  assert.equal(result.catalog[0].status, "unavailable");
  assert.equal(result.reports.length, 4);
  assert.ok(result.reports.every((report) => report.state === "error"));
});
function makeClient(desktop = true) {
  const state = blank();
  const calls = [];
  const client = {
    desktop,
    platformAccounts: async () => state.platformAccounts ?? [],
    savePixivSettings: async(id,settings) => {calls.push(["pixiv-save",id,settings]);return settings;},
    loginXiaohongshu: async (id) => { calls.push(["xiaohongshu-login", id]); },
    loginX: async (id) => { calls.push(["x-login", id]); },
    pixivSettings: async (id) => {
      calls.push(["pixiv-settings", id]);
      return { userId: "123", tags: ["默认"], aiType: "notAiGenerated", visibility: "private", rating: null, original: false, allowTagEdit: false, allowComment: false };
    },
    loginPixiv: async (id, settings) => { calls.push(["pixiv-login", id, structuredClone(settings)]); },
    forWorkspace: () => client,
    selectTargets: async (ids) => {
      state.workspace.selectedTargetIds = [...ids];
      calls.push(["selection", ids]);
    },
    createWorkspace: async () => {
      throw new Error("not implemented in fixture");
    },
    switchWorkspace: async () => {
      throw new Error("not implemented in fixture");
    },
    updateWorkspace: async (config) => {
      Object.assign(state.workspace, config);
      state.workspaces = [state.workspace];
      calls.push(["workspace", config]);
      return structuredClone(state);
    },
    bootstrap: async () => structuredClone(state),
    jobs: async () => structuredClone(state.jobs),
    history: async () => structuredClone(state.history),
    createTarget: async (driverId, name) => {
      const target = {
        id: "new-account",
        driverId,
        name,
        createdAt: Date.now(),
      };
      calls.push(["target", driverId, name]);
      state.history.targets.push(target);
      return target;
    },
    previewReplay: async (request) => {
      calls.push(["preview", structuredClone(request)]);
      return {
        eligibleCount: request.archiveIds.length,
        skippedCount: 0,
        items: request.archiveIds.map((id) => ({
          archiveId: id,
          title: id,
          eligible: true,
          reason: null,
          scheduledAt: request.startAt,
        })),
      };
    },
    scheduleReplay: async (request) => {
      calls.push(["replay", structuredClone(request)]);
      return {
        plan: {},
        preview: { eligibleCount: request.archiveIds.length, skippedCount: 0 },
      };
    },
    setPlanStatus: async (id, status) => {
      calls.push(["plan", id, status]);
      state.history.plans.find((plan) => plan.id === id).status = status;
    },
    saveDraft: async (draft) => {
      calls.push(["save", structuredClone(draft)]);
      state.draft = structuredClone(draft);
    },
    chooseFiles: async () => ["test.png"],
    importPaths: async (paths) => {
      calls.push(["import", paths]);
      return {
        assets: [
          {
            id: "image-1",
            name: "test.png",
            path: "test.png",
            kind: "image",
            size: 30,
            modifiedMs: 1,
          },
        ],
        errors: [],
      };
    },
    preflight: async () => [
      { driverId: "local-archive", eligible: true, issues: [] },
    ],
    enqueue: async (draft, driverIds) => {
      calls.push(["enqueue", structuredClone(draft), driverIds]);
      state.jobs = [
        {
          id: "job-1",
          kind: "upload",
          targetId: "local-archive",
          archiveId: "archive-1",
          batchId: "batch-1",
          driverId: "local-archive",
          status: "succeeded",
          completedBytes: 30,
          totalBytes: 30,
          createdAt: Date.now(),
          attempt: 1,
          retryOf: null,
          error: null,
          receipt: { location: "test-data/archives/job-1", files: [] },
          publication: { title: draft.title, body: draft.body, assets: [] },
        },
      ];
      return state.jobs;
    },
    cancel: async () => {},
    retry: async () => [],
    onJobsChanged: async () => () => {},
    onDrop: async () => () => {},
  };
  return { client, calls, state };
}
async function mount(client) {
  const element = document.createElement("div");
  document.body.append(element);
  const root = createRoot(element);
  await React.act(async () => {
    root.render(React.createElement(App, { client }));
  });
  return {
    element,
    dispose: async () => {
      await React.act(async () => root.unmount());
      element.remove();
    },
  };
}
const button = (element, text) =>
  [...element.querySelectorAll("button")].find((button) =>
    button.textContent.includes(text),
  );
async function click(element) {
  assert.ok(element);
  await React.act(async () => element.click());
}
async function input(element, value) {
  await React.act(async () => {
    const prototype =
      element.tagName === "TEXTAREA"
        ? window.HTMLTextAreaElement.prototype
        : element.tagName === "SELECT"
          ? window.HTMLSelectElement.prototype
          : window.HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(
      element,
      value,
    );
    element.dispatchEvent(
      new window.Event(element.tagName === "SELECT" ? "change" : "input", {
        bubbles: true,
      }),
    );
  });
}

test("browser preview clearly disables real upload, import and save", async () => {
  const { client } = makeClient(false);
  const view = await mount(client);
  try {
    assert.match(view.element.textContent, /当前为界面预览/);
    await input(view.element.querySelector("#title"), "预览标题");
    assert.equal(button(view.element, "一键上传").disabled, true);
    assert.equal(button(view.element, "浏览本地文件").disabled, true);
    assert.equal(button(view.element, "保存草稿").disabled, true);
    for (const name of [
      "哔哩哔哩",
      "小红书",
      "Pixiv",
      "Hugging Face",
      "Civitai",
    ])
      assert.match(view.element.textContent, new RegExp(name));
    await click(button(view.element, "平台登录"));
    assert.match(view.element.textContent, /3 个平台计划接入/);
  } finally {
    await view.dispose();
  }
});

test("native composer deduplicates attachments, excludes planned drivers and submits the edited draft", async () => {
  const { client, calls } = makeClient();
  const view = await mount(client);
  try {
    await input(view.element.querySelector("#title"), "测试发布标题");
    await input(view.element.querySelector("#body"), "正文与模型说明");
    await click(button(view.element, "浏览本地文件"));
    await click(button(view.element, "继续添加文件"));
    assert.equal(view.element.querySelectorAll(".asset-row").length, 1);
    assert.equal(button(view.element, "哔哩哔哩").disabled, true);
    assert.equal(button(view.element, "一键上传").disabled, false);
    await click(button(view.element, "发布检查"));
    assert.match(view.element.textContent, /发布检查通过/);
    await click(button(view.element, "一键上传"));
    const submitted = calls.find((call) => call[0] === "enqueue");
    assert.deepEqual(submitted[1], {
      title: "测试发布标题",
      body: "正文与模型说明",
      assetIds: ["image-1"],
    });
    assert.deepEqual(submitted[2], ["local-archive"]);
    assert.match(view.element.textContent, /测试发布标题/);
    assert.ok(view.element.querySelector(".status-succeeded"));
    await click(button(view.element, "查看详情"));
    assert.match(view.element.textContent, /test-data\/archives\/job-1/);
  } finally {
    await view.dispose();
  }
});

test("backend rejection is visible and does not produce a false success", async () => {
  const { client } = makeClient();
  client.enqueue = async () => {
    throw { code: "source_changed", message: "文件已改变，请重新导入" };
  };
  const view = await mount(client);
  try {
    await input(view.element.querySelector("#body"), "仅正文");
    await click(button(view.element, "一键上传"));
    assert.match(
      view.element.querySelector("[role=alert]").textContent,
      /文件已改变/,
    );
    assert.equal(view.element.querySelectorAll(".job").length, 0);
    assert.equal(button(view.element, "一键上传").disabled, false);
  } finally {
    await view.dispose();
  }
});

test("publishing with no selected account still creates an archive", async () => {
  const { client, calls } = makeClient();
  const view = await mount(client);
  try {
    await input(view.element.querySelector("#title"), "只留档");
    await click(view.element.querySelector(".driver-option.selected"));
    assert.equal(button(view.element, "仅保存归档").disabled, false);
    await click(button(view.element, "仅保存归档"));
    assert.deepEqual(calls.find((call) => call[0] === "enqueue")[2], []);
  } finally {
    await view.dispose();
  }
});

function seedHistory(state) {
  state.history.archives = ["历史一", "历史二"].map((title, index) => ({
    id: `archive-${index}`,
    sourceJobId: `archive-${index}`,
    createdAt: 1_700_000_000_000 + index,
    publication: { title, body: "正文保存在历史", assets: [] },
    receipt: { location: `saved/${index}`, files: [] },
  }));
}

function workspaceClients() {
  const entries = new Map([["default", makeClient()]]);
  let active = "default";
  const sync = () => {
    const profiles = [...entries.values()].map((entry) =>
      structuredClone(entry.state.workspace),
    );
    for (const entry of entries.values()) entry.state.workspaces = profiles;
  };
  const bind = (id, entry) => {
    entry.client.forWorkspace = (key) => entries.get(key).client;
    entry.client.updateWorkspace = async (config) => {
      Object.assign(entry.state.workspace, structuredClone(config));
      entry.state.workspace.selectedTargetIds =
        entry.state.workspace.selectedTargetIds.filter((targetId) =>
          entry.state.history.targets.some(
            (target) =>
              target.id === targetId &&
              config.enabledDriverIds.includes(target.driverId),
          ),
        );
      sync();
      return structuredClone(entry.state);
    };
  };
  bind("default", entries.get("default"));
  const root = {
    ...entries.get("default").client,
    forWorkspace: (id) => entries.get(id).client,
    bootstrap: async () => {
      sync();
      return structuredClone(entries.get(active).state);
    },
    switchWorkspace: async (id) => {
      active = id;
      sync();
      return structuredClone(entries.get(id).state);
    },
    createWorkspace: async (config) => {
      const id = `workspace-${entries.size}`;
      const entry = makeClient();
      entry.state.workspace = {
        id,
        createdAt: 1,
        selectedTargetIds: [],
        ...structuredClone(config),
      };
      entries.set(id, entry);
      bind(id, entry);
      active = id;
      sync();
      return structuredClone(entry.state);
    },
  };
  return { root, entries, sync };
}

test("workspaces save separate drafts and account selections, filter plugins, and isolate visible history", async () => {
  const { root, entries } = workspaceClients();
  const original = entries.get("default");
  seedHistory(original.state);
  const view = await mount(root);
  try {
    await input(view.element.querySelector("#title"), "全年龄未保存草稿");
    await click(view.element.querySelector(".driver-option.selected"));
    await click(view.element.querySelector('[aria-label="管理工作区"]'));
    await click(button(view.element, "新建工作区"));
    await input(
      view.element.querySelector('[aria-label="工作区名称"]'),
      "R18 专用",
    );
    await input(
      view.element.querySelector('[aria-label="工作区内容分类"]'),
      "r18",
    );
    await click(
      view.element.querySelector('[aria-label="工作区启用 本地导出"]'),
    );
    await click(view.element.querySelector('[aria-label="工作区启用 Pixiv"]'));
    await click(button(view.element, "创建并进入"));
    assert.equal(
      view.element.querySelector('[aria-label="切换工作区"]').value,
      "workspace-1",
    );
    assert.equal(view.element.querySelector("#title").value, "");
    assert.equal(view.element.querySelectorAll(".driver-option").length, 7);
    assert.match(
      view.element.querySelector(".workspace-context").textContent,
      /R18 专用/,
    );
    assert.equal(original.state.draft.title, "全年龄未保存草稿");
    assert.deepEqual(original.state.workspace.selectedTargetIds, []);
    await input(view.element.querySelector("#title"), "成人草稿");
    await click(button(view.element, "本地导出"));
    await click(button(view.element, "历史归档"));
    assert.doesNotMatch(view.element.textContent, /历史一/);
    await input(
      view.element.querySelector('[aria-label="切换工作区"]'),
      "default",
    );
    assert.equal(
      view.element.querySelector("#title").value,
      "全年龄未保存草稿",
    );
    assert.equal(
      view.element.querySelectorAll(".driver-option.selected").length,
      0,
    );
    assert.equal(entries.get("workspace-1").state.draft.title, "成人草稿");
    await click(button(view.element, "历史归档"));
    assert.match(view.element.textContent, /历史一/);
    await input(
      view.element.querySelector('[aria-label="切换工作区"]'),
      "workspace-1",
    );
    assert.equal(view.element.querySelector("#title").value, "成人草稿");
    assert.equal(
      view.element.querySelectorAll(".driver-option.selected").length,
      1,
    );
    await click(view.element.querySelector('[aria-label="管理工作区"]'));
    await click(
      view.element.querySelector('[aria-label="工作区启用 本地导出"]'),
    );
    await click(button(view.element, "保存工作区设置"));
    await click(button(view.element, "新建发布"));
    assert.equal(view.element.querySelectorAll(".driver-option").length, 7);
    assert.equal(
      view.element.querySelectorAll(".driver-option.selected").length,
      0,
    );
  } finally {
    await view.dispose();
  }
});

test("a late refresh from the previous workspace cannot replace the new workspace history", async () => {
  const { root, entries } = workspaceClients();
  await root.createWorkspace({
    name: "全年龄",
    contentRating: "all_ages",
    enabledDriverIds: ["local-archive"],
  });
  await root.switchWorkspace("default");
  const original = entries.get("default");
  seedHistory(original.state);
  let changed, release;
  original.client.onJobsChanged = async (callback) => {
    changed = callback;
    return () => {};
  };
  const view = await mount(root);
  try {
    const oldHistory = structuredClone(original.state.history);
    original.client.history = () =>
      new Promise((resolve) => {
        release = resolve;
      });
    await React.act(async () => changed());
    assert.ok(release);
    await input(
      view.element.querySelector('[aria-label="切换工作区"]'),
      "workspace-1",
    );
    await React.act(async () => release(oldHistory));
    await click(button(view.element, "历史归档"));
    assert.doesNotMatch(view.element.textContent, /历史一/);
    assert.equal(
      view.element.querySelector('[aria-label="切换工作区"]').value,
      "workspace-1",
    );
  } finally {
    await view.dispose();
  }
});

test("failed draft save keeps the current workspace and edited content", async () => {
  const { root, entries } = workspaceClients();
  await root.createWorkspace({
    name: "另一个空间",
    contentRating: "all_ages",
    enabledDriverIds: [],
  });
  await root.switchWorkspace("default");
  entries.get("default").client.saveDraft = async () => {
    throw new Error("磁盘暂时不可写");
  };
  const view = await mount(root);
  try {
    await input(view.element.querySelector("#title"), "不可丢失的编辑");
    await input(
      view.element.querySelector('[aria-label="切换工作区"]'),
      "workspace-1",
    );
    assert.equal(
      view.element.querySelector('[aria-label="切换工作区"]').value,
      "default",
    );
    assert.equal(view.element.querySelector("#title").value, "不可丢失的编辑");
    assert.match(
      view.element.querySelector('[role="alert"]').textContent,
      /磁盘暂时不可写/,
    );
  } finally {
    await view.dispose();
  }
});
test("all-history schedule uses the chosen account and local time; edited settings invalidate preview", async () => {
  const { client, calls, state } = makeClient();
  seedHistory(state);
  state.history.targets.push({
    id: "account-2",
    driverId: "local-archive",
    name: "备用账号",
    createdAt: 0,
  });
  const view = await mount(client);
  try {
    await click(button(view.element, "历史归档"));
    assert.equal(button(view.element, "确认创建计划").disabled, true);
    await click(view.element.querySelector('[aria-label="全选全部历史"]'));
    await input(view.element.querySelector("#replay-target"), "local-archive");
    await input(view.element.querySelector("#replay-mode"), "scheduled");
    await input(view.element.querySelector("#replay-date"), "2030-12-24T09:30");
    await input(view.element.querySelector("#replay-interval"), "1440");
    await click(button(view.element, "检查补传计划"));
    assert.equal(button(view.element, "确认创建计划").disabled, false);
    await input(view.element.querySelector("#replay-interval"), "60");
    assert.equal(button(view.element, "确认创建计划").disabled, true);
    await click(button(view.element, "检查补传计划"));
    await click(button(view.element, "确认创建计划"));
    const req = calls.find((call) => call[0] === "replay")[1];
    assert.deepEqual(req, {
      archiveIds: ["archive-0", "archive-1"],
      targetId: "local-archive",
      startAt: new Date("2030-12-24T09:30").getTime(),
      intervalMinutes: 60,
      force: false,
    });
    assert.match(view.element.textContent, /已创建补传计划：2 条待执行/);
    assert.match(view.element.textContent, /不会自动唤醒电脑/);
  } finally {
    await view.dispose();
  }
});

test("platform login shows one account per platform and reuses an existing login", async () => {
  const { client, state } = makeClient();
  state.history.targets.push({id:"authenticated-pixiv",driverId:"pixiv",name:"CyanAutumn",createdAt:1});
  state.platformAccounts = [{driverId:"pixiv",targetId:"authenticated-pixiv",connected:true,name:"CyanAutumn"}];
  const view = await mount(client);
  try {
    await click(button(view.element,"平台登录"));
    assert.equal(view.element.querySelector('[aria-label="新账号标识"]'),null);
    assert.equal(button(view.element,"添加目标"),undefined);
    const rows = [...view.element.querySelectorAll('.target-row')].filter(row=>row.querySelector('strong')?.textContent==='Pixiv');
    assert.equal(rows.length,1);
    assert.match(rows[0].textContent,/已登录/);
    await click(button(rows[0],"补传历史"));
    assert.equal(view.element.querySelector('#replay-target').value,'authenticated-pixiv');
    const choices=[...view.element.querySelectorAll('#replay-target option')].filter(o=>o.textContent.includes('Pixiv'));
    assert.equal(choices.length,1);
  }finally{await view.dispose();}
});

test("scheduled plan can pause, resume, and cancel from history", async () => {
  const { client, calls, state } = makeClient();
  state.history.plans = [
    {
      id: "plan-1",
      targetId: "local-archive",
      createdAt: Date.now(),
      startAt: Date.now(),
      intervalMinutes: 60,
      status: "active",
      nextReleaseAt: Date.now(),
      archiveIds: ["archive-0"],
      force: false,
    },
  ];
  state.jobs = [{ id: "scheduled-1", planId: "plan-1", status: "scheduled" }];
  const view = await mount(client);
  try {
    await click(button(view.element, "历史归档"));
    await click(button(view.element, "暂停"));
    assert.match(view.element.textContent, /已暂停/);
    await click(button(view.element, "继续"));
    await click(button(view.element, "取消计划"));
    assert.deepEqual(
      calls.filter((call) => call[0] === "plan").map((call) => call.slice(1)),
      [
        ["plan-1", "paused"],
        ["plan-1", "active"],
        ["plan-1", "cancelled"],
      ],
    );
    assert.match(view.element.textContent, /已取消/);
    assert.equal(button(view.element, "取消计划"), undefined);
  } finally {
    await view.dispose();
  }
});

test("Pixiv opens browser login directly with the selected account and form settings", async () => {
  const { client, calls } = makeClient();
  const view = await mount(client);
  try {
    await click(button(view.element, "平台登录"));
    const details = view.element.querySelector('.pixiv-account-help:not(.x-account-help)');
    await React.act(async () => { details.open = true; details.dispatchEvent(new Event("toggle")); });
    await input(details.querySelector('[aria-label="Pixiv 默认标签"]'), "插画，オリジナル");
    await input(details.querySelector('[aria-label="Pixiv 可见性"]'), "public");
    await click(button(view.element, "登录 Pixiv"));
    const submitted = calls.find(c => c[0] === "pixiv-login");
    assert.equal(submitted[1], "pixiv");
    assert.deepEqual(submitted[2].tags, ["插画", "オリジナル"]);
    assert.equal(submitted[2].visibility, "public");
    assert.equal(submitted[2].userId, "123");
    assert.equal(submitted[2].allowComment, false);
    assert.equal(details.querySelector('textarea'), null);
    assert.match(details.textContent, /Pixiv 登录成功/);
    assert.match(view.element.textContent, /默认仅自己可见/);
    
  } finally {
    await view.dispose();
  }
});

test("Pixiv login failures are shown and do not report success", async () => {
  const { client } = makeClient();
  client.loginPixiv = async () => { throw new Error("登录窗口已关闭"); };
  const view = await mount(client);
  try {
    await click(button(view.element, "平台登录"));
    const details = view.element.querySelector('.pixiv-account-help:not(.x-account-help)');
    await React.act(async () => { details.open = true; details.dispatchEvent(new Event("toggle")); });
    await click(button(view.element, "登录 Pixiv"));
    assert.match(details.textContent, /登录窗口已关闭/);
    assert.doesNotMatch(details.textContent, /Pixiv 登录成功/);
    assert.equal(button(view.element, "登录 Pixiv").disabled, false);
  } finally { await view.dispose(); }
});

test("X login opens from its platform card without copying commands", async () => {
  const { client, calls } = makeClient();
  const view = await mount(client);
  try {
    await click(button(view.element,"平台登录"));
    assert.equal(view.element.querySelector('[aria-label="X 配置命令"]'),null);
    await click(button(view.element,"登录 X"));
    assert.deepEqual(calls.find(c=>c[0]==='x-login'),['x-login','x']);
    assert.match(view.element.textContent,/X 登录成功/);
  }finally{await view.dispose();}
});

test("Pixiv publication settings save without opening a login browser",async()=>{
 const {client,calls}=makeClient();const view=await mount(client);
 try{await click(button(view.element,"平台登录"));await click(button(view.element,"保存投稿设置"));assert.ok(calls.find(c=>c[0]==="pixiv-save"));assert.ok(!calls.find(c=>c[0]==="pixiv-login"));assert.match(view.element.textContent,/无需重新登录/);}finally{await view.dispose();}
});

test("Xiaohongshu login opens from its card and reports completion", async()=>{
 const {client,calls}=makeClient();const view=await mount(client);
 try{await click(button(view.element,"平台登录"));await click(button(view.element,"登录小红书"));assert.deepEqual(calls.find(c=>c[0]==="xiaohongshu-login"),["xiaohongshu-login","xiaohongshu"]);assert.match(view.element.textContent,/小红书登录成功/);}finally{await view.dispose();}
});
