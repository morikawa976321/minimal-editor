import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {fileURLToPath} from 'node:url';
const exec=promisify(execFile);
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const fixture=protocol=>`import http from 'node:http';import fs from 'node:fs';import path from 'node:path';import{fileURLToPath}from'node:url';
export const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
export function comparisonHtml(){return ''} export function openEdge(){throw Error('Test must not launch GUI')}
if(process.argv[1]===fileURLToPath(import.meta.url)){const c=JSON.parse(fs.readFileSync(path.join(root,'.work/server.json')));http.createServer((q,s)=>{s.setHeader('Content-Type','application/json');s.end(JSON.stringify(q.url==='/health'?{app:'minimal-editor-v0',pid:process.pid}:{app:'minimal-editor-v0'${protocol?',protocol:'+protocol:''}}))}).listen(c.port,'127.0.0.1')}`;
for(const oldProtocol of [undefined,2])test(`upgrade protocol ${oldProtocol??'legacy'} preserves active editor, rejects other process, replaces cold server with protocol 3`,{skip:process.platform!=='win32',timeout:60000},async()=>{
  const base=fs.mkdtempSync(path.join(repo,'.work/upgrade-test-'));
  const scripts=path.join(base,'scripts');fs.mkdirSync(scripts);fs.mkdirSync(path.join(base,'.work'));
  for(const name of ['bootstrap.mjs','upgrade-server.ps1','focus-editor.ps1'])fs.copyFileSync(path.join(repo,'scripts',name),path.join(scripts,name));
  const net=await import('node:net');const probe=net.createServer();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));
  fs.writeFileSync(path.join(base,'.work/server.json'),JSON.stringify({port,token:'isolated-upgrade-test'}));
  const serverPath=path.join(scripts,'server.mjs');fs.writeFileSync(serverPath,fixture(oldProtocol));
  const old=spawn(process.execPath,[serverPath],{windowsHide:true,stdio:'ignore'});
  const endpoint=`http://127.0.0.1:${port}`;
  async function health(){return(await fetch(endpoint+'/health',{signal:AbortSignal.timeout(1000)})).json()}
  let upgradedPid;
  try{
    for(let n=0;n<50;n++){try{await health();break}catch{await delay(100)}}
    assert.equal((await health()).pid,old.pid);
    const focusPath=path.join(scripts,'focus-editor.ps1');const realFocus=fs.readFileSync(focusPath,'utf8');
    fs.writeFileSync(focusPath,"param([switch]$ProbeOnly)\n'{\"found\":true}'\n");
    await assert.rejects(exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(scripts,'upgrade-server.ps1'),'-ServerPid',String(old.pid)],{windowsHide:true}),/Close minimal-editor/);
    assert.equal((await health()).pid,old.pid,'active editor keeps old server');
    fs.writeFileSync(focusPath,realFocus);
    await assert.rejects(exec('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(scripts,'upgrade-server.ps1'),'-ServerPid',String(process.pid)],{windowsHide:true}),/Refusing to stop/);
    assert.equal((await health()).pid,old.pid);
    fs.writeFileSync(serverPath,fixture(3));
    await exec(process.execPath,[path.join(scripts,'bootstrap.mjs'),'--no-open'],{windowsHide:true});
    upgradedPid=(await health()).pid;
    assert.notEqual(upgradedPid,old.pid);
    assert.equal((await(await fetch(endpoint+'/api/status')).json()).protocol,3);
    await exec(process.execPath,[path.join(scripts,'bootstrap.mjs'),'--no-open'],{windowsHide:true});
    assert.equal((await health()).pid,upgradedPid,'current server is reused');
  }finally{
    old.kill();
    if(upgradedPid)try{process.kill(upgradedPid)}catch{}
  }
});
