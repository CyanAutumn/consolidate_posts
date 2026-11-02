import { BilibiliAccountHelp } from "./BilibiliAccountHelp";
import { XiaohongshuAccountHelp } from "./XiaohongshuAccountHelp";
import { ArrowRight } from "lucide-react";
import type { Client, Driver, UploadTarget, PlatformAccount } from "./types";
import { PixivAccountHelp } from "./PixivAccountHelp";
import { XAccountHelp } from "./XAccountHelp";
export function TargetManager({client,drivers,targets,accounts,enabledDriverIds,refresh,replay,dataDir,pluginDir}: {
  enabledDriverIds: string[]; client: Client; drivers: Driver[]; targets: UploadTarget[]; accounts: PlatformAccount[];
  refresh: () => Promise<void>; replay: (id:string) => void; dataDir:string; pluginDir:string;
}) {
  return <section className="panel target-manager">
    <div className="section-heading"><h2>平台登录</h2></div>
    <p className="section-description">每个平台登录一个账号，登录后即可发布。</p>
    <div className="target-list">{drivers.map(driver => {
      const target = targets.find(t => t.driverId === driver.id);
      if (!target) return null;
      const enabled = enabledDriverIds.includes(driver.id);
      const connected = accounts.find(a => a.driverId === driver.id)?.connected ?? driver.id === "local-archive";
      return <div className="target-row" key={driver.id}><div>
        <strong>{driver.name}</strong>
        <small>{driver.id === "local-archive" ? "本地保存，无需登录" : driver.status !== "ready" ? "尚未接入" : connected ? "已登录" : "未登录"}</small>
        {!enabled && <small>当前工作区未启用，可先登录；发布前请在“工作区管理”启用此平台。</small>}
        {driver.id === "pixiv" && <PixivAccountHelp key={`${dataDir}/${target.id}`} client={client} targetId={target.id} dataDir={dataDir} pluginDir={pluginDir} connected={connected} onLoggedIn={refresh} />}
        {driver.id === "bilibili" && <BilibiliAccountHelp key={`${dataDir}/${target.id}`} client={client} targetId={target.id} connected={connected} onLoggedIn={refresh} />}
        {driver.id === "xiaohongshu" && <XiaohongshuAccountHelp key={`${dataDir}/${target.id}`} client={client} targetId={target.id} connected={connected} onLoggedIn={refresh} />}
        {driver.id === "x" && <XAccountHelp key={`${dataDir}/${target.id}`} client={client} targetId={target.id} connected={connected} onLoggedIn={refresh} />}
      </div><button className="button secondary small" disabled={driver.status !== "ready" || !enabled} onClick={() => replay(target.id)}>补传历史<ArrowRight size={14}/></button></div>;
    })}</div>
  </section>;
}