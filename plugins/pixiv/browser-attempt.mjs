import { openLoginContext, loggedInUser } from './browser-login.mjs';
import fs from 'node:fs/promises';
let raw='';for await(const c of process.stdin)raw+=c;const p=JSON.parse(raw);raw='';
const existing=JSON.parse(await fs.readFile(p.journal,'utf8'));
if(existing.receipt){console.log(JSON.stringify({receipt:existing.receipt,reused:true}));process.exit(0);}
if(existing.browserAttempt?.stage==='submitting')throw Error('已有浏览器提交尝试，请先核对远端结果，禁止盲目重发');
const ctx=await openLoginContext({headless:true});
try {
 await ctx.addCookies([{name:'PHPSESSID',value:p.session,domain:'.pixiv.net',path:'/',secure:true,httpOnly:true}]);
 const page=await ctx.newPage();await page.goto('https://www.pixiv.net/illustration/create',{waitUntil:'domcontentloaded'});
 if(await loggedInUser(page)!==p.config.userId)throw Error('登录用户不符');
 await page.locator('input[type=file]').setInputFiles(p.file);
 await page.locator('input[name=title]').fill(p.title);
 await page.locator('textarea[name=comment]').fill(p.body);
 const tags=page.getByPlaceholder('标签',{exact:true});
 for(const tag of p.config.tags){await tags.fill(tag);await tags.press('Enter');}
 for(const [name,value] of [['x_restrict',p.config.rating],['ai_type',p.config.aiType],['restrict',p.config.visibility],['allow_comment',String(p.config.allowComment)]]){
 await page.locator(`input[name="${name}"][value="${value}"]`).check();
 }
 await page.locator('input[name=original]').setChecked(p.config.original);
 await page.locator('input[name=allow_tag_edit]').setChecked(p.config.allowTagEdit);
 await page.waitForTimeout(2000);
 if(await page.locator('input[name=sexual][value=false]').count())await page.locator('input[name=sexual][value=false]').check();
 await page.waitForTimeout(1500);
 const btn=page.getByRole('button',{name:'投稿',exact:true}).last();
 if(!await btn.isEnabled())throw Error('投稿按钮不可用，未提交');
 const journal=JSON.parse(await fs.readFile(p.journal,'utf8'));journal.browserAttempt={startedAt:new Date().toISOString(),stage:'submitting'};await fs.writeFile(p.journal,JSON.stringify(journal,null,2));
 const responsePromise=page.waitForResponse(r=>r.url().endsWith('/ajax/work/create/illustration')&&r.request().method()==='POST',{timeout:60000});
 await btn.click();
 const response=await responsePromise;const result=await response.json();
 console.log(JSON.stringify({stage:'response',status:response.status(),result}));
 if(result.error===false&&result.body?.convertKey){journal.convertKey=result.body.convertKey;journal.stage='processing';await fs.writeFile(p.journal,JSON.stringify(journal,null,2));
 for(let i=0;i<90;i++){const progress=await page.evaluate(async(key)=>(await(await fetch('/ajax/work/create/illustration/progress?convertKey='+encodeURIComponent(key))).json()),result.body.convertKey);if(progress.body?.status==='COMPLETE'){journal.stage='complete';journal.browserAttempt.stage='complete';journal.receipt={location:'https://www.pixiv.net/artworks/'+progress.body.illustId,files:[]};await fs.writeFile(p.journal,JSON.stringify(journal,null,2));console.log(JSON.stringify({receipt:journal.receipt}));break;}if(progress.body?.status==='FAILURE')throw Error('图片转换失败');await page.waitForTimeout(2000);}
 }
} finally {await ctx.close();}
