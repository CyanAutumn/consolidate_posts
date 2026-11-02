import {restoreSession} from './session.mjs';
import {PluginError,numericId} from './storage.mjs';
import {setTimeout as sleep} from 'node:timers/promises';
export const CREATOR='https://creator.xiaohongshu.com';
export async function openBrowser(profile,headless,{restore=true}={}){
 process.env.CLOAKBROWSER_AUTO_UPDATE??='false';const {launchPersistentContext}=await import('cloakbrowser');
 const context=await launchPersistentContext({userDataDir:profile,headless,locale:'zh-CN'});
 try{if(restore)await restoreSession(context,profile);context.setDefaultTimeout(30000);context.setDefaultNavigationTimeout(60000);return context;}catch(e){await context.close().catch(()=>{});throw e;}
}
export async function identity(page,context){
 const url=new URL(page.url());if(url.protocol!=="https:"||url.hostname!=="www.xiaohongshu.com")throw new PluginError("xiaohongshu_login","尚未回到小红书账号页面");
 // Only first-party logged-in state identifies the account, never a feed author's link.
 const user=await page.evaluate(()=>{const u=window.__INITIAL_STATE__?.user;const i=u?.userInfo?.value??u?.userInfo;return i&&!i.guest?{userId:i.userId??i.user_id,username:i.nickname}:null;});
 if(!numericId(user?.userId)||!user.username)throw new PluginError('xiaohongshu_login','小红书尚未登录，请在浏览器完成扫码登录');return user;
}
export async function verifyIdentity(page,context,config){
 const probe=await context.newPage();try{await probe.goto('https://www.xiaohongshu.com/explore',{waitUntil:'domcontentloaded'});let actual;
 for(let i=0;i<20;i++){try{actual=await identity(probe,context);break;}catch{await sleep(500);}}
 if(!actual)throw new PluginError('xiaohongshu_login','无法确认小红书登录状态，请重新登录');if(actual.userId!==config.userId)throw new PluginError('xiaohongshu_account','小红书登录账号与配置不一致，已停止发布');
 }finally{await probe.close();}
}
export async function ensureCreator(context){const cookies=await context.cookies(CREATOR);if(!cookies.some(c=>c.name==='galaxy_creator_session_id'&&c.value))throw new PluginError('xiaohongshu_login','创作者中心登录尚未完成，请在登录窗口进入创作者中心');}
export function creationRequest(r){try{const u=new URL(r.url());return r.method()==='POST'&&['edith.xiaohongshu.com','creator.xiaohongshu.com'].includes(u.hostname)&&/^\/web_api\/sns\/v[12]\/note(?:\/publish)?\/?$/.test(u.pathname);}catch{return false;}}
export function receiptFrom(body){const id=body?.data?.id??body?.data?.note_id??body?.data?.noteId;
 if(body?.success===false||(typeof body?.code==='number'&&body.code!==0))throw new PluginError('xiaohongshu_rejected','小红书拒绝发布：'+String(body.msg??body.message??'请检查网页提示').slice(0,500));
 if(!/^[a-fA-F0-9]{24}$/.test(id??'')||!(body.success===true||body.code===0))throw new PluginError('xiaohongshu_uncertain','未取得明确的笔记 ID，请先在创作者中心核对发布结果');
 return {location:`https://www.xiaohongshu.com/explore/${id}`,files:[]};}
export async function composeAndPost(page,request,config,journal,{progress=()=>{},responseTimeout=90000}={}){
 const p=request.publication;const video=p.assets[0].kind==='video';
 await page.goto(`${CREATOR}/publish/publish?source=official&target=${video?'video':'image'}`,{waitUntil:'domcontentloaded'});
 await ensureCreator(page.context());
 const tab=page.getByText(video?'上传视频':'上传图文',{exact:true}).filter({visible:true}).first();if(await tab.count())await tab.click();
 const input=page.locator('input[type=file]').first();await input.waitFor({state:'attached'});
 if(p.assets.length===1||await input.getAttribute('multiple')!==null)await input.setInputFiles(p.assets.map(a=>a.path));
 else for(const [index,asset] of p.assets.entries()){await page.locator('input[type=file]')[index===0?'first':'last']().setInputFiles(asset.path);await page.locator('.img-preview-area .pr').nth(index).waitFor({state:'visible',timeout:60000});}
 await page.locator('input[placeholder*="标题"]').first().fill(p.title);
 await page.locator('div[role="textbox"][contenteditable=true],div.tiptap[contenteditable=true],div.ql-editor[contenteditable=true]').first().fill(p.body);
 const errors=await page.locator('.title-container .length-error,.edit-container .length-error').allTextContents();if(errors.some(e=>e.trim()))throw new PluginError('xiaohongshu_content',errors.join('；'));
 let button;const deadline=Date.now()+300000;
 while(Date.now()<deadline){
 const modern=page.locator('xhs-publish-btn[is-publish="true"]').filter({visible:true}).first();
 if(await modern.count()){if(await modern.getAttribute('submit-disabled')!=='true'){button=modern;break;}}
 else{const old=page.getByRole('button',{name:'发布',exact:true}).filter({visible:true}).last();if(await old.count()&&await old.isEnabled()){button=old;break;}}
 await sleep(500);
 }
 if(!button)throw new PluginError('xiaohongshu_media','小红书附件仍在处理或发布按钮未就绪，尚未提交');
 if(!video){const previews=await page.locator('.img-preview-area .pr').count();if(previews!==p.assets.length)throw new PluginError('xiaohongshu_media','小红书图片预览数量不一致，尚未提交');}
 await verifyIdentity(page,page.context(),config);
 let forwarded=false,blocked=false;
 const handler=async route=>{if(!creationRequest(route.request()))return route.fallback();if(forwarded){blocked=true;return route.abort();}forwarded=true;await route.fallback();};
 await page.route('**/web_api/sns/**',handler);
 try{await journal.submitting();const wait=page.waitForResponse(r=>creationRequest(r.request()),{timeout:responseTimeout});wait.catch(()=>{});await button.click();const response=await wait;
 if([400,401,403,422,429].includes(response.status())){await journal.rejected();throw new PluginError('xiaohongshu_rejected',`小红书拒绝发布（HTTP ${response.status()}），请处理网页提示后重试`);}
 if(!response.ok())throw new PluginError('xiaohongshu_uncertain','小红书提交响应异常，请先核对发布结果');const body=await response.json();
 let receipt;try{receipt=receiptFrom(body);}catch(e){if(e.code==='xiaohongshu_rejected'&&!body?.data?.id&&!body?.data?.note_id&&!body?.data?.noteId)await journal.rejected();throw e;}
 if(blocked)throw new PluginError('xiaohongshu_uncertain','网页尝试重复提交，已拦截；请先核对发布结果');await journal.complete(receipt);await progress(p.assets.reduce((t,a)=>t+a.size,0));return receipt;
 }finally{await page.unroute('**/web_api/sns/**',handler).catch(()=>{});}
}
