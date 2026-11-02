import { openSync,readSync,closeSync,statSync } from 'node:fs';
export function textFor(p){return JSON.stringify([p.title,p.body]);}
export function mediaType(asset){const b=Buffer.alloc(32);const f=openSync(asset.path,'r');let n;try{n=readSync(f,b,0,b.length,0);}finally{closeSync(f);}
 if(n>=8&&b.subarray(0,8).equals(Buffer.from([137,80,78,71,13,10,26,10])))return 'image';
 if(n>=3&&b[0]===255&&b[1]===216&&b[2]===255)return 'image';
 if(n>=12&&b.toString('ascii',0,4)==='RIFF'&&b.toString('ascii',8,12)==='WEBP')return 'image';
 if(n>=12&&b.toString('ascii',4,8)==='ftyp'&&/^(isom|iso[2-9]|mp4[12]|avc1|M4V )$/.test(b.toString('ascii',8,12)))return 'video';throw Error('unsupported');}
export function validate(p){if(!p||typeof p.title!=='string'||typeof p.body!=='string'||!Array.isArray(p.assets))return ['小红书发布内容格式无效'];
 const issues=[];if(!p.title.trim()||p.title.length>20)issues.push('小红书标题必须填写，最多 20 个 UTF-16 单元');if(p.body.length>1000)issues.push('小红书正文当前限制为 1000 个 UTF-16 单元');
 if(!p.assets.length||p.assets.length>18)issues.push('请选择 1–18 张图片，或单个 MP4 视频');let videos=0;
 for(const a of p.assets){try{const s=statSync(a.path);const type=mediaType(a);if(!s.isFile()||s.size!==a.size||!a.size||type!==a.kind)throw Error('changed');if(type==='video')videos++;if(a.size>(type==='video'?512:20)*1024*1024)issues.push(`${a.name}：插件图片上限 20 MiB，视频上限 512 MiB`);}catch{issues.push(`${a.name??'附件'}：文件无效、大小变化或格式不支持；支持 PNG/JPEG/WebP/MP4`);}}
 if(videos&&p.assets.length!==1)issues.push('视频必须单独发布，不能混合图片');return issues;
}
