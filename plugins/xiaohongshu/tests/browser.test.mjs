import {test,before,after} from 'node:test';import assert from 'node:assert/strict';
import {mkdtemp,rm,writeFile} from 'node:fs/promises';import path from 'node:path';import os from 'node:os';
import {openBrowser,composeAndPost,receiptFrom,verifyIdentity} from '../browser.mjs';
import {atomicJson,prepareJournal} from '../storage.mjs';import {validate} from '../validation.mjs';import {upload} from '../runner.mjs';
let context,dir;const config={userId:'123abc',username:'测试账号',headless:true,sensitive:false};let mode={code:0,id:'0123456789abcdef01234567',modern:false};let posts=0;
const png=Buffer.from('89504e470d0a1a0a00000000','hex');
function fixture(){return `<meta charset="utf-8"><input type=file multiple><input placeholder="填写标题"><div role=textbox contenteditable=true></div><div class=img-preview-area></div>${mode.modern?'<xhs-publish-btn is-publish="true" submit-disabled="false">发布</xhs-publish-btn>':'<button>发布</button>'}<script>
 document.querySelector('input[type=file]').onchange=e=>document.querySelector('.img-preview-area').innerHTML=Array.from(e.target.files).map(()=>'<div class=pr></div>').join('');
 document.querySelector('button,xhs-publish-btn').onclick=()=>fetch('https://edith.xiaohongshu.com/web_api/sns/v2/note',{method:'POST',body:JSON.stringify({title:document.querySelector('input[placeholder]').value})});</script>`;}
before(async()=>{dir=await mkdtemp(path.join(os.tmpdir(),'xhs-test-'));context=await openBrowser(path.join(dir,'profile'),true);
 await context.addCookies([{name:'galaxy_creator_session_id',value:'local-fixture',domain:'.xiaohongshu.com',path:'/',secure:true}]);
 await context.route('**/*',r=>{const url=r.request().url();if(r.request().method()==='POST'){posts++;return r.fulfill({json:{code:mode.code,success:mode.code===0,data:mode.id?{id:mode.id}:{},msg:'fixture rejection'}});}
 if(url.includes('/explore'))return r.fulfill({contentType:'text/html; charset=utf-8',body:`<script>window.__INITIAL_STATE__={user:{userInfo:{value:{userId:'123abc',nickname:'测试账号'}}}};</script>`});
 return r.fulfill({contentType:'text/html; charset=utf-8',body:fixture()});});});
after(async()=>{await context?.close();await rm(dir,{recursive:true});});
async function setup(name,video=false){const file=path.join(dir,name+(video?'.mp4':'.png'));const bytes=video?Buffer.from([0,0,0,24,102,116,121,112,105,115,111,109]):png;await writeFile(file,bytes);const a={path:file,name:path.basename(file),kind:video?'video':'image',size:bytes.length,sha256:name};
 const request={jobId:name,operationId:name,publication:{title:'测试标题',body:'正文',assets:[a]},workspace:{dataDir:dir,contentRating:'all_ages'},target:{id:'account',driverId:'xiaohongshu'},outputDir:dir};
 return {request,journal:await prepareJournal(request,config,path.join(dir,name),false)};}
async function post(name,video=false){const v=await setup(name,video);const page=await context.newPage();try{return await composeAndPost(page,v.request,config,v.journal,{responseTimeout:2000});}finally{await page.close();}}
test('real headless browser uploads an image and confirms a note ID',async()=>{assert.match((await post('image')).location,/0123456789abcdef01234567$/);});
test('modern publish widget supports a single MP4',async()=>{mode.modern=true;try{assert.match((await post('video',true)).location,/explore/);}finally{mode.modern=false;}});
test('explicit rejection remains manually retryable',async()=>{mode.code=400;mode.id=null;try{await assert.rejects(post('rejected'),{code:'xiaohongshu_rejected'});const v=await setup('rejected');assert.equal(v.journal.value.stage,'prepared');}finally{mode.code=0;mode.id='0123456789abcdef01234567';}});
test('missing receipt blocks retries and other submissions',async()=>{mode.id=null;try{await assert.rejects(post('unknown'),{code:'xiaohongshu_uncertain'});await assert.rejects(setup('unknown'),{code:'xiaohongshu_uncertain'});}finally{mode.id='0123456789abcdef01234567';}});
test('wrong account fails without publishing',async()=>{const page=await context.newPage();const before=posts;try{await assert.rejects(verifyIdentity(page,context,{...config,userId:'bad'}),{code:'xiaohongshu_account'});assert.equal(posts,before);}finally{await page.close();}});
test('completed operations reuse their receipt without opening a browser',async()=>{const {request}=await setup('cached');const folder=path.join(dir,'account');await atomicJson(path.join(folder,'config.json'),config);const j=await prepareJournal(request,config,folder,false);await j.complete({location:'https://www.xiaohongshu.com/explore/0123456789abcdef01234567',files:[]});const r=await upload(request,{launch:()=>{throw Error('must not launch');}});assert.match(r.location,/explore/);});
test('content validation rejects long titles, mixed video and false receipts',async()=>{const {request}=await setup('validation');assert.deepEqual(validate(request.publication),[]);assert.ok(validate({...request.publication,title:'a'.repeat(21)}).length);assert.ok(validate({...request.publication,body:'a'.repeat(1001)}).length);assert.throws(()=>receiptFrom({code:0,data:{}}),{code:'xiaohongshu_uncertain'});});
