import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {spawn} from 'node:child_process';

export const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const hash=text=>crypto.createHash('sha256').update(text).digest('hex');
export function durableWrite(file,text){
  const temporary=file+'.tmp';
  const fd=fs.openSync(temporary,'w');
  try{fs.writeFileSync(fd,text,'utf8');fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
  if(fs.existsSync(file)){
    fs.copyFileSync(file,file+'.bak.tmp');
    const backup=fs.openSync(file+'.bak.tmp','r+');try{fs.fsyncSync(backup)}finally{fs.closeSync(backup)}
    fs.renameSync(file+'.bak.tmp',file+'.bak');
  }
  fs.renameSync(temporary,file);
}
function readText(file){try{return new TextDecoder('utf-8',{fatal:true}).decode(fs.readFileSync(file))}catch(e){if(e.code==='ENOENT')return '';throw e}}
export function createEditorServer({port=43127,token,dataDir=path.join(root,'data'),launch}={}){
  if(!token)throw Error('Token required');
  fs.mkdirSync(dataDir,{recursive:true});
  const file=path.join(dataDir,'web.txt');
  let text='',revision=hash(''),loadError=null,lease=null,leaseUntil=0;
  try{text=readText(file);revision=hash(text)}catch(e){loadError=e}
  const send=(res,code,data)=>{res.writeHead(code,{'Content-Type':'application/json; charset=utf-8'});res.end(JSON.stringify(data))};
  const server=http.createServer(async(req,res)=>{
    res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');res.setHeader('X-Frame-Options','DENY');
    res.setHeader('Content-Security-Policy',"default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self' http://127.0.0.1:"+port+"; frame-ancestors 'none'");
    if(req.headers.host!==`127.0.0.1:${port}`){send(res,403,{error:'Host rejected'});return}
    const origin=req.headers.origin;
    if(origin&&origin!=='null'&&origin!==`http://127.0.0.1:${port}`){send(res,403,{error:'Origin rejected'});return}
    if(origin){res.setHeader('Access-Control-Allow-Origin',origin);res.setHeader('Vary','Origin');res.setHeader('Access-Control-Allow-Headers','Authorization, Content-Type');res.setHeader('Access-Control-Allow-Methods','GET, POST, PUT, DELETE, OPTIONS')}
    if(req.method==='OPTIONS'){res.writeHead(204);res.end();return}
    const url=new URL(req.url,`http://127.0.0.1:${port}`);
    if(req.method==='GET'&&url.pathname==='/health'){send(res,200,{app:'minimal-editor-v0',pid:process.pid});return}
    if(req.method==='GET'&&url.pathname==='/editor'){
      res.setHeader('Content-Type','text/html; charset=utf-8');res.end(fs.readFileSync(path.join(root,'prototypes/web/editor.html')));return
    }
    if(req.method==='GET'&&url.pathname==='/'&&url.searchParams.get('token')===token){
      res.setHeader('Content-Type','text/html; charset=utf-8');res.end(comparisonHtml(port,token));return
    }
    if(req.headers.authorization!==`Bearer ${token}`){send(res,401,{error:'Unauthorized'});return}
    try{
      const chunks=[];let size=0;for await(const chunk of req){size+=chunk.length;if(size>4*1024*1024){send(res,413,{error:'Too large'});return}chunks.push(chunk)}
      const raw=new TextDecoder('utf-8',{fatal:true}).decode(Buffer.concat(chunks));
      const body=raw?JSON.parse(raw):{};
      if(url.pathname==='/api/status'&&req.method==='GET'){send(res,200,{app:'minimal-editor-v0'});return}
      if(url.pathname==='/api/document'&&req.method==='GET'){
        if(loadError){text=readText(file);revision=hash(text);loadError=null}
        send(res,200,{text,revision});return
      }
      if(url.pathname==='/api/lease'){
        if(req.method==='POST'){if(lease&&Date.now()<leaseUntil){send(res,409,{error:'Already open'});return}lease=crypto.randomUUID();leaseUntil=Date.now()+10000;send(res,200,{lease});return}
        if(body.lease!==lease){send(res,409,{error:'Invalid lease'});return}
        if(req.method==='PUT'){leaseUntil=Date.now()+10000;send(res,200,{});return}
        if(req.method==='DELETE'){lease=null;send(res,200,{});return}
      }
      if(url.pathname==='/api/document'&&req.method==='PUT'){
        if(loadError)throw loadError;
        if(!lease||body.lease!==lease||Date.now()>leaseUntil){send(res,409,{error:'Lease lost'});return}
        if(typeof body.text!=='string'){send(res,400,{error:'Text required'});return}
        if(readText(file)!==text){send(res,409,{error:'File changed outside editor; original retained'});return}
        // A request whose response was lost is safe to acknowledge again.
        if(body.revision!==revision){if(body.text===text){send(res,200,{revision});return}send(res,409,{error:'Document changed'});return}
        durableWrite(file,body.text);text=body.text;revision=hash(text);leaseUntil=Date.now()+10000;send(res,200,{revision});return
      }
      if(url.pathname==='/api/recovery'&&req.method==='POST'&&typeof body.text==='string'){
        durableWrite(path.join(dataDir,'web-recovery-'+Date.now()+'-'+crypto.randomUUID()+'.txt'),body.text);send(res,200,{});return
      }
      if(url.pathname==='/api/launch'&&req.method==='POST'&&['web','native','tk'].includes(body.kind)){
        if(!launch)throw Error('Launch unavailable');await launch(body.kind);send(res,200,{});return
      }
      send(res,404,{error:'Not found'});
    }catch(e){send(res,500,{error:String(e.message)})}
  });
  return server;
}
export function comparisonHtml(port,token){return fs.readFileSync(path.join(root,'prototypes/compare.template.html'),'utf8').replaceAll('__PORT__',String(port)).replaceAll('__TOKEN__',token)}
export const edge=path.join(process.env['ProgramFiles(x86)']||'C:\\Program Files (x86)','Microsoft/Edge/Application/msedge.exe');
// All targets here are GUI-subsystem executables (Edge, native, pythonw).
// SW_HIDE suppresses native's first ShowWindow too, leaving an invisible lock owner.
// The separate Node helper still uses windowsHide:true in bootstrap.mjs.
export function startProcess(exe,args){return new Promise((resolve,reject)=>{const child=spawn(exe,args,{cwd:root,detached:true,stdio:'ignore',windowsHide:false});child.once('error',reject);child.once('spawn',()=>{child.unref();resolve()})})}
export function openEdge(url){return startProcess(edge,['--disable-gpu','--no-first-run','--no-default-browser-check','--user-data-dir='+path.join(root,'.work/edge-profile'),'--app='+url])}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const config=JSON.parse(fs.readFileSync(path.join(root,'.work/server.json'),'utf8'));
  const server=createEditorServer({...config,launch:async kind=>{
    if(kind==='web')await openEdge(`http://127.0.0.1:${config.port}/editor#${config.token}`);
    if(kind==='native')await startProcess(path.join(root,'build/native.exe'),[]);
    if(kind==='tk')await startProcess(config.pythonw,[path.join(root,'prototypes/tk/editor.py')]);
  }});
  server.listen(config.port,'127.0.0.1');
  server.on('error',error=>{console.error(error);process.exit(1)});
}
