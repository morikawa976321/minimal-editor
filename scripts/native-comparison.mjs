import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {launchNative,root} from './launch-native.mjs';

function start(exe,args){return new Promise((resolve,reject)=>{const child=spawn(exe,args,{cwd:root,detached:true,stdio:'ignore',windowsHide:!exe.toLowerCase().endsWith('msedge.exe')});child.once('error',reject);child.once('spawn',()=>{child.unref();resolve()})})}
export function createNativeComparison({port=43140,token,launch,onActivity=()=>{}}){
  if(!token)throw Error('Comparison token required');
  return http.createServer(async(req,res)=>{
    const send=(status,value)=>{res.writeHead(status,{'Content-Type':'application/json; charset=utf-8'});res.end(JSON.stringify(value))};
    res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');res.setHeader('X-Frame-Options','DENY');
    res.setHeader('Content-Security-Policy',"default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'");
    if(req.headers.host!==`127.0.0.1:${port}`)return send(403,{error:'Host rejected'});
    if(req.headers.origin&&req.headers.origin!==`http://127.0.0.1:${port}`)return send(403,{error:'Origin rejected'});
    if(req.method==='GET'&&req.url==='/health')return send(200,{app:'minimal-editor-native-comparison',version:1});
    if(req.method==='GET'&&req.url==='/favicon.ico'){res.writeHead(204);return res.end()}
    if(req.method==='GET'&&req.url==='/'){
      res.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});
      return res.end(fs.readFileSync(path.join(root,'prototypes/native-comparison.html')));
    }
    if(req.headers.authorization!==`Bearer ${token}`)return send(401,{error:'Unauthorized'});
    if(req.method==='POST'&&req.url==='/api/heartbeat'){onActivity();return send(200,{})}
    if(req.method!=='POST'||req.url!=='/api/launch')return send(404,{error:'Not found'});
    try{
      const chunks=[];let bytes=0;
      for await(const chunk of req){bytes+=chunk.length;if(bytes>1024)return send(413,{error:'Too large'});chunks.push(chunk)}
      const body=JSON.parse(Buffer.concat(chunks).toString('utf8'));
      if(!['web','native'].includes(body.kind))return send(400,{error:'Invalid editor'});
      onActivity();await launch(body.kind);send(200,{});
    }catch(error){send(500,{error:error.message})}
  });
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const work=path.join(root,'.work');fs.mkdirSync(work,{recursive:true});
  const configFile=path.join(work,'native-comparison.json');let config;
  try{config=JSON.parse(fs.readFileSync(configFile,'utf8'))}catch(error){if(error.code!=='ENOENT')throw error}
  if(!config){
    const initial={port:43140,token:crypto.randomBytes(32).toString('hex')};
    try{fs.writeFileSync(configFile,JSON.stringify(initial),{flag:'wx'})}catch(error){if(error.code!=='EEXIST')throw error}
    config=JSON.parse(fs.readFileSync(configFile,'utf8'));
  }
  if(config.port!==43140||typeof config.token!=='string'||!/^[0-9a-f]{64}$/.test(config.token))throw Error('Invalid comparison configuration');
  const base=`http://127.0.0.1:${config.port}`;
  let running=false;
  try{running=(await(await fetch(base+'/health',{signal:AbortSignal.timeout(1000)})).json()).app==='minimal-editor-native-comparison'}catch{}
  if(!running){
    let lastActivity=Date.now();
    const server=createNativeComparison({...config,onActivity:()=>{lastActivity=Date.now()},launch:async kind=>{
      if(kind==='native')await launchNative();
      else await start('wscript.exe',[path.join(root,'scripts/app.vbs')]);
    }});
    try{await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(config.port,'127.0.0.1',resolve)})}
    catch(error){
      // A simultaneous launcher may have started the same known helper.
      if(error.code!=='EADDRINUSE')throw error;
      const health=await(await fetch(base+'/health',{signal:AbortSignal.timeout(1000)})).json();
      const authorized=await fetch(base+'/api/heartbeat',{method:'POST',headers:{Authorization:'Bearer '+config.token},signal:AbortSignal.timeout(1000)});
      if(health.app!=='minimal-editor-native-comparison'||!authorized.ok)throw error;
    }
    // Only the comparison launcher needs Node. Close its page after launching
    // Native; this helper exits, leaving the editor's single process running.
    if(server.listening)setInterval(()=>{if(Date.now()-lastActivity>90000){server.close();server.closeAllConnections();process.exit(0)}},10000).unref();
  }
  if(!process.argv.includes('--no-open')){
    const edge=path.join(process.env['ProgramFiles(x86)']||'C:\\Program Files (x86)','Microsoft/Edge/Application/msedge.exe');
    await start(edge,['--disable-gpu','--no-first-run','--no-default-browser-check','--user-data-dir='+path.join(work,'native-comparison-profile'),'--app='+base+'/#'+config.token]);
  }
}
