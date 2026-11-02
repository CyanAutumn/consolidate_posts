import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { previewCatalog as catalog, previewPluginReports } from "./catalog";
import type { Client } from "../types";

const desktop = isTauri();
const unavailable = async (): Promise<never> => {
  throw new Error("请启动桌面版以使用文件和上传功能");
};
function desktopClient(workspaceId?: string): Client {
  const call = <T>(command: string, args = {}) =>
    invoke<T>(command, { ...args, workspaceId });
  return {
    forWorkspace: desktopClient,
    createWorkspace: (config) => call("create_workspace", { config }),
    switchWorkspace: (id) => invoke("switch_workspace", { workspaceId: id }),
    updateWorkspace: (config) => call("update_workspace", { config }),
    selectTargets: (ids) => call("select_targets", { ids }),
    desktop,
    platformAccounts: () => call("platform_accounts"),
    savePixivSettings: (targetId, settings) => call("save_pixiv_settings", { targetId, settings }),
    loginBilibili: (targetId) => call("login_bilibili", { targetId }),
    bilibiliSettings: (targetId) => call("bilibili_settings", { targetId }),
    saveBilibiliSettings: (targetId,settings) => call("save_bilibili_settings", { targetId,settings }),
    loginXiaohongshu: (targetId) => call("login_xiaohongshu", { targetId }),
    loginX: (targetId) => call("login_x", { targetId }),
    pixivSettings: (targetId) => call("pixiv_settings", { targetId }),
    loginPixiv: (targetId, settings) => call("login_pixiv", { targetId, settings }),
    bootstrap: () => call("bootstrap"),
    importPaths: (paths) => call("import_files", { paths }),
    chooseFiles: async () => {
      const result = await open({
        multiple: true,
        directory: false,
        title: "添加待发布的附件",
      });
      return result == null ? [] : Array.isArray(result) ? result : [result];
    },
    saveDraft: (draft) => call("save_draft", { draft }),
    preflight: (draft) => call("preflight", { draft }),
    enqueue: (draft, targetIds) =>
      call("enqueue_upload", { request: { draft, targetIds } }),
    jobs: () => call("list_jobs"),
    cancel: (id) => call("cancel_job", { id }),
    retry: (id) => call("retry_job", { id }),
    history: () => call("list_history"),
    createTarget: (driverId, name) => call("create_target", { driverId, name }),
    previewReplay: (request) => call("preview_replay", { request }),
    scheduleReplay: (request) => call("schedule_replay", { request }),
    setPlanStatus: (id, status) => call("set_plan_status", { id, status }),
    onJobsChanged: (callback) => listen("jobs-changed", callback),
    onDrop: (callback, hover) =>
      getCurrentWebview().onDragDropEvent((event) => {
        hover(event.payload.type === "enter" || event.payload.type === "over");
        if (event.payload.type === "drop") callback(event.payload.paths);
      }),
  };
}
const preview: Client = {
  platformAccounts: async () => [],
  savePixivSettings: unavailable,
  loginBilibili: unavailable,
  bilibiliSettings: unavailable,
  saveBilibiliSettings: unavailable,
  loginXiaohongshu: unavailable,
  loginX: unavailable,
  pixivSettings: unavailable,
  loginPixiv: unavailable,
  forWorkspace: () => preview,
  createWorkspace: unavailable,
  switchWorkspace: unavailable,
  updateWorkspace: unavailable,
  selectTargets: unavailable,
  desktop,
  bootstrap: async () => ({
    workspace: previewWorkspace(),
    workspaces: [previewWorkspace()],
    drivers: catalog,
    pluginDir: "plugins/",
    pluginReports: previewPluginReports,
    draft: { title: "", body: "", assetIds: [] },
    assets: [],
    jobs: [],
    dataDir: "",
    archiveDir: "",
    concurrency: 2,
    history: previewHistory(),
  }),
  importPaths: unavailable,
  chooseFiles: unavailable,
  saveDraft: unavailable,
  preflight: unavailable,
  enqueue: unavailable,
  jobs: async () => [],
  cancel: unavailable,
  retry: unavailable,
  history: async () => previewHistory(),
  createTarget: unavailable,
  previewReplay: unavailable,
  scheduleReplay: unavailable,
  setPlanStatus: unavailable,
  onJobsChanged: async () => () => {},
  onDrop: async () => () => {},
};

export const api = desktop ? desktopClient() : preview;

function previewWorkspace() {
  return {
    id: "default",
    name: "默认工作区",
    contentRating: "unspecified" as const,
    enabledDriverIds: catalog.map((driver) => driver.id),
    selectedTargetIds: ["local-archive"],
    createdAt: 0,
  };
}

function previewHistory() {
  return {
    archives: [],
    plans: [],
    targets: catalog.map((driver) => ({
      id: driver.id,
      driverId: driver.id,
      name:
        driver.id === "local-archive" ? "本机默认目录" : "默认账号（待连接）",
      createdAt: 0,
    })),
  };
}

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error)
    return String(error.message);
  return "操作失败，请重试";
}
