import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { checkLogin } from '../runner.mjs';
import { atomicJson } from '../storage.mjs';

test('login check verifies saved account and closes browser without composing', async()=>{
 const dir=await mkdtemp(path.join(os.tmpdir(),'x-check-'));
 try{
 await atomicJson(path.join(dir,'config.json'),{username:'tester',userId:'123',headless:true,sensitive:false});
 let closed=false;
 const page={goto:async url=>assert.equal(url,'https://x.com/home'),locator:()=>({first:()=>({getAttribute:async()=>'/tester'})})};
 const launch=async(profile,headless)=>{assert.equal(headless,true);return {pages:()=>[page],cookies:async()=>[{name:'twid',value:'u%3D123'},{name:'auth_token',value:'fixture'}],close:async()=>{closed=true;}};};
 assert.deepEqual(await checkLogin(dir,{launch}),{connected:true,username:'tester'});
 assert.equal(closed,true);
 }finally{await rm(dir,{recursive:true});}
});
