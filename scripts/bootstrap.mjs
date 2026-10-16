import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {root,comparisonHtml,openEdge} from './server.mjs';
const work=path.join(root,'.work');fs.mkdirSync(work,{recursive:true});
const configPath=path.join(work,'server.json');
let config;
try{config=JSON.parse(fs.readFileSync(configPath,'utf8'))}catch{}
if(!config){
  const pythonw=path.join(process.env.APPDATA,'uv/python/cpython-3.12-windows-x86_64-none/pythonw.exe');
  config={port:43127,token:crypto.randomBytes(32).toString('hex'),pythonw};fs.writeFileSync(configPath,JSON.stringify(config,null,2));
}
async function status(){try{const response=await fetch(`http://127.0.0.1:${config.port}/api/status`,{headers:{Authorization:'Bearer '+config.token},signal:AbortSignal.timeout(1000)});return response.ok?await response.json():null}catch{return null}}
const running=await status();
if(running&&running.protocol!==2){
  const health=await(await fetch(`http://127.0.0.1:${config.port}/health`,{signal:AbortSignal.timeout(1000)})).json();
  if(health.app!=='minimal-editor-v0'||!Number.isSafeInteger(health.pid)||health.pid<=0)throw Error('Cannot identify the previous editor server safely.');
  await promisify(execFile)('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(root,'scripts/upgrade-server.ps1'),'-ServerPid',String(health.pid)],{windowsHide:true});
}
if(!running||running.protocol!==2){
  const log=fs.openSync(path.join(work,'server.log'),'a');
  const child=spawn(process.execPath,[path.join(root,'scripts/server.mjs')],{cwd:root,detached:true,stdio:['ignore',log,log],windowsHide:true});child.unref();fs.closeSync(log);
  let ok=false;for(let i=0;i<50;i++){await new Promise(r=>setTimeout(r,100));if((await status())?.protocol===2){ok=true;break}}
  if(!ok)throw Error('Local server failed; see .work/server.log');
}
const comparison=process.argv.includes('--compare');
if(comparison)fs.writeFileSync(path.join(root,'比較.html'),comparisonHtml(config.port,config.token));
if(!process.argv.includes('--no-open'))await openEdge(comparison
  ?`http://127.0.0.1:${config.port}/?token=${config.token}`
  :`http://127.0.0.1:${config.port}/editor#${config.token}`);
