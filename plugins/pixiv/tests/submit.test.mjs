import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm,writeFile} from 'node:fs/promises';
import os from 'node:os';import path from 'node:path';
import {submit} from '../browser-submit.mjs';
import {openLoginContext} from '../browser-login.mjs';
test('browser submission selects all four files and obtains a manga conversion key',async()=>{
 const dir=await mkdtemp(path.join(os.tmpdir(),'pixiv-submit-'));let sent=0;
 const config={userId:'123',tags:['test'],rating:'general',aiType:'aiGenerated',visibility:'private',allowComment:true,original:true,allowTagEdit:false};
 const publication={title:'Test',body:'Caption',assets:[]};
 for(let i=0;i<4;i++){const file=path.join(dir,i+'.png');await writeFile(file,Buffer.from('fixture'));publication.assets.push({path:file});}
 const bootstrap=JSON.stringify({props:{pageProps:{isLoggedIn:true,gaUserData:{userId:'123'}}}});
 const html=`<meta charset="utf-8"><script id="__NEXT_DATA__" type="application/json">${bootstrap}</script><input type=file multiple><input name=title><textarea name=comment></textarea><input placeholder="标签">${[['x_restrict','general'],['ai_type','aiGenerated'],['restrict','private'],['allow_comment','true'],['sexual','false']].map(([n,v])=>`<input type=radio name="${n}" value="${v}">`).join('')}<input type=checkbox name=original><input type=checkbox name=allow_tag_edit><button onclick="post()">投稿</button><script>function post(){const files=document.querySelector('input[type=file]').files;if(files.length!==4)throw Error('files');fetch('/ajax/work/create/manga',{method:'POST',body:'fixture'});}</script>`;
 try{const result=await submit({session:'123_fixture',config,rating:'general',publication},{launch:async options=>{
 const c=await openLoginContext(options);await c.route('**/*',async r=>{
 if(r.request().method()==='POST'){sent++;return r.fulfill({json:{error:false,body:{convertKey:'four-files'}}});}
 await r.fulfill({contentType:'text/html; charset=utf-8',body:html});
 });return c;
 }});assert.equal(result.convertKey,'four-files');assert.equal(sent,1);
 }finally{await rm(dir,{recursive:true});}
});
