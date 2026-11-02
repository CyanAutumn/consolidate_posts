import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,rm} from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {saveSession,restoreSession} from '../session.mjs';
import {openBrowser} from '../browser.mjs';
test('DPAPI snapshot restores main-site and creator login cookies into a reopened headless browser',async()=>{
 const dir=await mkdtemp(path.join(os.tmpdir(),'xhs-session-'));const profile=path.join(dir,'profile');let context;
 try{
 context=await openBrowser(profile,true);
 await context.addCookies([{name:'web_session',value:'fixture-secret-never-plaintext',domain:'.xiaohongshu.com',path:'/',secure:true},{name:'galaxy_creator_session_id',value:'u%3D123',domain:'.xiaohongshu.com',path:'/',secure:true}]);
 await saveSession(context,profile);await context.close();context=null;
 assert.ok(!(await readFile(path.join(dir,'session.dpapi.json'),'utf8')).includes('fixture-secret'));
 context=await openBrowser(profile,true);
 assert.equal((await context.cookies('https://www.xiaohongshu.com/')).find(c=>c.name==='web_session')?.value,'fixture-secret-never-plaintext');
 }finally{await context?.close();await rm(dir,{recursive:true});}
});
