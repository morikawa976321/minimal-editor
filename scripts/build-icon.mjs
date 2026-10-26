// Optional asset regeneration: render the SVG with the installed headless Edge.
// Normal native builds embed the checked-in ICO and do not launch a browser.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const edge=path.join(process.env['ProgramFiles(x86)']||'C:/Program Files (x86)','Microsoft/Edge/Application/msedge.exe');
fs.mkdirSync(path.join(root,'.work'),{recursive:true});
const profile=fs.mkdtempSync(path.join(root,'.work/icon-assets-'));
const browser=spawn(edge,['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-port=0','--user-data-dir='+profile,'about:blank'],{windowsHide:true,stdio:['ignore','ignore','pipe']});
let browserErrors='';browser.stderr.on('data',data=>browserErrors+=data);
let ws;
try {
  const started=Date.now();let port;
  while(Date.now()-started<15000){
    // On Windows the Edge launcher may exit successfully after handing off.
    if(browser.exitCode!==null&&browser.exitCode!==0)throw Error('Edge exited before icon rendering ('+browser.exitCode+'): '+browserErrors);
    const file=path.join(profile,'DevToolsActivePort');
    if(fs.existsSync(file)){port=fs.readFileSync(file,'utf8').split('\n')[0];break}
    await new Promise(r=>setTimeout(r,50));
  }
  if(!port)throw Error('Edge debugging endpoint not ready');
  const target=await(await fetch(`http://127.0.0.1:${port}/json/new?about:blank`,{method:'PUT'})).json();
  ws=new WebSocket(target.webSocketDebuggerUrl);await once(ws,'open');
  let id=0;const pending=new Map();
  ws.addEventListener('message',event=>{const data=JSON.parse(event.data);if(data.id){const p=pending.get(data.id);if(p){pending.delete(data.id);clearTimeout(p.timer);data.error?p.reject(Error(JSON.stringify(data.error))):p.resolve(data.result)}}});
  const cdp=(method,params={})=>new Promise((resolve,reject)=>{
    const i=++id,timer=setTimeout(()=>{pending.delete(i);reject(Error('CDP timeout: '+method))},10000);
    pending.set(i,{resolve,reject,timer});ws.send(JSON.stringify({id:i,method,params}));
  });
  await cdp('Page.enable');
  await cdp('Emulation.setDefaultBackgroundColorOverride',{color:{r:0,g:0,b:0,a:0}});
  const svg=fs.readFileSync(path.join(root,'assets/minimal-editor.svg'),'utf8');
  await cdp('Runtime.evaluate',{expression:`document.body.style.margin='0';document.body.innerHTML=${JSON.stringify(svg)};const svg=document.querySelector('svg');svg.style.cssText='display:block;width:100vw;height:100vh'`});
  const images=[];
  for(const size of [16,20,24,32,40,48,64,128,256]){
    await cdp('Emulation.setDeviceMetricsOverride',{width:size,height:size,deviceScaleFactor:1,mobile:false});
    // Screenshot waits for the compositor to present the resized SVG viewport.
    const shot=await cdp('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
    const bytes=Buffer.from(shot.data,'base64');
    if(bytes.readUInt32BE(16)!==size||bytes.readUInt32BE(20)!==size)throw Error('Wrong icon dimensions');
    images.push({size,bytes});
  }
  const header=Buffer.alloc(6+images.length*16);header.writeUInt16LE(1,2);header.writeUInt16LE(images.length,4);
  let offset=header.length;
  images.forEach(({size,bytes},i)=>{
    const at=6+i*16;header[at]=header[at+1]=size===256?0:size;
    header.writeUInt16LE(1,at+4);header.writeUInt16LE(32,at+6);
    header.writeUInt32LE(bytes.length,at+8);header.writeUInt32LE(offset,at+12);offset+=bytes.length;
  });
  fs.writeFileSync(path.join(root,'assets/minimal-editor.ico'),Buffer.concat([header,...images.map(v=>v.bytes)]));
  fs.writeFileSync(path.join(root,'assets/minimal-editor.png'),images.at(-1).bytes);
  console.log('Icon assets: 16, 20, 24, 32, 40, 48, 64, 128, 256 px');
} finally {
  if(ws?.readyState===WebSocket.OPEN){
    ws.send(JSON.stringify({id:0,method:'Browser.close'}));
    await Promise.race([once(ws,'close'),new Promise(r=>setTimeout(r,1500))]);
  }
  if(ws)ws.close();
  if(browser.exitCode===null)spawnSync('taskkill',['/PID',String(browser.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});
}
