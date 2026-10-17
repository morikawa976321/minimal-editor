// Real, isolated headless Edge UI coverage. No editor apps or clipboard use.
// Parent checks free RAM and runs this test sequentially with other GUI tests.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {createNativeComparison} from '../scripts/native-comparison.mjs';
import {edge, root} from '../scripts/server.mjs';

const port=43142, token=crypto.randomBytes(24).toString('hex');
const base=`http://127.0.0.1:${port}`;
fs.mkdirSync(path.join(root,'.work'),{recursive:true});
const dir=fs.mkdtempSync(path.join(root,'.work/native-comparison-ui-'));
const profile=path.join(dir,'profile');
const launched=[], consoleErrors=[], cleanupErrors=[], screenshots=[];
let server, browser, session, browserEndpoint, spawnError, failure;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const redact=value=>String(value).replaceAll(token,'[redacted]');

async function waitFor(predicate,label,timeout=6000){
  const deadline=Date.now()+timeout;
  while(Date.now()<deadline){
    if(spawnError)throw Error('Owned Edge process could not start');
    const value=await predicate();
    if(value)return value;
    await sleep(50);
  }
  throw Error(`Timed out: ${label}`);
}

async function connect(url){
  const socket=new WebSocket(url);
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>{socket.close();reject(Error('CDP connection timeout'));},5000);
    socket.addEventListener('open',()=>{clearTimeout(timer);resolve();},{once:true});
    socket.addEventListener('error',()=>{clearTimeout(timer);reject(Error('CDP connection failed'));},{once:true});
  });
  let next=1;
  const pending=new Map();
  function rejectPending(){
    for(const {reject,timer} of pending.values()){clearTimeout(timer);reject(Error('CDP connection closed'));}
    pending.clear();
  }
  socket.addEventListener('close',rejectPending);
  socket.addEventListener('error',rejectPending);
  socket.addEventListener('message',event=>{
    const message=JSON.parse(event.data);
    if(message.id){
      const request=pending.get(message.id);
      if(!request)return;
      pending.delete(message.id);clearTimeout(request.timer);
      if(message.error)request.reject(Error(`CDP ${request.method} failed`));
      else request.resolve(message.result);
    }else if(message.method==='Runtime.exceptionThrown'){
      consoleErrors.push('JavaScript exception');
    }else if(message.method==='Runtime.consoleAPICalled'&&message.params.type==='error'){
      consoleErrors.push('console.error');
    }else if(message.method==='Log.entryAdded'&&message.params.entry.level==='error'){
      consoleErrors.push(redact(message.params.entry.text));
    }
  });
  const send=(method,params={},timeout=6000)=>new Promise((resolve,reject)=>{
    if(socket.readyState!==WebSocket.OPEN){reject(Error('CDP connection is not open'));return;}
    const id=next++, timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP timeout: ${method}`));},timeout);
    pending.set(id,{resolve,reject,timer,method});
    try{socket.send(JSON.stringify({id,method,params}));}
    catch{clearTimeout(timer);pending.delete(id);reject(Error(`CDP send failed: ${method}`));}
  });
  return {send,close:()=>{rejectPending();socket.close();}};
}

function running(){return browser?.pid&&browser.exitCode===null&&browser.signalCode===null;}
async function exited(timeout){
  if(!running())return true;
  return new Promise(resolve=>{
    const finish=value=>{clearTimeout(timer);browser.removeListener('exit',onExit);resolve(value);};
    const onExit=()=>finish(true);
    const timer=setTimeout(()=>finish(false),timeout);
    browser.once('exit',onExit);
    if(!running())finish(true);
  });
}

try{
  server=createNativeComparison({port,token,launch:async kind=>{launched.push(kind);}});
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(Error('Comparison server listen timeout')),3000);
    server.once('error',error=>{clearTimeout(timer);reject(error);});
    server.listen(port,'127.0.0.1',()=>{clearTimeout(timer);resolve();});
  });
  browser=spawn(edge,['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check',
    '--disable-background-networking','--disable-extensions','--disable-component-update','--disable-sync',
    '--remote-debugging-port=0','--user-data-dir='+profile,'about:blank'],
    {cwd:root,windowsHide:true,stdio:'ignore'});
  browser.on('error',error=>{spawnError=error;});
  const debugPort=await waitFor(()=>{
    const active=path.join(profile,'DevToolsActivePort');
    if(!fs.existsSync(active))return false;
    const value=fs.readFileSync(active,'utf8').split('\n')[0];
    return /^\d+$/.test(value)&&Number(value)>0?Number(value):false;
  },'owned Edge debugging endpoint',20000);
  const version=await(await fetch(`http://127.0.0.1:${debugPort}/json/version`,{signal:AbortSignal.timeout(3000)})).json();
  browserEndpoint=version.webSocketDebuggerUrl;
  const targets=await(await fetch(`http://127.0.0.1:${debugPort}/json/list`,{signal:AbortSignal.timeout(3000)})).json();
  const target=targets.find(item=>item.type==='page'&&item.url==='about:blank');
  assert.ok(target?.webSocketDebuggerUrl,'Owned about:blank page must exist');
  session=await connect(target.webSocketDebuggerUrl);
  const evaluate=async expression=>{
    const value=await session.send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
    if(value.exceptionDetails)throw Error('Page evaluation failed');
    return value.result.value;
  };
  await session.send('Page.enable');await session.send('Runtime.enable');await session.send('Log.enable');
  await session.send('Emulation.setDeviceMetricsOverride',{width:1100,height:800,deviceScaleFactor:1,mobile:false});
  await session.send('Page.navigate',{url:base+'/#'+token});
  await waitFor(()=>evaluate('document.querySelectorAll("section .problems button").length===12&&document.querySelector("#result").value.includes("選択: 未選択")'),'comparison controls ready');
  assert.equal(await evaluate('location.hash'),'','Token must leave the visible page URL');
  assert.deepEqual(launched,[]);
  for(const [index,kind] of ['web','native'].entries()){
    await evaluate(`document.querySelector('[data-kind="${kind}"] .launch').click()`);
    await waitFor(()=>launched.length===index+1,`${kind} launch callback`);
    await waitFor(()=>evaluate(`performance.getEntriesByType('resource').filter(entry=>entry.name.endsWith('/api/launch')).length===${index+1}`),`${kind} launch response`);
    assert.equal(launched[index],kind);
    assert.equal(await evaluate('document.querySelector("#state").textContent'),'');
  }
  assert.deepEqual(launched,['web','native']);

  for(const [kind,name,reason] of [['web','ブラウザ版','入力'],['native','ネイティブ版','見た目']]){
    await evaluate(`document.querySelector('[data-kind="${kind}"] .select').click()`);
    assert.ok((await evaluate('document.querySelector("#result").value')).includes('選択: '+name));
    assert.equal(await evaluate(`document.querySelector('[data-kind="${kind}"] .select').getAttribute('aria-pressed')`),'true');
    const problem=`[...document.querySelectorAll('[data-kind="${kind}"] .problems button')].find(button=>button.textContent===${JSON.stringify(reason)})`;
    await evaluate(`${problem}.click()`);
    assert.ok((await evaluate('document.querySelector("#result").value')).includes(name+': '+reason));
    await evaluate(`${problem}.click()`);
    assert.ok((await evaluate('document.querySelector("#result").value')).includes(name+': 問題の選択なし'));
    await evaluate(`${problem}.click()`);
  }
  await evaluate('document.querySelector("[data-choice=どちらも問題ない]").click()');
  assert.equal(await evaluate('document.querySelector("[data-choice=どちらも問題ない]").getAttribute("aria-pressed")'),'true');
  const result=await evaluate('document.querySelector("#result").value');
  assert.ok(result.includes('選択: どちらも問題ない'));
  assert.ok(result.includes('ブラウザ版: 入力'));
  assert.ok(result.includes('ネイティブ版: 見た目'));
  assert.equal(await evaluate('document.querySelector("#result").readOnly'),true);

  for(const width of [1100,360]){
    await session.send('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
    await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
    const size=await evaluate('({width:innerWidth,height:innerHeight,content:document.scrollingElement.scrollWidth})');
    assert.equal(size.width,width);assert.equal(size.height,800);
    assert.ok(size.content<=size.width,`No horizontal overflow at ${width}px: ${size.content}`);
    const screenshot=await session.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});
    const name=`comparison-${width}x800.png`;
    fs.writeFileSync(path.join(dir,name),Buffer.from(screenshot.data,'base64'));screenshots.push(name);
  }
  assert.deepEqual(consoleErrors,[],'Comparison page must have no JavaScript or console errors');
}catch(error){failure=redact(error.stack||error.message||error);}
finally{
  // Browser.close closes every process/window of this unique owned profile.
  // A page-only close would leave browser/GPU/utility processes behind.
  if(browserEndpoint||running()){
    let control;
    try{
      control=browserEndpoint?await connect(browserEndpoint):session;
      if(control)await control.send('Browser.close',{},2500).catch(()=>{});
    }catch{try{await session?.send('Browser.close',{},2500);}catch{}}
    finally{if(control&&control!==session)control.close();}
    // Release the page debugger too before waiting for Edge's shutdown.
    // Keeping it attached can keep the owned process alive until this closes.
    session?.close();session=undefined;
    if(!await exited(15000)){
      // Startup/CDP failures can prevent Browser.close. This unique child PID
      // owns only the isolated test profile; never kill browsers by image name.
      // ChildProcess.kill uses the owned native process handle on Windows;
      // avoid invoking a PID-based tree kill after a slow shutdown.
      browser.kill();
      if(!await exited(5000))cleanupErrors.push('Owned Edge process did not exit');
    }
  }
  session?.close();
  if(server){
    await new Promise(resolve=>{
      const timer=setTimeout(()=>{server.closeAllConnections();resolve();},2500);
      server.close(()=>{clearTimeout(timer);resolve();});server.closeAllConnections();
    });
    if(server.listening)cleanupErrors.push('Owned comparison server did not close');
  }
  const report={passed:!failure&&!cleanupErrors.length&&!consoleErrors.length,launched,console_errors:consoleErrors,
    screenshots,clipboard_test:'skipped; no clipboard access',
    artifacts:dir,...(failure?{error:failure}:{}),...(cleanupErrors.length?{cleanup_errors:cleanupErrors}:{})};
  fs.writeFileSync(path.join(dir,'result.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report,null,2));
  if(!report.passed)process.exitCode=1;
}
