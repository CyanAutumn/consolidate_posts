import type { Driver, PluginReport } from "../types";

type Manifest = Driver & {
  schemaVersion: number;
  version: string;
  enabled: boolean;
};

// Raw imports keep a malformed optional plugin from breaking the UI build itself.
const sources = import.meta.glob<string>("../../plugins/*/plugin.json", {
  eager: true,
  query: "?raw",
  import: "default",
});

export function parsePluginPreviews(input: Record<string, string>) {
  const reports: PluginReport[] = [];
  const parsed: { folder: string; manifest: Manifest }[] = [];
  for (const [path, raw] of Object.entries(input).sort(([a], [b]) =>
    a.localeCompare(b),
  )) {
    const folder = path.replace("../../", "").replace("/plugin.json", "");
    try {
      const value = JSON.parse(raw) as Partial<Manifest> | null;
      if (
        !value ||
        value.schemaVersion !== 1 ||
        typeof value.id !== "string" ||
        !/^[a-z0-9-]{1,64}$/.test(value.id) ||
        typeof value.name !== "string" ||
        typeof value.version !== "string" ||
        typeof value.category !== "string" ||
        typeof value.description !== "string" ||
        typeof value.enabled !== "boolean" ||
        !["ready", "planned"].includes(value.status ?? "") ||
        !Array.isArray(value.kinds) ||
        value.kinds.some(
          (kind) => !["image", "video", "text", "model", "file"].includes(kind),
        ) ||
        typeof value.acceptsTitle !== "boolean" ||
        typeof value.acceptsBody !== "boolean"
      ) {
        throw new Error("清单字段或版本无效");
      }
      parsed.push({
        folder,
        manifest: {
          ...value,
          maxFiles: value.maxFiles ?? null,
          maxFileBytes: value.maxFileBytes ?? null,
        } as Manifest,
      });
    } catch {
      reports.push({
        folder,
        id: null,
        name: null,
        version: null,
        state: "error",
        message: "无法读取 plugin.json，请检查 JSON 格式和必填字段",
      });
    }
  }
  const catalog = new Map<string, Driver>();
  for (const { folder, manifest } of parsed) {
    const duplicate =
      manifest.enabled &&
      parsed.filter(
        (item) => item.manifest.enabled && item.manifest.id === manifest.id,
      ).length > 1;
    if (manifest.enabled)
      catalog.set(manifest.id, {
        ...manifest,
        status: duplicate ? "unavailable" : manifest.status,
      });
    reports.push({
      folder,
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      state: !manifest.enabled
        ? "disabled"
        : duplicate
          ? "error"
          : manifest.status,
      message: !manifest.enabled
        ? "插件已禁用"
        : duplicate
          ? "插件 ID 重复，所有同 ID 插件均未加载"
          : "浏览器预览；入口可用性由桌面版检查",
    });
  }
  return {
    catalog: [...catalog.values()].sort(
      (a, b) =>
        Number(b.id === "local-archive") - Number(a.id === "local-archive") ||
        a.id.localeCompare(b.id),
    ),
    reports,
  };
}
const preview = parsePluginPreviews(sources);
export const previewCatalog = preview.catalog;
export const previewPluginReports = preview.reports;
