import { useEffect, useRef, useState } from "react";
import type { Client, PixivSettings } from "./types";
import { errorMessage } from "./lib/api";

export function PixivAccountHelp({ client, targetId, connected = false, onLoggedIn }: {
  client: Client; targetId: string; dataDir: string; pluginDir: string; connected?: boolean; onLoggedIn?: () => Promise<void>;
}) {
  const loaded = useRef(false);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [tags, setTags] = useState("");
  const [settings, setSettings] = useState<PixivSettings>({
    userId: "", tags: [], aiType: "", visibility: "private", rating: null,
    original: false, allowTagEdit: false, allowComment: true,
  });
  const load = async () => {
    if (loaded.current || !client.desktop) return;
    loaded.current = true; setLoading(true); setError("");
    try {
      const saved = await client.pixivSettings(targetId);
      setSettings(saved); setTags(saved.tags.join(", "));
    } catch (e) { setError(errorMessage(e)); loaded.current = false; }
    finally { setLoading(false); }
  };
  const login = async () => {
    if (busy || loading || !client.desktop) return;
    setBusy(true); setError(""); setNotice("正在打开浏览器，请在窗口中登录 Pixiv 并完成验证。首次启动可能需要下载浏览器组件。");
    try {
      const next = { ...settings, tags: [...new Set(tags.split(/[,，]/).map(t => t.trim()).filter(Boolean))] };
      await client.loginPixiv(targetId, next);
      setNotice("Pixiv 登录成功，账号设置和加密凭据已保存，可以上传或重试失败任务。");
      await onLoggedIn?.().catch(()=>{});
    } catch (e) { setNotice(""); setError(errorMessage(e)); }
    finally { setBusy(false); }
  };
  useEffect(() => { void load(); }, [client, targetId]);
  const save = async () => {
    setBusy(true); setError(""); setNotice("");
    try {
      const next = { ...settings, tags: [...new Set(tags.split(/[,，]/).map(t=>t.trim()).filter(Boolean))] };
      setSettings(await client.savePixivSettings(targetId, next)); setNotice("投稿设置已保存，无需重新登录。");
    } catch(e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };
  return (
    <div className="pixiv-account-help">
      <details>
      <summary>投稿设置</summary>
      <p>发布前请填写标签和 AI 标记，已有设置自动保留。</p>
      <fieldset disabled={busy || loading || !client.desktop} className="pixiv-login-fields">
        <label>默认投稿标签（英文或中文逗号分隔）<input aria-label="Pixiv 默认标签" value={tags} onChange={e => setTags(e.target.value)} placeholder="例如：オリジナル, イラスト" /></label>
        <label>AI 标记<select aria-label="Pixiv AI 标记" value={settings.aiType} onChange={e => setSettings({ ...settings, aiType: e.target.value })}>
          <option value="">请选择</option><option value="notAiGenerated">非 AI 生成</option><option value="aiGenerated">AI 生成</option>
        </select></label>
        <label>可见性<select aria-label="Pixiv 可见性" value={settings.visibility} onChange={e => setSettings({ ...settings, visibility: e.target.value })}>
          <option value="private">仅自己可见</option><option value="public">公开</option><option value="mypixiv">好友可见</option>
        </select></label>
        <label>年龄限制<select aria-label="Pixiv 年龄限制" value={settings.rating ?? "workspace"} onChange={e => setSettings({ ...settings, rating: e.target.value === "workspace" ? null : e.target.value })}>
          <option value="workspace">跟随工作区</option><option value="general">全年龄</option><option value="r18">R18</option>
        </select></label>
        <label><input type="checkbox" checked={settings.original} onChange={e => setSettings({ ...settings, original: e.target.checked })} />原创作品</label>
        <button className="button secondary small" onClick={()=>void save()} disabled={!tags.trim() || !settings.aiType}>保存投稿设置</button>
      </fieldset>
      </details>
      <button className="button secondary small" disabled={!client.desktop || busy || loading} onClick={() => void login()}>{busy ? "等待浏览器登录…" : loading ? "读取账号设置…" : connected ? "重新登录 Pixiv" : "登录 Pixiv"}</button>
      {notice && <p role="status">{notice}</p>}
      {error && <p role="alert" className="feedback error">{error}</p>}
      {!client.desktop && <p>请在桌面版中登录。</p>}
      <p>默认仅自己可见。登录只需在浏览器完成验证，之后无需保持浏览器开启。</p>
    </div>
  );
}
