import {restoreSession} from './session.mjs';import {PluginError,numericId} from './storage.mjs';import {setTimeout as sleep} from 'node:timers/promises';
export const CREATOR='https://member.bilibili.com';
export async function openBrowser(profile,headless,{restore=true}={}){process.env.CLOAKBROWSER_AUTO_UPDATE??='false';const {launchPersistentContext}=await import('cloakbrowser');const c=await launchPersistentContext({userDataDir:profile,headless,locale:'zh-CN'});try{if(restore)await restoreSession(c,profile);c.setDefaultTimeout(30000);c.setDefaultNavigationTimeout(60000);return c;}catch(e){await c.close().catch(()=>{});throw e;}}
export async function identity(page,context){const u=new URL(page.url());if(u.protocol!=='https:'||!['www.bilibili.com','member.bilibili.com'].includes(u.hostname))throw new PluginError('bilibili_login','请在浏览器完成哔哩哔哩登录');
 const result=await page.evaluate(async()=>{try{const r=await fetch('https://api.bilibili.com/x/web-interface/nav',{credentials:'include'});const j=await r.json();return j.code===0&&j.data?.isLogin?{userId:String(j.data.mid),username:j.data.uname}:null;}catch{return null;}});
 if(!numericId(result?.userId)||!result.username)throw new PluginError('bilibili_login','哔哩哔哩登录尚未完成或已失效');return result;}
export async function verifyIdentity(page,context,config){const p=await context.newPage();try{await p.goto('https://www.bilibili.com/',{waitUntil:'domcontentloaded'});const actual=await identity(p,context);if(actual.userId!==config.userId)throw new PluginError('bilibili_account','哔哩哔哩账号与保存配置不一致，已停止投稿');}finally{await p.close();}}
export async function ensureCreator(context){const cookies=await context.cookies(CREATOR);if(!cookies.some(c=>c.name==='SESSDATA'&&c.value)||!cookies.some(c=>c.name==='bili_jct'&&c.value))throw new PluginError('bilibili_login','创作中心登录凭据不完整，请重新登录');}
export function creationRequest(r){try{const u=new URL(r.url());return r.method()==='POST'&&u.hostname==='member.bilibili.com'&&/^\/x\/vu\/(web|client)\/add(?:\/v[0-9]+)?\/?$/.test(u.pathname);}catch{return false;}}
export function receiptFrom(body){const bv=body?.data?.bvid;const aid=body?.data?.aid;
 if(typeof body?.code==='number'&&body.code!==0)throw new PluginError('bilibili_rejected','哔哩哔哩拒绝投稿：'+String(body.message??body.msg??'请检查网页提示').slice(0,500));
 if(body?.code!==0)throw new PluginError('bilibili_uncertain','未取得明确投稿回执，请先在稿件管理核对');
 const id=/^BV[A-Za-z0-9]{10}$/.test(bv??'')?bv:(Number.isSafeInteger(aid)&&aid>0?'av'+aid:(/^[1-9][0-9]{0,15}$/.test(aid??'')?'av'+aid:null));
 if(!id)throw new PluginError('bilibili_uncertain','未取得有效 BV/AV 号，请先在稿件管理核对');return {location:`https://www.bilibili.com/video/${id}`,files:[]};}
async function surface(page){const deadline=Date.now()+60000;while(Date.now()<deadline){for(const f of page.frames()){try{const u=new URL(f.url());if(u.hostname==='member.bilibili.com'&&await f.locator('input[type=file]').count())return f;}catch{}}await sleep(300);}throw new PluginError('bilibili_layout','找不到哔哩哔哩视频上传表单');}
export async function composeAndPost(page,request,config,journal,{progress=()=>{},responseTimeout=90000}={}){
 const p=request.publication;await page.goto(`${CREATOR}/platform/upload/video/frame`,{waitUntil:'domcontentloaded'});await ensureCreator(page.context());const form=await surface(page);
 await form.locator('input[type=file]').first().setInputFiles(p.assets[0].path);
 await form.locator('.upload-success').or(form.getByText('上传完成',{exact:false})).or(form.getByText('重新上传',{exact:true})).first().waitFor({state:'visible',timeout:1800000});
 await form.locator('.video-title input,input[maxlength="80"],input[placeholder*="标题"]').first().fill(p.title);
 await form.locator('textarea[placeholder*="简介"],.ql-editor[contenteditable=true],div[contenteditable=true]').first().fill(p.body);
 await form.getByText(config.copyright===1?'自制':'转载',{exact:true}).filter({visible:true}).first().click();
 if(config.copyright===2)await form.locator('input[placeholder*="来源"],textarea[placeholder*="来源"]').first().fill(config.source);
 const category=form.locator('.type-container .select-item-cont,.video-type .select-item-cont,.select-item-cont').filter({visible:true}).first();await category.click();
 for(const label of config.category.split(/[/>]/).map(t=>t.trim()).filter(Boolean))await form.getByText(label,{exact:true}).filter({visible:true}).last().click();
 const tagInput=form.locator('.tag-container input,input[placeholder*="标签"],input[placeholder*="Tag"]').first();
 // The final request must match the user's explicit tags; unexpected defaults stop submission.
 for(const tag of config.tags){await tagInput.fill(tag);await tagInput.press('Enter');}
 await verifyIdentity(page,page.context(),config);const button=form.getByRole('button',{name:'立即投稿',exact:true}).or(form.getByRole('button',{name:'投稿',exact:true})).or(form.locator('.submit-add')).filter({visible:true}).last();
 await button.waitFor({state:'visible'});if(!await button.isEnabled())throw new PluginError('bilibili_media','投稿按钮未就绪，请检查封面、分区、标签或网页校验提示');
 let forwarded=false,blocked=false;const handler=async route=>{
 if(!creationRequest(route.request()))return route.fallback();let data;try{data=route.request().postDataJSON();}catch{}
 const tags=typeof data?.tag==='string'?data.tag.split(',').map(t=>t.trim()):[];
 if(forwarded||data?.title!==p.title||String(data?.desc??'')!==p.body||Number(data?.copyright)!==config.copyright||!Number.isInteger(Number(data?.tid))||Number(data.tid)<=0||!Array.isArray(data?.videos)||data.videos.length!==1||!data.videos[0]?.filename||tags.length!==config.tags.length||config.tags.some(t=>!tags.includes(t))||(config.copyright===2&&data.source!==config.source)){blocked=true;return route.abort();}
 forwarded=true;await route.fallback();};await page.route('**/x/vu/**',handler);
 try{await journal.submitting();const wait=page.waitForResponse(r=>creationRequest(r.request()),{timeout:responseTimeout});wait.catch(()=>{});await button.click();const response=await wait;
 if([400,401,403,422,429].includes(response.status())){await journal.rejected();throw new PluginError('bilibili_rejected',`哔哩哔哩拒绝投稿（HTTP ${response.status()}），请处理网页提示后重试`);}
 if(!response.ok())throw new PluginError('bilibili_uncertain','投稿响应异常，请先核对稿件状态');const body=await response.json();let receipt;
 try{receipt=receiptFrom(body);}catch(e){if(e.code==='bilibili_rejected'&&!body?.data?.bvid&&!body?.data?.aid)await journal.rejected();throw e;}
 if(blocked)throw new PluginError('bilibili_uncertain','已阻止重复投稿，请先核对稿件状态');await journal.complete(receipt);await progress(p.assets[0].size);return receipt;
 }catch(e){if(blocked&&!forwarded){await journal.rejected();throw new PluginError('bilibili_layout','最终请求与标题、简介、自制/转载或标签设置不一致，已拦截投稿，请检查网页结构或投稿设置');}throw e;
 }finally{await page.unroute('**/x/vu/**',handler).catch(()=>{});}
}
