import { useState } from "react";
import { CheckCheck, Layers3, Plus, Save } from "lucide-react";
import type {
  ContentRating,
  Driver,
  WorkspaceConfig,
  WorkspaceProfile,
} from "./types";

export const ratingNames: Record<ContentRating, string> = {
  unspecified: "未分类",
  all_ages: "全年龄",
  r18: "R18",
};

export function WorkspaceManager({
  profile,
  drivers,
  creating,
  busy,
  desktop,
  create,
  update,
  setCreating,
}: {
  profile: WorkspaceProfile;
  drivers: Driver[];
  creating: boolean;
  busy: boolean;
  desktop: boolean;
  create: (config: WorkspaceConfig) => Promise<void>;
  update: (config: WorkspaceConfig) => Promise<void>;
  setCreating: (value: boolean) => void;
}) {
  const [name, setName] = useState(creating ? "" : profile.name);
  const [rating, setRating] = useState<ContentRating>(
    creating ? "all_ages" : profile.contentRating,
  );
  const [enabled, setEnabled] = useState<string[]>(
    creating ? [] : profile.enabledDriverIds,
  );
  const missing = enabled.filter(
    (id) => !drivers.some((driver) => driver.id === id),
  );
  const toggle = (id: string) =>
    setEnabled((previous) =>
      previous.includes(id)
        ? previous.filter((value) => value !== id)
        : [...previous, id],
    );
  return (
    <section className="panel workspace-manager">
      <div className="section-heading">
        <div>
          <Layers3 size={20} />
          <h2>{creating ? "创建工作区" : "当前工作区设置"}</h2>
        </div>
        <button
          className="button secondary small"
          disabled={busy}
          onClick={() => setCreating(!creating)}
        >
          {creating ? (
            "返回当前工作区"
          ) : (
            <>
              <Plus size={15} />
              新建工作区
            </>
          )}
        </button>
      </div>
      <p className="section-description">
        例如「R18」只启用指定平台，「全年龄」启用所有需要的平台。每个工作区独立保存草稿、账号、归档、上传记录和定时计划。
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void (creating ? create : update)({
            name,
            contentRating: rating,
            enabledDriverIds: enabled,
          });
        }}
      >
        <fieldset disabled={busy || !desktop}>
          <div className="workspace-fields">
            <label>
              工作区名称
              <input
                aria-label="工作区名称"
                required
                maxLength={80}
                value={name}
                placeholder="例如：R18 创作、全年龄作品"
                onChange={(event) => setName(event.target.value)}
              />
            </label>
            <label>
              内容分类
              <select
                aria-label="工作区内容分类"
                value={rating}
                onChange={(event) =>
                  setRating(event.target.value as ContentRating)
                }
              >
                {Object.entries(ratingNames).map(([id, label]) => (
                  <option key={id} value={id}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div className="workspace-plugin-heading">
            <strong>启用插件 · {enabled.length} 个</strong>
            <div>
              <button
                type="button"
                className="text-button"
                onClick={() =>
                  setEnabled([
                    ...new Set([
                      ...drivers.map((driver) => driver.id),
                      ...missing,
                    ]),
                  ])
                }
              >
                <CheckCheck size={14} />
                全选插件
              </button>
              <button
                type="button"
                className="text-button"
                onClick={() => setEnabled([])}
              >
                清空
              </button>
            </div>
          </div>
          <div className="workspace-plugin-list">
            {drivers.map((driver) => (
              <label
                key={driver.id}
                className={enabled.includes(driver.id) ? "enabled" : ""}
              >
                <input
                  type="checkbox"
                  aria-label={`工作区启用 ${driver.name}`}
                  checked={enabled.includes(driver.id)}
                  onChange={() => toggle(driver.id)}
                />
                <span>
                  <strong>{driver.name}</strong>
                  <small>
                    {driver.status === "ready"
                      ? "可用"
                      : driver.status === "planned"
                        ? "计划接入"
                        : "加载失败"}
                  </small>
                </span>
              </label>
            ))}
            {missing.map((id) => (
              <label key={id}>
                <input
                  type="checkbox"
                  aria-label={`工作区启用 ${id}`}
                  checked
                  onChange={() => toggle(id)}
                />
                <span>
                  <strong>{id}</strong>
                  <small>插件当前未安装，保留配置</small>
                </span>
              </label>
            ))}
          </div>
          <p className="workspace-hint">
            本地留档始终执行。这里的「本地导出」是可选的额外副本。内容分类作为工作区标识；素材由你选入当前工作区。
          </p>
          <div className="workspace-form-footer">
            <span>新安装的插件需要在此勾选。停用插件会保留历史记录。</span>
            <button className="button primary" disabled={!name.trim()}>
              {creating ? <Plus size={16} /> : <Save size={16} />}
              {creating ? "创建并进入" : "保存工作区设置"}
            </button>
          </div>
        </fieldset>
      </form>
    </section>
  );
}
