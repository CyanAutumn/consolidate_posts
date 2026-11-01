import { useEffect, useState } from "react";
import {
  ArrowUpRight,
  ArrowUpToLine,
  ArrowRight,
  Layers3,
  LayoutDashboard,
  ListTodo,
  Plug,
  SlidersHorizontal,
  Image,
  Film,
  Box,
  FileText,
  File,
  Plus,
  X,
  Check,
  CheckCheck,
  CircleCheck,
  Clock3,
  HardDrive,
  FolderOpen,
  Info,
  LoaderCircle,
  RotateCcw,
  ShieldCheck,
  Save,
  ChevronRight,
  Copy,
  ChevronDown,
  Upload,
  AlertCircle,
  Archive,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { api, errorMessage } from "./lib/api";
import {
  activeJob,
  compatibility,
  formatBytes,
  kindNames,
  statusNames,
} from "./lib/format";
import type { AssetKind, Client, Driver, Job } from "./types";
import { useWorkspace } from "./useWorkspace";
import { HistoryPage } from "./HistoryPage";
import { TargetManager } from "./TargetManager";
import { platformTargets, normalizePlatforms } from "./lib/platformAccounts";
import { PluginPanel } from "./PluginPanel";
import { WorkspaceManager, ratingNames } from "./WorkspaceManager";

type Page = "compose" | "tasks" | "history" | "drivers" | "settings";
const icons: Record<AssetKind, LucideIcon> = {
  image: Image,
  video: Film,
  text: FileText,
  model: Box,
  file: File,
};
const pageNames: Record<Page, string> = {
  compose: "发布工作台",
  tasks: "上传任务",
  history: "历史归档",
  drivers: "平台登录",
  settings: "工作区管理",
};
const driverMarks: Record<string, string> = {
  "local-archive": "本",
  bilibili: "B",
  xiaohongshu: "红",
  pixiv: "P",
  x: "𝕏",
  huggingface: "HF",
  civitai: "C",
};
function DriverMark({ driver }: { driver: Driver }) {
  return (
    <span className={`driver-mark mark-${driver.id}`}>
      {driver.id === "local-archive" ? (
        <HardDrive size={21} />
      ) : (
        (driverMarks[driver.id] ?? driver.name.slice(0, 1))
      )}
    </span>
  );
}

export default function App({ client: rootClient = api }: { client?: Client }) {
  const workspace = useWorkspace(rootClient);
  const { client, selected, setSelected } = workspace;
  const { snapshot, draft, assets, jobs, history, busy, saved, validation } =
    workspace;
  const [replayTarget, setReplayTarget] = useState("");
  const [page, setPage] = useState<Page>("compose");
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [page]);
  const [creatingWorkspace, setCreatingWorkspace] = useState(false);
  useEffect(() => {
    setPage("compose");
    setReplayTarget("");
    setTaskFilter("all");
    setCreatingWorkspace(false);
  }, [snapshot?.workspace.id]);
  const [compatibleOnly, setCompatibleOnly] = useState(false);
  const [taskFilter, setTaskFilter] = useState("all");
  const catalog = snapshot?.drivers ?? [];
  const drivers = catalog.filter((driver) =>
    snapshot?.workspace.enabledDriverIds.includes(driver.id),
  );
  const activeCount = jobs.filter((job) => activeJob(job.status)).length;
  const completedCount = jobs.filter(
    (job) => job.status === "succeeded",
  ).length;
  const totalSize = assets.reduce((size, asset) => size + asset.size, 0);
  const hasContent = Boolean(
    assets.length || draft.title.trim() || draft.body.trim(),
  );
  const available = drivers.filter(
    (driver) =>
      driver.status === "ready" &&
      compatibility(driver, draft, assets).length === 0,
  );
  const primaryTargets = platformTargets(history.targets, snapshot?.platformAccounts);
  const normalizedSelection = normalizePlatforms(selected, history.targets, snapshot?.platformAccounts);
  const targets = primaryTargets.flatMap((target) => {
    const driver = drivers.find((driver) => driver.id === target.driverId);
    return driver ? [{ target, driver }] : [];
  });
  const chosen = normalizedSelection.filter((id) =>
    targets.some(
      ({ target, driver }) => target.id === id && available.includes(driver),
    ),
  );
  const shownTargets = targets.filter(
    ({ driver }) =>
      !compatibleOnly ||
      (driver.status === "ready" &&
        !compatibility(driver, draft, assets).length),
  );
  const shownJobs = jobs.filter(
    (job) =>
      taskFilter === "all" ||
      (taskFilter === "active"
        ? activeJob(job.status)
        : taskFilter === "failed"
          ? ["failed", "interrupted"].includes(job.status)
          : job.status === taskFilter),
  );
  const toggle = (id: string) =>
    setSelected(normalizedSelection.includes(id) ? normalizedSelection.filter(item => item !== id) : [...normalizedSelection, id]);

  return (
    <div
      className="app-shell"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        if (!client.desktop)
          workspace.setError("浏览器预览不读取本机文件，请使用桌面版导入");
      }}
    >
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-icon">
            <ArrowUpToLine size={24} />
          </span>
          <div>
            <strong>Upload Hub</strong>
            <small>聚合上传工作台</small>
          </div>
        </div>
        <div className="workspace-label">
          <span
            className={`workspace-avatar ${snapshot?.workspace.contentRating === "r18" ? "rating-r18" : ""}`}
          >
            {snapshot?.workspace.contentRating === "r18" ? "18" : "W"}
          </span>
          <label>
            当前工作区
            <select
              aria-label="切换工作区"
              value={snapshot?.workspace.id ?? ""}
              disabled={busy || !snapshot || !client.desktop}
              onChange={(event) =>
                void workspace.switchWorkspace(event.target.value)
              }
            >
              {snapshot?.workspaces.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
          </label>
          <button
            aria-label="管理工作区"
            title="管理工作区"
            disabled={!snapshot || busy}
            onClick={() => {
              setCreatingWorkspace(false);
              setPage("settings");
            }}
          >
            <SlidersHorizontal size={15} />
          </button>
        </div>
        <div className="nav-label">工作空间</div>
        <nav aria-label="主导航">
          {(
            [
              ["compose", LayoutDashboard],
              ["tasks", ListTodo],
              ["history", Archive],
              ["drivers", Plug],
            ] as [Page, LucideIcon][]
          ).map(([id, Icon]) => (
            <button
              key={id}
              className={`nav-item ${page === id ? "active" : ""}`}
              aria-current={page === id ? "page" : undefined}
              aria-label={pageNames[id]}
              onClick={() => setPage(id)}
            >
              <Icon size={18} />
              <span>{pageNames[id]}</span>
              {id === "tasks" && activeCount > 0 && <b>{activeCount}</b>}
              {id === "drivers" && <small>{drivers.length}</small>}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="sidebar-note">
            <Layers3 size={20} />
            <strong>一份内容，多处抵达</strong>
            <p>
              整理好你的创作，
              <br />
              让每个平台各司其职。
            </p>
            <span>FRAMEWORK · 0.1</span>
          </div>
          <button
            className={`nav-item ${page === "settings" ? "active" : ""}`}
            onClick={() => setPage("settings")}
          >
            <SlidersHorizontal size={18} />
            工作区管理
          </button>
          <div className="engine-status">
            <i className={client.desktop ? "online" : ""} />
            {client.desktop
              ? snapshot
                ? "Rust 引擎已连接"
                : "正在连接引擎"
              : "浏览器界面预览"}
            <span>v0.1.0</span>
          </div>
        </div>
      </aside>
      <div className="main-shell">
        <header className="topbar">
          <div className="breadcrumb">
            工作空间 <ChevronRight size={14} />
            <strong>{pageNames[page]}</strong>
          </div>
          <div className="topbar-right">
            <div className="mobile-workspace">
              <label>
                当前工作区
                <select
                  aria-label="切换工作区（窄屏）"
                  value={snapshot?.workspace.id ?? ""}
                  disabled={busy || !snapshot || !client.desktop}
                  onChange={(event) =>
                    void workspace.switchWorkspace(event.target.value)
                  }
                >
                  {snapshot?.workspaces.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="button secondary small"
                aria-label="管理工作区（窄屏）"
                disabled={!snapshot || busy}
                onClick={() => {
                  setCreatingWorkspace(false);
                  setPage("settings");
                }}
              >
                <SlidersHorizontal size={15} />
                管理
              </button>
            </div>
            <span className="local-badge">
              <ShieldCheck size={14} />
              本地工作区
            </span>
            <span className="avatar">U</span>
          </div>
        </header>
        <main>
          {!client.desktop && (
            <div className="preview-banner">
              <Info size={16} />
              <span>
                当前为界面预览。桌面版支持文件拖入、草稿保存和实际上传。
              </span>
              <code>npm run desktop:dev</code>
            </div>
          )}
          {workspace.error && (
            <div className="feedback error" role="alert">
              <AlertCircle size={17} />
              <span>{workspace.error}</span>
              <button
                aria-label="关闭错误"
                onClick={() => workspace.setError("")}
              >
                <X size={16} />
              </button>
            </div>
          )}
          {workspace.notice && (
            <div className="feedback notice" role="status">
              <CircleCheck size={16} />
              {workspace.notice}
            </div>
          )}
          <div className="page-heading">
            <div>
              <div className="eyebrow">
                {page === "compose"
                  ? "CREATE ONCE. SHARE EVERYWHERE."
                  : "YOUR PUBLISHING WORKSPACE"}
              </div>
              <h1>{pageNames[page]}</h1>
              <p>
                {page === "compose"
                  ? "放入你的创作，选择目的地，其余交给上传工作台。"
                  : page === "tasks"
                    ? "每个平台独立执行，每一次发布都有迹可循。"
                    : page === "history"
                      ? "保存每一份创作，随时补传到需要的平台。"
                      : page === "drivers"
                        ? "一个平台，一个驱动。按内容能力连接你的发布目的地。"
                        : "文件留在本机，任务有据可查。"}
              </p>
            </div>
            {page === "compose" ? (
              <button
                className="button secondary"
                disabled={!client.desktop || busy || !snapshot || saved}
                onClick={() => void workspace.save()}
              >
                <Save size={16} />
                {saved && client.desktop ? "草稿已保存" : "保存草稿"}
              </button>
            ) : (
              <button
                className="button secondary"
                onClick={() => setPage("compose")}
              >
                <Plus size={16} />
                新建发布
              </button>
            )}
          </div>
          {snapshot && (
            <div
              className={`workspace-context ${snapshot.workspace.contentRating === "r18" ? "r18" : ""}`}
            >
              <div>
                <Layers3 size={16} />
                <strong>{snapshot.workspace.name}</strong>
                <span className="pill">
                  {ratingNames[snapshot.workspace.contentRating]}
                </span>
              </div>
              <span>
                仅显示本工作区内容 · {drivers.length} 个插件 · 切换不暂停任务
              </span>
            </div>
          )}

          {!snapshot && !workspace.error && (
            <div className="empty-state">
              <LoaderCircle className="spin" />
              正在加载工作区…
            </div>
          )}
          {snapshot && page === "compose" && (
            <>
              <div className="composer-grid">
                <div className="composer-left">
                  <section className="panel attachments-panel">
                    <div className="section-heading">
                      <div>
                        <span className="step">01</span>
                        <h2>添加内容</h2>
                        <span className="subtle">
                          {assets.length
                            ? `${assets.length} 个附件`
                            : "从一份创作开始"}
                        </span>
                      </div>
                      <span className="mini-label">支持多种格式</span>
                    </div>
                    <button
                      className={`dropzone ${workspace.dragging ? "dragging" : ""} ${assets.length ? "compact" : ""}`}
                      disabled={busy || !client.desktop}
                      onClick={() => void workspace.chooseFiles()}
                    >
                      <div className="file-illustration">
                        <span>
                          <Image size={26} />
                        </span>
                        <span>
                          <Upload size={28} />
                        </span>
                        <span>
                          <Box size={26} />
                        </span>
                      </div>
                      <strong>
                        {workspace.dragging
                          ? "松开鼠标，导入文件"
                          : "拖入文件，开始发布"}
                      </strong>
                      <p>
                        或 <em>浏览本地文件</em>，一次添加多个附件
                      </p>
                      <div className="supported-kinds">
                        <span>图片</span>
                        <i /> <span>视频</span>
                        <i />
                        <span>模型</span>
                        <i />
                        <span>文档</span>
                      </div>
                    </button>
                    {assets.length > 0 && (
                      <div className="asset-list">
                        {assets.map((asset) => {
                          const Icon = icons[asset.kind];
                          return (
                            <div className="asset-row" key={asset.id}>
                              <span className={`asset-icon kind-${asset.kind}`}>
                                <Icon size={20} />
                              </span>
                              <div className="asset-name">
                                <strong title={asset.path}>{asset.name}</strong>
                                <small>
                                  {kindNames[asset.kind]} ·{" "}
                                  {formatBytes(asset.size)}
                                </small>
                              </div>
                              <CircleCheck size={15} className="green" />
                              <button
                                className="icon-button"
                                aria-label={`移除 ${asset.name}`}
                                disabled={busy}
                                onClick={() =>
                                  workspace.updateDraft({
                                    ...draft,
                                    assetIds: draft.assetIds.filter(
                                      (id) => id !== asset.id,
                                    ),
                                  })
                                }
                              >
                                <X size={16} />
                              </button>
                            </div>
                          );
                        })}
                        <button
                          className="add-more"
                          disabled={busy || !client.desktop}
                          onClick={() => void workspace.chooseFiles()}
                        >
                          <Plus size={15} />
                          继续添加文件
                        </button>
                      </div>
                    )}
                    <div className="panel-footnote">
                      <ShieldCheck size={14} />
                      导入仅引用源文件，上传时才读取内容。
                    </div>
                  </section>
                  <section className="panel content-panel">
                    <div className="section-heading">
                      <div>
                        <span className="step">02</span>
                        <h2>完善文案</h2>
                      </div>
                      <span className="mini-label">也可单独发布文字</span>
                    </div>
                    <label htmlFor="title">
                      标题 <span>选填</span>
                    </label>
                    <div className="input-wrap">
                      <input
                        id="title"
                        disabled={busy}
                        maxLength={300}
                        placeholder="为这次创作起个名字"
                        value={draft.title}
                        onChange={(event) =>
                          workspace.updateDraft({
                            ...draft,
                            title: event.target.value,
                          })
                        }
                      />
                      <small>{draft.title.length}/300</small>
                    </div>
                    <label htmlFor="body">
                      正文 / 描述 <span>选填</span>
                    </label>
                    <textarea
                      id="body"
                      disabled={busy}
                      maxLength={100000}
                      rows={6}
                      placeholder="分享创作故事、补充作品说明，或写下模型的使用方法…"
                      value={draft.body}
                      onChange={(event) =>
                        workspace.updateDraft({
                          ...draft,
                          body: event.target.value,
                        })
                      }
                    />
                    <div className="editor-footer">
                      <span>
                        <FileText size={13} />
                        支持纯文本与 Markdown 源文
                      </span>
                      <span>{draft.body.length.toLocaleString()} 字符</span>
                    </div>
                  </section>
                </div>
                <div className="composer-right">
                  <section className="panel destination-panel">
                    <div className="section-heading">
                      <div>
                        <span className="step">03</span>
                        <h2>选择发布平台</h2>
                      </div>
                      <span className="count-badge">{chosen.length}</span>
                    </div>
                    <p className="section-description">
                      先自动留档，再发布到选中的平台。每个平台使用已登录的账号。
                    </p>
                    <label className="filter-toggle">
                      <input
                        type="checkbox"
                        checked={compatibleOnly}
                        onChange={(event) =>
                          setCompatibleOnly(event.target.checked)
                        }
                      />
                      <span>仅显示当前可用的平台</span>
                      <span>{available.length} 个</span>
                    </label>
                    <div className="driver-options">
                      {shownTargets.map(({ target, driver }) => {
                        const issues = compatibility(driver, draft, assets);
                        const enabled =
                          driver.status === "ready" && !issues.length;
                        const check = validation.find(
                          (item) => item.driverId === driver.id,
                        );
                        return (
                          <button
                            key={target.id}
                            className={`driver-option ${chosen.includes(target.id) ? "selected" : ""}`}
                            disabled={!enabled || busy}
                            aria-pressed={chosen.includes(target.id)}
                            onClick={() => toggle(target.id)}
                            title={issues.join("；") || driver.description}
                          >
                            <DriverMark driver={driver} />
                            <div className="driver-option-info">
                              <strong>
                                {driver.name}
                                <span
                                  className={`driver-status ${enabled ? "ready" : ""}`}
                                >
                                  {driver.status !== "ready"
                                    ? driver.status === "unavailable"
                                      ? "加载失败"
                                      : "待接入"
                                    : issues.length
                                      ? "不兼容"
                                      : "可用"}
                                </span>
                              </strong>
                              <small>{driver.id === "local-archive" ? "保存到本机" : snapshot.platformAccounts?.find(a => a.driverId === driver.id)?.connected ? "已登录" : "未登录"}</small>
                              <small>
                                {issues.length
                                  ? issues.join(" · ")
                                  : driver.kinds
                                      .map((kind) => kindNames[kind])
                                      .join(" / ")}
                              </small>
                              {check && driver.status === "ready" && (
                                <small
                                  className={check.eligible ? "green" : "red"}
                                >
                                  {check.eligible
                                    ? "发布检查通过"
                                    : check.issues.join("；")}
                                </small>
                              )}
                            </div>
                            <span className="checkbox">
                              {chosen.includes(target.id) && (
                                <Check size={13} />
                              )}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                    <div className="driver-note">
                      <Info size={15} />
                      <span>
                        本地留档始终执行。未就绪或加载失败的插件不会参与上传。
                      </span>
                    </div>
                  </section>
                  <div className="flow-note">
                    <div className="flow-icons">
                      <FileText size={17} />
                      <span />
                      <Layers3 size={17} />
                      <span />
                      <ArrowUpRight size={17} />
                    </div>
                    <strong>先留档，再分发</strong>
                    <p>
                      标题、正文和附件副本保存在本机，可从历史按时补传到各平台。
                    </p>
                  </div>
                </div>
              </div>
              <div className="publish-bar">
                <div className="publish-summary">
                  <span className="summary-icon">
                    <Layers3 size={21} />
                  </span>
                  <div>
                    <strong>
                      {assets.length} 个附件 <span>·</span> {chosen.length}{" "}
                      个目的地
                    </strong>
                    <small>
                      {formatBytes(totalSize)}
                      {draft.title || draft.body
                        ? " · 附带标题 / 正文"
                        : " · 准备好后即可开始"}
                      {!saved ? " · 草稿有未保存修改" : ""}
                    </small>
                  </div>
                </div>
                <div className="publish-actions">
                  <button
                    className="button secondary"
                    disabled={!client.desktop || !hasContent || busy}
                    onClick={() => void workspace.check()}
                  >
                    <ShieldCheck size={16} />
                    发布检查
                  </button>
                  <button
                    className="button primary"
                    disabled={!client.desktop || !hasContent || busy}
                    onClick={() =>
                      void workspace.publish(chosen, () => setPage("tasks"))
                    }
                  >
                    {busy ? (
                      <LoaderCircle size={17} className="spin" />
                    ) : (
                      <ArrowUpToLine size={17} />
                    )}
                    {chosen.length ? "一键上传" : "仅保存归档"}
                    <ArrowRight size={16} />
                  </button>
                </div>
              </div>
            </>
          )}

          {snapshot && page === "tasks" && (
            <>
              <div className="stats-row">
                <Stat label="全部任务" value={jobs.length} icon={Layers3} />
                <Stat label="进行中" value={activeCount} icon={Clock3} />
                <Stat label="已完成" value={completedCount} icon={CheckCheck} />
              </div>
              <section className="panel tasks-panel">
                <div className="task-tabs">
                  {[
                    ["all", "全部任务"],
                    ["active", "进行中"],
                    ["succeeded", "已完成"],
                    ["failed", "失败 / 中断"],
                    ["cancelled", "已取消"],
                  ].map(([id, label]) => (
                    <button
                      className={taskFilter === id ? "selected" : ""}
                      key={id}
                      onClick={() => setTaskFilter(id)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                {shownJobs.length ? (
                  <div className="jobs-list">
                    {shownJobs.map((job) => (
                      <JobRow
                        key={job.id}
                        job={job}
                        targetName={
                          history.targets.find(
                            (target) => target.id === job.targetId,
                          )?.name
                        }
                        driver={catalog.find(
                          (driver) => driver.id === job.driverId,
                        )}
                        busy={busy}
                        cancel={() => void workspace.cancel(job.id)}
                        retry={() => void workspace.retry(job.id)}
                        onError={workspace.setError}
                      />
                    ))}
                  </div>
                ) : (
                  <div className="empty-state">
                    <span className="empty-icon">
                      <ListTodo size={30} />
                    </span>
                    <h3>
                      {jobs.length
                        ? "当前分类没有任务"
                        : "你的第一次发布，从这里开始"}
                    </h3>
                    <p>添加内容并选择平台后，任务进度会显示在这里。</p>
                    <button
                      className="button secondary"
                      onClick={() => setPage("compose")}
                    >
                      去发布工作台 <ArrowRight size={15} />
                    </button>
                  </div>
                )}
              </section>
            </>
          )}

          {snapshot && page === "history" && (
            <HistoryPage
              key={`${snapshot.workspace.id}:${snapshot.workspace.enabledDriverIds.join(",")}`}
              client={client}
              history={{ ...history, targets: primaryTargets, plans: history.plans.map(plan => ({ ...plan, targetId: primaryTargets.find(t => t.driverId === history.targets.find(old => old.id === plan.targetId)?.driverId)?.id ?? plan.targetId })) }}
              drivers={drivers}
              catalog={catalog}
              jobs={jobs}
              initialTarget={replayTarget}
              refresh={workspace.refresh}
              manageTargets={() => setPage("drivers")}
            />
          )}

          {snapshot && page === "drivers" && (
            <>
              <div className="info-strip">
                <Plug size={19} />
                <span>
                  <strong>
                    {
                      drivers.filter((driver) => driver.status === "ready")
                        .length
                    }{" "}
                    个驱动已就绪
                  </strong>{" "}
                  ·{" "}
                  {
                    drivers.filter((driver) => driver.status === "planned")
                      .length
                  }{" "}
                  个平台计划接入。平台登录与专用投稿字段将在各驱动中提供。
                </span>
              </div>
              <TargetManager
                dataDir={snapshot.dataDir}
                pluginDir={snapshot.pluginDir}
                key={`${snapshot.workspace.id}:${snapshot.workspace.enabledDriverIds.join(",")}`}
                client={client}
                drivers={catalog}
                enabledDriverIds={snapshot.workspace.enabledDriverIds}
                accounts={snapshot.platformAccounts ?? []}
                targets={primaryTargets}
                refresh={workspace.refresh}
                replay={(id) => {
                  setReplayTarget(id);
                  setPage("history");
                }}
              />
              <details className="panel"><summary>高级插件设置</summary>
              <PluginPanel
                directory={snapshot.pluginDir}
                reports={snapshot.pluginReports}
              />
              <div className="driver-grid">
                {drivers.map((driver) => (
                  <section className="panel driver-detail" key={driver.id}>
                    <div className="driver-detail-top">
                      <DriverMark driver={driver} />
                      <span
                        className={`pill ${driver.status === "ready" ? "success" : ""}`}
                      >
                        {driver.status === "ready" ? "已接入" : "计划接入"}
                      </span>
                    </div>
                    <h2>{driver.name}</h2>
                    <span className="subtle">{driver.category}</span>
                    <p>{driver.description}</p>
                    <div className="type-tags">
                      {driver.kinds.map((kind) => (
                        <span key={kind}>{kindNames[kind]}</span>
                      ))}
                    </div>
                    <div className="driver-detail-footer">
                      {driver.status === "ready" ? (
                        <>
                          <CircleCheck size={14} />
                          可在工作台选择
                        </>
                      ) : (
                        <>
                          <Clock3 size={14} />
                          能力为设计目标，尚不可上传
                        </>
                      )}
                    </div>
                  </section>
                ))}
              </div>
              </details>
            </>
          )}

          {snapshot && page === "settings" && (
            <>
              <WorkspaceManager
                key={`${snapshot.workspace.id}:${creatingWorkspace}:${JSON.stringify(snapshot.workspace)}`}
                profile={snapshot.workspace}
                drivers={catalog}
                creating={creatingWorkspace}
                setCreating={setCreatingWorkspace}
                busy={busy}
                desktop={client.desktop}
                create={workspace.createWorkspace}
                update={workspace.updateWorkspace}
              />
              <section className="panel settings-panel">
                <h2>运行与存储</h2>
                <p>沿用 CyanTools2 的桌面技术路线，上传业务由 Rust 执行。</p>
                <dl>
                  <dt>当前运行环境</dt>
                  <dd>
                    {client.desktop ? "Tauri 2 桌面应用" : "浏览器界面预览"}
                  </dd>
                  <dt>前端</dt>
                  <dd>React 19 · TypeScript · Vite</dd>
                  <dt>后端</dt>
                  <dd>Rust · SQLite · 独立平台驱动</dd>
                  <dt>本工作区同时执行的任务</dt>
                  <dd>{snapshot.concurrency} 个（其他工作区独立运行）</dd>
                  <dt>本机数据目录</dt>
                  <dd>{snapshot.dataDir || "桌面版启动后创建"}</dd>
                  <dt>本地归档目录</dt>
                  <dd>{snapshot.archiveDir || "桌面版启动后创建"}</dd>
                  <dt>插件目录</dt>
                  <dd>{snapshot.pluginDir}</dd>
                </dl>
                <div className="settings-note">
                  <Info size={17} />
                  <p>
                    发布时自动留存文件副本、标题和正文。定时计划需要保持应用开启；关闭后待执行计划保留，重启后按间隔继续。正在执行的任务若中断，需核对后手动重试。每个平台使用已登录的账号。
                  </p>
                </div>
              </section>
            </>
          )}
          <footer className="page-footer">
            <span>UPLOAD HUB</span>
            <span>把时间留给创作。</span>
            <span>本地优先 · 驱动扩展</span>
          </footer>
        </main>
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  icon: Icon,
}: {
  label: string;
  value: number;
  icon: LucideIcon;
}) {
  return (
    <div className="panel stat">
      <div>
        <span>{label}</span>
        <strong>{value.toString().padStart(2, "0")}</strong>
      </div>
      <Icon size={23} />
    </div>
  );
}

function JobRow({
  job,
  driver,
  busy,
  cancel,
  retry,
  onError,
}: {
  job: Job;
  driver?: Driver;
  targetName?: string;
  busy: boolean;
  cancel: () => void;
  retry: () => void;
  onError: (message: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const progress =
    job.status === "succeeded"
      ? 100
      : job.totalBytes > 0
        ? Math.min(99, Math.round((job.completedBytes / job.totalBytes) * 100))
        : 0;
  return (
    <article className="job">
      <div className="job-top">
        {driver ? <DriverMark driver={driver} /> : <HardDrive size={22} />}
        <div className="job-title">
          <strong>
            {job.publication.title ||
              job.publication.assets[0]?.name ||
              "文字发布"}
          </strong>
          <small>
            {job.kind === "archive"
              ? "本地留档"
              : `${driver?.name ?? job.driverId}`}{" "}
            · {job.publication.assets.length} 个附件 · 第 {job.attempt} 次尝试 ·{" "}
            {new Date(job.createdAt).toLocaleString("zh-CN", { hour12: false })}
          </small>
        </div>
        <span className={`pill status-${job.status}`}>
          {["running", "cancelling"].includes(job.status) && (
            <LoaderCircle size={12} className="spin" />
          )}
          {statusNames[job.status]}
        </span>
      </div>
      <div className="progress-row">
        <div className="progress-track">
          <div style={{ width: `${progress}%` }} />
        </div>
        <span>{progress}%</span>
      </div>
      <div className="job-meta">
        <span>
          {formatBytes(job.completedBytes)} / {formatBytes(job.totalBytes)}
        </span>
        <div>
          {["queued", "running", "scheduled", "waiting_archive"].includes(
            job.status,
          ) && (
            <button disabled={busy} onClick={cancel}>
              <X size={14} />
              取消任务
            </button>
          )}
          {["failed", "cancelled", "interrupted"].includes(job.status) && (
            <button disabled={busy} onClick={retry}>
              <RotateCcw size={14} />
              重试
            </button>
          )}
          <button onClick={() => setExpanded(!expanded)}>
            {expanded ? "收起详情" : "查看详情"}
            <ChevronDown size={14} />
          </button>
        </div>
      </div>
      {job.error && <p className="job-error">{job.error.message}</p>}
      {expanded && (
        <div className="job-details">
          <span>任务 ID：{job.id}</span>
          {job.archiveId && <span>归档 ID：{job.archiveId}</span>}
          {job.scheduledAt && (
            <span>
              最早执行：
              {new Date(job.scheduledAt).toLocaleString("zh-CN", {
                hour12: false,
              })}
              （实际时间受队列和计划间隔影响）
            </span>
          )}
          {job.receipt && (
            <>
              <label>
                <FolderOpen size={14} />
                输出位置
              </label>
              <code>{job.receipt.location}</code>
              <button
                className="button secondary small"
                onClick={() => {
                  void navigator.clipboard
                    .writeText(job.receipt!.location)
                    .then(() => setCopied(true))
                    .catch((error) => onError(errorMessage(error)));
                }}
              >
                <Copy size={13} />
                {copied ? "已复制" : "复制路径"}
              </button>
              {job.receipt.files.map((file) => (
                <div className="receipt-file" key={file.assetId}>
                  <strong>{file.fileName}</strong>
                  <small>SHA-256 · {file.sha256}</small>
                </div>
              ))}
            </>
          )}
          <p>本地归档会生成附件副本及包含标题、正文的 manifest.json。</p>
        </div>
      )}
    </article>
  );
}
