import { openLoginContext, loggedInUser } from './browser-login.mjs';
import { pathToFileURL } from 'node:url';
export class SubmitError extends Error {
 constructor(code,message){super(message);this.code=code;}
}
export async function submit(p,{launch=openLoginContext}={}) {
 let clicked=false;const context=await launch({headless:true});
 try {
  await context.addCookies([{name:'PHPSESSID',value:p.session,domain:'.pixiv.net',path:'/',secure:true,httpOnly:true}]);
  const page=await context.newPage();
  await page.goto('https://www.pixiv.net/illustration/create',{waitUntil:'domcontentloaded'});
  if(await loggedInUser(page)!==p.config.userId)throw new SubmitError('pixiv_not_submitted','浏览器登录状态不符，请重新登录 Pixiv');
  await page.locator('input[type=file]').setInputFiles(p.publication.assets.map(a=>a.path));
  await page.locator('input[name=title]').fill(p.publication.title);
  await page.locator('textarea[name=comment]').fill(p.publication.body);
  const tags=page.getByPlaceholder('标签',{exact:true});
  for(const tag of p.config.tags){await tags.fill(tag);await tags.press('Enter');}
  for(const [name,value] of [['x_restrict',p.rating],['ai_type',p.config.aiType],['restrict',p.config.visibility],['allow_comment',String(p.config.allowComment)]])
   await page.locator(`input[name="${name}"][value="${value}"]`).check();
  await page.locator('input[name=original]').setChecked(p.config.original);
  await page.locator('input[name=allow_tag_edit]').setChecked(p.config.allowTagEdit);
  if(await page.locator('input[name=sexual][value=false]').count())await page.locator('input[name=sexual][value=false]').check();
  const button=page.getByRole('button',{name:'投稿',exact:true}).last();
  await button.waitFor({state:'visible'});
  const deadline=Date.now()+120000;
  while(!await button.isEnabled()){
   if(Date.now()>deadline)throw new SubmitError('pixiv_not_submitted','Pixiv 投稿按钮未就绪，请在网页检查必填项或验证码；尚未提交');
   await page.waitForTimeout(500);
  }
  if(p.dryRun)return {prepared:true};
  const responsePromise=page.waitForResponse(r=>/^https:\/\/www\.pixiv\.net\/ajax\/work\/create\/(illustration|manga)$/.test(r.url())&&r.request().method()==='POST',{timeout:90000});
  responsePromise.catch(()=>{});clicked=true;await button.click();
  const response=await responsePromise;
  const result=await response.json();
  if(result.error===true&&!result.body?.convertKey)throw new SubmitError('pixiv_rejected','Pixiv 拒绝投稿：'+String(result.message||'请在网页检查投稿设置').slice(0,500));
  const key=result.body?.convertKey;
  if(result.error!==false||typeof key!=='string'||!key||key.length>511)throw Error('invalid response');
  return {convertKey:key};
 }catch(error){
  if(error instanceof SubmitError)throw error;
  throw new SubmitError(clicked?'pixiv_uncertain':'pixiv_not_submitted',clicked?'浏览器提交后未取得确认回执，请先核对作品状态':'Pixiv 网页填写或加载失败，尚未提交；请检查登录、网络或验证码');
 }finally{await context.close().catch(()=>{});}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 console.log=console.info=(...v)=>console.error(...v);
 try {let raw='';for await(const chunk of process.stdin){raw+=chunk;if(raw.length>2*1024*1024)throw Error('large input');}const p=JSON.parse(raw);raw='';process.stdout.write(JSON.stringify(await submit(p)));}
 catch(e){process.stdout.write(JSON.stringify({error:{code:e instanceof SubmitError?e.code:'pixiv_not_submitted',message:e instanceof SubmitError?e.message:'Pixiv 浏览器启动失败，尚未提交'}}));}
}
