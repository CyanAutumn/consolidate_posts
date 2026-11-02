import { useState } from "react";
import type { Client } from "./types";
import { errorMessage } from "./lib/api";
export function XiaohongshuAccountHelp({client,targetId,connected,onLoggedIn}: {client:Client;targetId:string;connected:boolean;onLoggedIn:()=>Promise<void>}) {
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState("");
  const [notice,setNotice] = useState("");
  const login = async () => {
    if(busy) return;
    setBusy(true);setError("");setNotice("请在打开的浏览器中登录小红书。");
    try {await client.loginXiaohongshu(targetId);setNotice("小红书登录成功");await onLoggedIn().catch(()=>{});}
    catch(e){setError(errorMessage(e));setNotice("");}finally{setBusy(false);}
  };
  return <div className="pixiv-account-help xiaohongshu-account-help">
    <button className="button secondary small" disabled={!client.desktop || busy} onClick={()=>void login()}>{busy?"等待浏览器登录…":connected?"重新登录小红书":"登录小红书"}</button>
    {notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}