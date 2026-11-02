import {useEffect,useState} from 'react';
import type {Client,BilibiliPosting} from './types';import {errorMessage} from './lib/api';
export function BilibiliAccountHelp({client,targetId,connected,onLoggedIn}:{client:Client;targetId:string;connected:boolean;onLoggedIn:()=>Promise<void>}){
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const [settings,setSettings]=useState<BilibiliPosting>({category:'',tags:[],copyright:0,source:''});const [tags,setTags]=useState('');
 useEffect(()=>{let current=true;if(client.desktop)void client.bilibiliSettings(targetId).then(s=>{if(current){setSettings(s);setTags(s.tags.join(', '));}}).catch(e=>{if(current)setError(errorMessage(e));});return()=>{current=false;};},[client,targetId]);
 const run=async(action:()=>Promise<void>,message:string)=>{if(busy)return;setBusy(true);setError('');setNotice('');try{await action();setNotice(message);}catch(e){setError(errorMessage(e));}finally{setBusy(false);}};
 return <div className="bilibili-account-help">
 <button className="button secondary small" disabled={!client.desktop||busy} onClick={()=>void run(async()=>{await client.loginBilibili(targetId);await onLoggedIn();},'哔哩哔哩登录成功')}>{busy?'处理中…':connected?'重新登录哔哩哔哩':'登录哔哩哔哩'}</button>
 <details><summary>投稿设置</summary><p>发布单个 MP4 视频。请按网页分区名称填写路径；设置可单独保存，无需重新登录。</p>
 <label>分区路径<input aria-label="B站分区路径" value={settings.category} placeholder="父分区/子分区" onChange={e=>setSettings({...settings,category:e.target.value})}/></label>
 <label>默认标签<input aria-label="B站标签" value={tags} placeholder="用逗号分隔，最多10个" onChange={e=>setTags(e.target.value)}/></label>
 <label>创作类型<select aria-label="B站创作类型" value={settings.copyright} onChange={e=>setSettings({...settings,copyright:Number(e.target.value)})}><option value={0}>请选择</option><option value={1}>自制</option><option value={2}>转载</option></select></label>
 {settings.copyright===2&&<label>转载来源<input aria-label="B站转载来源" value={settings.source} onChange={e=>setSettings({...settings,source:e.target.value})}/></label>}
 <button className="button secondary small" disabled={!client.desktop||busy} onClick={()=>void run(()=>client.saveBilibiliSettings(targetId,{...settings,category:settings.category.trim(),tags:[...new Set(tags.split(/[,，]/).map(t=>t.trim()).filter(Boolean))]}),'哔哩哔哩投稿设置已保存')}>保存B站投稿设置</button>
 </details>{notice&&<p role="status">{notice}</p>}{error&&<p role="alert">{error}</p>}
 </div>;
}
