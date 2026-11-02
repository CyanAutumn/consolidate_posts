import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { atomicJson, PluginError } from './storage.mjs';

function protect(input, decrypt = false) {
 const script = '$ErrorActionPreference="Stop";Add-Type -AssemblyName System.Security;' + (decrypt
  ? '$b=[Convert]::FromBase64String([Console]::In.ReadToEnd());[Console]::Out.Write([Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect($b,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)))'
  : '$b=[Text.Encoding]::UTF8.GetBytes([Console]::In.ReadToEnd());[Console]::Out.Write([Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect($b,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)))');
 try { return execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',script],{input,encoding:'utf8',windowsHide:true,stdio:['pipe','pipe','pipe'],timeout:15000}).trim(); }
 catch { throw new PluginError('bilibili_session','无法保存或解密哔哩哔哩登录态，请在当前 Windows 用户下重新登录'); }
}
const file = profile => path.join(path.dirname(profile),'session.dpapi.json');
export async function saveSession(context, profile) {
 const cookies=(await context.cookies()).filter(c=>['bilibili.com','.bilibili.com','www.bilibili.com','member.bilibili.com','passport.bilibili.com','api.bilibili.com'].includes(c.domain));
 if(!cookies.some(c=>c.name==='SESSDATA'&&c.value)||!cookies.some(c=>c.name==='bili_jct'&&c.value))throw new PluginError('bilibili_login','未取得完整的哔哩哔哩登录凭据，请完成登录');
 await atomicJson(file(profile),{protected:protect(JSON.stringify(cookies))});
}
export async function restoreSession(context, profile) {
 let stored;
 try { stored=JSON.parse(await readFile(file(profile),'utf8')); }
 catch(error){if(error.code==='ENOENT')return;throw new PluginError('bilibili_session','哔哩哔哩加密登录态损坏，请重新登录');}
 const cookies=JSON.parse(protect(stored.protected,true));
 if(!Array.isArray(cookies)||cookies.some(c=>!['bilibili.com','.bilibili.com','www.bilibili.com','member.bilibili.com','passport.bilibili.com','api.bilibili.com'].includes(c.domain)))throw new PluginError('bilibili_session','哔哩哔哩登录态格式无效，请重新登录');
 await context.addCookies(cookies);
}
