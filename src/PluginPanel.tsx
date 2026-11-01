import { FolderOpen, Plug } from "lucide-react";
import type { PluginReport } from "./types";

const names: Record<string, string> = {
  ready: "已加载",
  planned: "待接入",
  disabled: "已禁用",
  error: "加载失败",
};
export function PluginPanel({
  directory,
  reports,
}: {
  directory: string;
  reports: PluginReport[];
}) {
  return (
    <section className="panel plugin-panel">
      <div className="section-heading">
        <div>
          <Plug size={19} />
          <h2>文件夹插件（全局）</h2>
        </div>
        <span className="count-badge">{reports.length}</span>
      </div>
      <p className="section-description">
        每个子文件夹是一个插件，使用 plugin.json
        统一入口。新增、更新或启用插件后，重启应用加载。
        各工作区的使用范围请在“工作区管理”中勾选。
      </p>
      <div className="plugin-path">
        <FolderOpen size={16} />
        <code>{directory}</code>
      </div>
      <div className="plugin-list">
        {reports.map((report) => (
          <article
            className={`plugin-row ${report.state === "error" ? "plugin-error" : ""}`}
            key={report.folder}
          >
            <div>
              <strong>
                {report.name ??
                  report.folder.split(/[\\/]/).pop() ??
                  "插件目录"}
                {report.version && <small>v{report.version}</small>}
              </strong>
              <p>{report.message}</p>
              <code>{report.folder}</code>
            </div>
            <span
              className={`pill ${report.state === "ready" ? "success" : report.state === "error" ? "status-failed" : ""}`}
            >
              {names[report.state] ?? report.state}
            </span>
          </article>
        ))}
      </div>
    </section>
  );
}
