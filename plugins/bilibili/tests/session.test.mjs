import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,rm} from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {saveSession,restoreSession} from '../session.mjs';
import {openBrowser} from '../browser.mjs';
test('DPAPI snapshot restores main-site and creator login cookies into a reopened headless browser',async()=>{
 const dir=await mkdtemp(path.join(os.tmpdir(),'bili-session-'));const profile=path.join(dir,'profile');let context;
 try{
 context=await openBrowser(profile,true);
 await context.addCookies([{name:'SESSDATA',value:'fixture-secret-never-plaintext',domain:'.bilibili.com',path:'/',secure:true},{name:'bili_jct',value:'u%3D123',domain:'.bilibili.com',path:'/',secure:true}]);
 await saveSession(context,profile);await context.close();context=null;
 assert.ok(!(await readFile(path.join(dir,'session.dpapi.json'),'utf8')).includes('fixture-secret'));
 context=await openBrowser(profile,true);
 assert.equal((await context.cookies('https://www.bilibili.com/')).find(c=>c.name==='SESSDATA')?.value,'fixture-secret-never-plaintext');
 }finally{await context?.close();await rm(dir,{recursive:true});}
});
