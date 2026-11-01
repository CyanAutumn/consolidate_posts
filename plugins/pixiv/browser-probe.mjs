import { openLoginContext, loggedInUser } from './browser-login.mjs';
let input='';for await (const chunk of process.stdin) input+=chunk;
const {session,userId}=JSON.parse(input);input='';
const ctx=await openLoginContext({headless:true});
try {
 await ctx.addCookies([{name:'PHPSESSID',value:session,domain:'.pixiv.net',path:'/',secure:true,httpOnly:true}]);
 const page=await ctx.newPage();
 await page.goto('https://www.pixiv.net/illustration/create',{waitUntil:'domcontentloaded'});
 if(await loggedInUser(page)!==userId) throw Error('浏览器登录验证失败');
 const works=await page.evaluate(async(id)=>{const r=await fetch(`/ajax/user/${id}/profile/all`);const j=await r.json();return {error:j.error,illusts:Object.keys(j.body?.illusts??{}),manga:Object.keys(j.body?.manga??{})};},userId);
 const ids=[...works.illusts,...works.manga].sort((a,b)=>Number(b)-Number(a)).slice(0,5);
 for(const id of ids){ console.log(JSON.stringify(await page.evaluate(async(id)=>{const j=await(await fetch(`/ajax/illust/${id}`)).json();return {id,title:j.body?.title,createDate:j.body?.createDate,uploadDate:j.body?.uploadDate};},id))); }
}finally{await ctx.close();}
