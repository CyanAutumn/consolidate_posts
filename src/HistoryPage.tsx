import { useState } from "react";
import {
  Archive,
  CalendarClock,
  CheckCheck,
  Clock3,
  Info,
  LoaderCircle,
  Pause,
  Play,
  Plus,
  ShieldCheck,
  X,
} from "lucide-react";
import type {
  Client,
  Driver,
  HistoryState,
  Job,
  PlanStatus,
  ReplayPreview,
  ReplayRequest,
} from "./types";
import { errorMessage } from "./lib/api";
import { activeJob, formatBytes, statusNames } from "./lib/format";

const dateText = (value: number) =>
  new Date(value).toLocaleString("zh-CN", { hour12: false });

export function HistoryPage({
  client,
  history,
  drivers,
  catalog = drivers,
  jobs,
  initialTarget,
  refresh,
  manageTargets,
}: {
  client: Client;
  history: HistoryState;
  drivers: Driver[];
  catalog?: Driver[];
  jobs: Job[];
  initialTarget: string;
  refresh: () => Promise<void>;
  manageTargets: () => void;
}) {
  const [selected, setSelected] = useState<string[]>([]);
  const [targetId, setTargetId] = useState(
    (history.targets.some(
      (target) =>
        target.id === initialTarget &&
        drivers.some(
          (driver) =>
            driver.id === target.driverId && driver.status === "ready",
        ),
    )
      ? initialTarget
      : "") ||
      history.targets.find((target) =>
        drivers.some(
          (driver) =>
            driver.id === target.driverId && driver.status === "ready",
        ),
      )?.id ||
      "",
  );
  const [mode, setMode] = useState("now");
  const [date, setDate] = useState("");
  const [interval, setInterval] = useState("30");
  const [force, setForce] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [preview, setPreview] = useState<{
    request: ReplayRequest;
    result: ReplayPreview;
    key: string;
  } | null>(null);
  const ids = selected.filter((id) =>
    history.archives.some((archive) => archive.id === id),
  );
  const key = JSON.stringify([ids, targetId, mode, date, interval, force]);
  const targetLabel = (id: string) => {
    const target = history.targets.find((target) => target.id === id);
    return `${catalog.find((driver) => driver.id === target?.driverId)?.name ?? target?.driverId ?? "未知平台"}`;
  };
  const run = async (action: () => Promise<void>) => {
    if (pending) return;
    setPending(true);
    setError("");
    setNotice("");
    try {
      await action();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setPending(false);
    }
  };
  const check = () =>
    run(async () => {
      const minutes = Number(interval);
      const startAt = mode === "now" ? Date.now() : new Date(date).getTime();
      if (
        !interval.trim() ||
        !Number.isInteger(minutes) ||
        minutes < 0 ||
        minutes > 525600
      )
        throw new Error("请输入 0–525600 的整数分钟间隔");
      if (
        !Number.isFinite(startAt) ||
        (mode !== "now" && startAt <= Date.now())
      )
        throw new Error("请选择未来的开始时间");
      const request = {
        archiveIds: ids,
        targetId,
        startAt,
        intervalMinutes: minutes,
        force,
      };
      const result = await client.previewReplay(request);
      setPreview({ request, result, key });
    });
  const submit = () =>
    run(async () => {
      if (!preview || preview.key !== key) return;
      const result = await client.scheduleReplay(preview.request);
      setPreview(null);
      await refresh();
      setNotice(
        `已创建补传计划：${result.preview.eligibleCount} 条待执行，${result.preview.skippedCount} 条跳过。`,
      );
    });
  const control = (id: string, status: PlanStatus) =>
    run(async () => {
      await client.setPlanStatus(id, status);
      await refresh();
    });

  return (
    <div className="history-page">
      <div className="info-strip">
        <Archive size={20} />
        <span>
          <strong>每次发布自动留档</strong> ·
          保存文件副本、标题、正文和校验值。原文件移动或删除后，仍可从历史补传。
        </span>
      </div>
      {error && (
        <div className="feedback error" role="alert">
          {error}
        </div>
      )}
      {notice && (
        <div className="feedback notice" role="status">
          <CheckCheck size={17} />
          {notice}
        </div>
      )}
      <div className="history-grid">
        <section className="panel archive-library">
          <div className="section-heading">
            <div>
              <Archive size={18} />
              <h2>历史归档</h2>
              <span className="count-badge">{history.archives.length}</span>
            </div>
            <label className="inline-check">
              <input
                aria-label="全选全部历史"
                type="checkbox"
                checked={
                  ids.length > 0 && ids.length === history.archives.length
                }
                disabled={!history.archives.length || pending}
                onChange={(event) =>
                  setSelected(
                    event.target.checked
                      ? history.archives.map((archive) => archive.id)
                      : [],
                  )
                }
              />
              全选全部历史
            </label>
          </div>
          {!history.archives.length ? (
            <div className="empty-state">
              <Archive size={30} />
              <h3>先保存你的第一份创作</h3>
              <p>在发布工作台点击上传或“仅保存归档”，完整内容会出现在这里。</p>
            </div>
          ) : (
            <div className="history-items">
              {history.archives.map((archive) => {
                const title =
                  archive.publication.title ||
                  archive.publication.assets[0]?.name ||
                  "文字归档";
                const lastJob = jobs.find(
                  (job) =>
                    job.kind === "upload" &&
                    job.archiveId === archive.id &&
                    (job.targetId === targetId || (jobs.length > 0 && ["pixiv", "x"].includes(job.driverId) && job.driverId === history.targets.find(t => t.id === targetId)?.driverId)),
                );
                return (
                  <article className="history-item" key={archive.id}>
                    <input
                      aria-label={`选择归档 ${title}`}
                      type="checkbox"
                      checked={ids.includes(archive.id)}
                      disabled={pending}
                      onChange={(event) =>
                        setSelected((previous) =>
                          event.target.checked
                            ? [...previous, archive.id]
                            : previous.filter((id) => id !== archive.id),
                        )
                      }
                    />
                    <details>
                      <summary>
                        <div>
                          <strong>{title}</strong>
                          <small>
                            {dateText(archive.createdAt)} ·{" "}
                            {archive.publication.assets.length} 个附件 ·{" "}
                            {formatBytes(
                              archive.publication.assets.reduce(
                                (sum, asset) => sum + asset.size,
                                0,
                              ),
                            )}
                          </small>
                        </div>
                        <span
                          className={`pill ${lastJob ? `status-${lastJob.status}` : ""}`}
                        >
                          {lastJob
                            ? statusNames[lastJob.status]
                            : "该平台未上传"}
                        </span>
                      </summary>
                      <div className="history-content">
                        {archive.publication.body && (
                          <pre>{archive.publication.body}</pre>
                        )}
                        {archive.publication.assets.map((asset) => (
                          <div className="history-file" key={asset.id}>
                            <span>{asset.name}</span>
                            <small>{formatBytes(asset.size)}</small>
                          </div>
                        ))}
                        <label>本地归档路径</label>
                        <code>{archive.receipt.location}</code>
                        <small>归档 ID：{archive.id}</small>
                      </div>
                    </details>
                  </article>
                );
              })}
            </div>
          )}
        </section>
        <section className="panel replay-composer">
          <div className="section-heading">
            <div>
              <CalendarClock size={18} />
              <h2>补传到平台</h2>
            </div>
          </div>
          <p className="section-description">
            已选 {ids.length} 条历史，将按归档时间从旧到新执行。
          </p>
          <label htmlFor="replay-target">发布平台</label>
          <select
            id="replay-target"
            value={targetId}
            disabled={pending}
            onChange={(event) => setTargetId(event.target.value)}
          >
            <option value="">请选择平台</option>
            {history.targets
              .filter((target) =>
                drivers.some((driver) => driver.id === target.driverId),
              )
              .map((target) => (
                <option
                  key={target.id}
                  value={target.id}
                  disabled={
                    !drivers.some(
                      (driver) =>
                        driver.id === target.driverId &&
                        driver.status === "ready",
                    )
                  }
                >
                  {targetLabel(target.id)}
                  {drivers.find((driver) => driver.id === target.driverId)
                    ?.status !== "ready"
                    ? " · 待接入"
                    : ""}
                </option>
              ))}
          </select>
          <button className="add-more" onClick={manageTargets}>
            <Plus size={14} />
            管理平台登录
          </button>
          <label htmlFor="replay-mode">开始方式</label>
          <select
            id="replay-mode"
            value={mode}
            disabled={pending}
            onChange={(event) => setMode(event.target.value)}
          >
            <option value="now">立即开始</option>
            <option value="scheduled">指定时间开始</option>
          </select>
          {mode === "scheduled" && (
            <>
              <label htmlFor="replay-date">开始时间（本机时区）</label>
              <input
                id="replay-date"
                type="datetime-local"
                value={date}
                disabled={pending}
                onChange={(event) => setDate(event.target.value)}
              />
              <small className="field-hint">
                {Intl.DateTimeFormat().resolvedOptions().timeZone}
              </small>
            </>
          )}
          <label htmlFor="replay-interval">每条历史的发布间隔（分钟）</label>
          <input
            id="replay-interval"
            type="number"
            min={0}
            max={525600}
            step={1}
            value={interval}
            disabled={pending}
            onChange={(event) => setInterval(event.target.value)}
          />
          <small className="field-hint">
            0 = 连续执行，60 = 每小时一条，1440 = 每天一条。
          </small>
          <label className="inline-check force-replay">
            <input
              type="checkbox"
              checked={force}
              disabled={pending}
              onChange={(event) => setForce(event.target.checked)}
            />
            也重新上传已成功的历史
          </label>
          <p className="field-hint">
            默认跳过该平台已成功或正在排队的内容。不兼容或缺失的归档会列出原因。
          </p>
          <div className="replay-buttons">
            <button
              className="button secondary"
              disabled={!client.desktop || !ids.length || !targetId || pending}
              onClick={() => void check()}
            >
              <ShieldCheck size={16} />
              检查补传计划
            </button>
            <button
              className="button primary"
              disabled={
                !client.desktop ||
                pending ||
                !preview ||
                preview.key !== key ||
                !preview.result.eligibleCount
              }
              onClick={() => void submit()}
            >
              {pending ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <CalendarClock size={16} />
              )}
              确认创建计划
            </button>
          </div>
          <div className="driver-note">
            <Info size={15} />
            <span>
              应用开启时执行。关闭后计划保留，重启后按间隔继续；不会自动唤醒电脑。可用目标以已加载插件为准。
            </span>
          </div>
        </section>
      </div>
      {preview && preview.key === key && (
        <section className="panel replay-preview">
          <div className="section-heading">
            <div>
              <ShieldCheck size={18} />
              <h2>执行前检查</h2>
            </div>
            <span>
              {preview.result.eligibleCount} 条可补传 ·{" "}
              {preview.result.skippedCount} 条跳过
            </span>
          </div>
          {preview.result.items.map((item) => (
            <div key={item.archiveId} className="replay-preview-row">
              <strong>{item.title || "无标题归档"}</strong>
              <span className={item.eligible ? "green" : "red"}>
                {item.eligible
                  ? `预计 ${dateText(item.scheduledAt!)}`
                  : item.reason}
              </span>
            </div>
          ))}
          <p className="field-hint">
            确认时会再次检查上传记录；应用离线、计划暂停或队列繁忙时，后续时间按实际执行顺延。
          </p>
        </section>
      )}
      <section className="panel replay-plans">
        <div className="section-heading">
          <div>
            <Clock3 size={18} />
            <h2>补传计划</h2>
          </div>
          <span className="subtle">暂停只影响未开始的任务</span>
        </div>
        {!history.plans.length ? (
          <p className="section-description">
            创建计划后，可在这里暂停、继续或取消；单条任务结果在“上传任务”查看。
          </p>
        ) : (
          history.plans.map((plan) => {
            const related = jobs.filter((job) => job.planId === plan.id);
            const waiting = related.filter((job) =>
              activeJob(job.status),
            ).length;
            const failed = related.filter(
              (job) => job.status === "failed" || job.status === "interrupted",
            ).length;
            const succeeded = related.filter(
              (job) => job.status === "succeeded",
            ).length;
            return (
              <article className="plan-row" key={plan.id}>
                <div>
                  <strong>{targetLabel(plan.targetId)}</strong>
                  <small>
                    {dateText(plan.startAt)} 开始 ·{" "}
                    {plan.intervalMinutes
                      ? `每 ${plan.intervalMinutes} 分钟一条`
                      : "连续执行"}{" "}
                    · 共 {plan.archiveIds.length} 条
                  </small>
                  <small>
                    完成 {succeeded} · 待完成 {waiting} · 失败 / 中断 {failed}
                  </small>
                </div>
                <span className="pill">
                  {plan.status === "cancelled"
                    ? "已取消"
                    : waiting === 0
                      ? failed
                        ? "已结束，有失败"
                        : "已结束"
                      : plan.status === "paused"
                        ? "已暂停"
                        : "执行计划中"}
                </span>
                <div className="plan-actions">
                  {plan.status !== "cancelled" && waiting > 0 && (
                    <>
                      <button
                        className="button secondary small"
                        disabled={pending || !client.desktop}
                        onClick={() =>
                          void control(
                            plan.id,
                            plan.status === "paused" ? "active" : "paused",
                          )
                        }
                      >
                        {plan.status === "paused" ? (
                          <Play size={13} />
                        ) : (
                          <Pause size={13} />
                        )}
                        {plan.status === "paused" ? "继续" : "暂停"}
                      </button>
                      <button
                        className="button secondary small"
                        disabled={pending || !client.desktop}
                        onClick={() => void control(plan.id, "cancelled")}
                      >
                        <X size={13} />
                        取消计划
                      </button>
                    </>
                  )}
                </div>
              </article>
            );
          })
        )}
      </section>
    </div>
  );
}
