import { useState } from "react";
import type { Client } from "./types";
import { errorMessage } from "./lib/api";
export function XAccountHelp({client,targetId,connected,onLoggedIn}: {client:Client;targetId:string;connected:boolean;onLoggedIn:()=>Promise<void>}) {
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState("");
  const [notice,setNotice] = useState("");
  const login = async () => {
    if(busy) return;
    setBusy(true);setError("");setNotice("请在打开的浏览器中登录 X。");
    try {await client.loginX(targetId);setNotice("X 登录成功");await onLoggedIn().catch(()=>{});}
    catch(e){setError(errorMessage(e));setNotice("");}finally{setBusy(false);}
  };
  return <div className="pixiv-account-help x-account-help">
    <button className="button secondary small" disabled={!client.desktop || busy} onClick={()=>void login()}>{busy?"等待浏览器登录…":connected?"重新登录 X":"登录 X"}</button>
    {notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}