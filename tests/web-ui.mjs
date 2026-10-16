import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {edge,root,comparisonHtml} from '../scripts/server.mjs';
const dir=fs.mkdtempSync(path.join(root,'.work/web-ui-')),port=43129,token='browser-test-token';
let server,browser,ws;
function killBrowser(){if(browser&&browser.exitCode===null)spawnSync('taskkill',['/PID',String(browser.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'})}
function stage(value){fs.appendFileSync(path.join(dir,'stages.log'),value+'\n')}
async function waitFor(fn,timeout=10000){const start=Date.now();while(Date.now()-start<timeout){try{const value=await fn();if(value)return value}catch{}await new Promise(r=>setTimeout(r,50))}throw Error('Timed out')}
async function startServer(){server=spawn(process.execPath,[path.join(root,'tests/web-fixture.mjs'),String(port),dir],{windowsHide:true,stdio:['ignore','pipe','pipe']});server.stderr.on('data',d=>process.stderr.write(d));await waitFor(async()=>{const r=await fetch(`http://127.0.0.1:${port}/health`);return r.ok})}
try{
  await startServer();
  const started=performance.now(),profile=path.join(dir,'profile');
  browser=spawn(edge,['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-port=0','--user-data-dir='+profile,'about:blank'],{windowsHide:true,stdio:['ignore','ignore','pipe']});
  let browserErrors='';browser.stderr.on('data',d=>browserErrors+=d);
  const debugPort=await waitFor(()=>{const p=path.join(profile,'DevToolsActivePort');return fs.existsSync(p)&&fs.readFileSync(p,'utf8').split('\n')[0]},20000);
  const target=await(await fetch(`http://127.0.0.1:${debugPort}/json/new?${encodeURIComponent(`http://127.0.0.1:${port}/editor#${token}`)}`,{method:'PUT'})).json();
  ws=new WebSocket(target.webSocketDebuggerUrl);await once(ws,'open');let nextId=1;const pending=new Map();
  const handleMessage=e=>{const m=JSON.parse(e.data);if(m.method==='Page.javascriptDialogOpening')cdp('Page.handleJavaScriptDialog',{accept:true}).catch(()=>{});if(m.id){const item=pending.get(m.id);if(!item)return;pending.delete(m.id);clearTimeout(item.timer);m.error?item.reject(Error(JSON.stringify(m.error))):item.resolve(m.result)}};
  ws.addEventListener('message',handleMessage);
  const cdp=(method,params={})=>new Promise((resolve,reject)=>{const id=nextId++;const timer=setTimeout(()=>{pending.delete(id);reject(Error('CDP timeout: '+method))},10000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({id,method,params}))});
  const evaluate=async expression=>{const r=await cdp('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value};
  await cdp('Page.enable');await cdp('Emulation.setDeviceMetricsOverride',{width:1100,height:800,deviceScaleFactor:1,mobile:false});
  await waitFor(()=>evaluate('document.querySelector("textarea") && !document.querySelector("textarea").disabled'));
  const readyMs=performance.now()-started;
  stage('ready');
  // A delayed save stays visually quiet but must still block accidental close.
  await evaluate(`{const original=window.fetch;let hold=true;window.fetch=async(...args)=>{if(hold&&String(args[0]).includes('/api/memos/')&&args[1]?.method==='PUT'){hold=false;await new Promise(resolve=>{window.releaseTestSave=()=>{window.fetch=original;resolve()}})}return original(...args)}}`);
  const text='日本語を一人で書く。abc123\n'.repeat(900);
  const t=performance.now();await cdp('Input.insertText',{text});
  await waitFor(()=>evaluate('typeof window.releaseTestSave==="function"'));
  assert.equal(await evaluate('document.querySelector("#state").textContent'),'');
  assert.equal(await evaluate(`(()=>{const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);return e.defaultPrevented})()`),true);
  await evaluate('window.releaseTestSave()');
  await waitFor(()=>fs.existsSync(path.join(dir,'web.txt'))&&fs.readFileSync(path.join(dir,'web.txt'),'utf8')===text);
  await waitFor(()=>evaluate('!Object.keys(localStorage).some(k=>k.startsWith("unsaved:"))'));
  assert.equal(await evaluate(`(()=>{const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);return e.defaultPrevented})()`),false);
  const insertionMs=performance.now()-t;
  await cdp('Input.imeSetComposition',{text:'へんかん',selectionStart:0,selectionEnd:4});
  assert.ok((await evaluate('document.querySelector("textarea").value')).endsWith('へんかん'));
  await cdp('Input.insertText',{text:'変換'});
  await waitFor(()=>fs.readFileSync(path.join(dir,'web.txt'),'utf8')===text+'変換');
  const image=await cdp('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(dir,'editor.png'),Buffer.from(image.data,'base64'));
  // Kill both after a confirmed disk save, without running close handlers.
  server.kill();await once(server,'exit');
  killBrowser();await once(browser,'exit');ws.close();ws=null;stage('first forced exit');
  await startServer();
  // New browser must reopen the confirmed disk save.
  const profile2=path.join(dir,'profile2');
  browser=spawn(edge,['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-port=0','--user-data-dir='+profile2,'about:blank'],{windowsHide:true,stdio:'ignore'});
  const secondPort=await waitFor(async()=>{const p=fs.readFileSync(path.join(profile2,'DevToolsActivePort'),'utf8').split('\n')[0];const r=await fetch(`http://127.0.0.1:${p}/json/version`);return r.ok&&p},20000);
  const target2=await(await fetch(`http://127.0.0.1:${secondPort}/json/new?${encodeURIComponent(`http://127.0.0.1:${port}/editor#${token}`)}`,{method:'PUT'})).json();
  ws=new WebSocket(target2.webSocketDebuggerUrl);await once(ws,'open');
  ws.addEventListener('message',handleMessage);await cdp('Page.enable');
  await waitFor(()=>evaluate('document.querySelector("textarea") && !document.querySelector("textarea").disabled'));
  const recovered=await evaluate('document.querySelector("textarea").value');
  stage('reopened disk save');
  assert.ok(recovered===text+'変換','Confirmed disk save must survive forced exit');
  await waitFor(()=>fs.readFileSync(path.join(dir,'web.txt'),'utf8')===recovered);
  // Server outage retains editing in the live page; reload recovers its journal.
  server.kill();await once(server,'exit');
  await evaluate('document.querySelector("textarea").setSelectionRange(1e9,1e9)');
  await cdp('Input.insertText',{text:'障害中の追記'});
  await waitFor(()=>evaluate('Object.keys(localStorage).some(k=>k.startsWith("unsaved:")&&localStorage.getItem(k).includes("障害中の追記"))'));
  await startServer();
  stage('outage text journaled; restarting page');
  await evaluate('window.__beforeReload=true');
  await cdp('Page.reload');
  await waitFor(()=>evaluate('!window.__beforeReload && document.querySelector("textarea") && !document.querySelector("textarea").disabled'));
  const outageResult=await evaluate('JSON.stringify({length:document.querySelector("textarea").value.length,tail:document.querySelector("textarea").value.slice(-40),state:document.querySelector("#state").textContent,keys:Object.keys(localStorage)})');stage(outageResult);
  assert.ok((await evaluate('document.querySelector("textarea").value'))===recovered+'障害中の追記','Live journal reload must recover outage text: '+outageResult);
  await waitFor(()=>fs.readFileSync(path.join(dir,'web.txt'),'utf8')===recovered+'障害中の追記');
  stage('recovery complete');
  let expectedLive=recovered+'障害中の追記';const keyMs=[];
  await evaluate('document.querySelector("textarea").setSelectionRange(1e9,1e9)');
  for(let i=0;i<20;i++){const start=performance.now();await cdp('Input.insertText',{text:'字'});keyMs.push(performance.now()-start);expectedLive+='字';await waitFor(()=>fs.readFileSync(path.join(dir,'web.txt'),'utf8')===expectedLive)}
  keyMs.sort((a,b)=>a-b);
  // Save reaches the server, but the client observes a lost response. A newer
  // edit must not replace the exact unconfirmed request before it is reconciled.
  await evaluate(`{const original=window.fetch;let drop=true;window.fetch=async(...args)=>{const response=await original(...args);if(drop&&String(args[0]).includes('/api/memos/')&&args[1]?.method==='PUT'){drop=false;throw new TypeError('simulated response loss')}return response}}`);
  await cdp('Input.insertText',{text:'応'});expectedLive+='応';
  await waitFor(()=>evaluate('document.querySelector("#state").textContent.startsWith("保存できません")'));
  await cdp('Input.insertText',{text:'答'});expectedLive+='答';
  await waitFor(()=>fs.readFileSync(path.join(dir,'web.txt'),'utf8')===expectedLive);
  await waitFor(()=>evaluate('document.querySelector("#state").textContent===""'));
  // Real keyboard shortcuts, save-before-switch, creation order and modal dismissal.
  const key=async(key,code,modifiers=0)=>{
    await cdp('Input.dispatchKeyEvent',{type:'keyDown',key,code,modifiers,windowsVirtualKeyCode:key.length===1?key.toUpperCase().charCodeAt(0):({Enter:13,Escape:27,ArrowUp:38,ArrowDown:40}[key])});
    await cdp('Input.dispatchKeyEvent',{type:'keyUp',key,code,modifiers});
  };
  await key('n','KeyN',2);
  await waitFor(()=>evaluate('document.querySelector("#editor").value===""&&!document.querySelector("#editor").readOnly'));
  await cdp('Input.insertText',{text:'新しいメモ\n本文'});
  await waitFor(()=>evaluate('!Object.keys(localStorage).some(k=>k.startsWith("unsaved:"))'));
  await key('p','KeyP',2);
  await waitFor(()=>evaluate('!document.querySelector("#overlay").hidden'));
  assert.equal(await evaluate('document.querySelectorAll(".memo").length'),2);
  assert.equal(await evaluate('document.querySelector(".memo[aria-selected=true]").textContent'),'新しいメモ');
  await key('ArrowDown','ArrowDown');await key('Enter','Enter');
  await waitFor(()=>evaluate('document.querySelector("#overlay").hidden'));
  assert.equal(await evaluate('document.querySelector("#editor").value'),expectedLive);
  for(const dismiss of ['escape','close','outside']){
    await key('p','KeyP',2);await waitFor(()=>evaluate('!document.querySelector("#overlay").hidden'));
    assert.equal(await evaluate('[...document.querySelectorAll(".memo")].findIndex(e=>e.getAttribute("aria-selected")==="true")'),1);
    await key('ArrowUp','ArrowUp');
    if(dismiss==='escape')await key('Escape','Escape');
    else await evaluate(`document.querySelector('${dismiss==='close'?'#close':'#overlay'}').click()`);
    assert.equal(await evaluate('document.querySelector("#overlay").hidden'),true);
    assert.equal(await evaluate('document.querySelector("#editor").value'),expectedLive);
  }
  await evaluate(`{const original=window.fetch;window.testFetch=original;window.fetch=(...args)=>{if(String(args[0]).includes('/api/memos/')&&args[1]?.method==='PUT')return Promise.reject(new TypeError('simulated outage'));return original(...args)}}`);
  await evaluate('document.querySelector("#editor").setSelectionRange(1e9,1e9)');
  await cdp('Input.insertText',{text:'保存失敗中'});expectedLive+='保存失敗中';
  await waitFor(()=>evaluate('document.querySelector("#state").textContent.startsWith("保存できません")'));
  await key('p','KeyP',2);await waitFor(()=>evaluate('!document.querySelector("#overlay").hidden'));
  await evaluate('document.querySelector(".memo").click()');
  await new Promise(r=>setTimeout(r,150));
  assert.equal(await evaluate('document.querySelector("#editor").value'),expectedLive);
  assert.equal(await evaluate('document.querySelector("#overlay").hidden'),false);
  await evaluate('window.fetch=window.testFetch;document.querySelector(".memo").click()');
  await waitFor(()=>evaluate('document.querySelector("#overlay").hidden'));
  assert.equal(fs.readFileSync(path.join(dir,'web.txt'),'utf8'),expectedLive);
  assert.equal(await evaluate('document.querySelector("#editor").value'),'新しいメモ\n本文');
  await evaluate('window.__memoReload=true');await cdp('Page.reload');
  await waitFor(()=>evaluate('!window.__memoReload&&document.querySelector("#editor")&&!document.querySelector("#editor").disabled'));
  assert.equal(await evaluate('document.querySelector("#editor").value'),'新しいメモ\n本文');
  stage('memo switching, failed flush, last memo reload passed');
  await cdp('Page.navigate',{url:`http://127.0.0.1:${port}/?token=${token}`});
  await waitFor(()=>evaluate('!!document.querySelector(".best")'));
  await evaluate('document.querySelector(".best").click();document.querySelector(".reason button").click()');
  assert.ok((await evaluate('document.querySelector("#result").value')).includes('選択: A · Edge'));
  assert.ok((await evaluate('document.querySelector("#result").value')).includes('致命的な問題 — 入力'));
  await cdp('Emulation.setDeviceMetricsOverride',{width:1100,height:800,deviceScaleFactor:1,mobile:false});
  const comparison=await cdp('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(dir,'comparison.png'),Buffer.from(comparison.data,'base64'));
  fs.writeFileSync(path.join(dir,'compare.html'),comparisonHtml(port,token));
  await cdp('Page.navigate',{url:'file:///'+path.join(dir,'compare.html').replaceAll('\\','/')});
  await waitFor(()=>evaluate('!!document.querySelector(".launch")'));
  await evaluate('document.querySelector(".launch").click()');
  await new Promise(r=>setTimeout(r,400));
  assert.equal(await evaluate('document.querySelector("#status").textContent'),'');
  // Typography comparison is isolated from real notes and application settings.
  await cdp('Page.navigate',{url:'file:///'+path.join(root,'previews/typography.html').replaceAll('\\','/')});
  await waitFor(()=>evaluate('!!document.querySelector("#sample")'));
  for(const [index,line,spacing] of [[0,'30px','normal'],[1,'30px','0.4px'],[2,'38px','normal']]){
    await evaluate(`document.querySelectorAll('.presets button')[${index}].click()`);
    const style=await evaluate(`(()=>{const s=getComputedStyle(document.querySelector('#sample'));return [s.lineHeight,s.letterSpacing]})()`);
    assert.equal(style[0],line);assert.ok(style[1]===spacing||(spacing==='normal'&&style[1]==='0px'));
  }
  await evaluate(`{const a=document.querySelector('#line-value'),b=document.querySelector('#letter-value');a.value='1.65';a.dispatchEvent(new Event('input'));b.value='-0.025';b.dispatchEvent(new Event('input'))}`);
  assert.deepEqual(await evaluate(`(()=>{const s=getComputedStyle(document.querySelector('#sample'));return [s.lineHeight,s.letterSpacing]})()`),['33px','-0.5px']);
  assert.ok((await evaluate('document.querySelector("#result").value')).includes('1.65'));
  await cdp('Emulation.setDeviceMetricsOverride',{width:1200,height:950,deviceScaleFactor:1,mobile:false});
  const typography=await cdp('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(dir,'typography.png'),Buffer.from(typography.data,'base64'));
  const report={passed:true,characters:[...text].length,ready_ms:+readyMs.toFixed(2),insert_and_save_ms:+insertionMs.toFixed(2),long_draft_key_dispatch_median_ms:+keyMs[10].toFixed(2),long_draft_key_dispatch_p95_ms:+keyMs[18].toFixed(2),quiet_save_with_unsaved_close_guard:true,cdp_composition:true,force_kill_and_recovery:true,response_loss_recovery:true,file_launcher:true,artifacts:dir};
  fs.writeFileSync(path.join(dir,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
}finally{
  if(ws)ws.close();
  if(browser&&browser.exitCode===null){killBrowser();await once(browser,'exit')}
  if(server&&server.exitCode===null){server.kill();await once(server,'exit')}
}
