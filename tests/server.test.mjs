import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {createEditorServer,root} from '../scripts/server.mjs';

test('HTTP persistence, conflicts, malformed requests and local-only authorization',async()=>{
  const dataDir=fs.mkdtempSync(path.join(root,'.work/server-test-'));
  const token='test-token',port=43128;let launches=[];
  const server=createEditorServer({port,token,dataDir,launch:async k=>launches.push(k)});
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(port,'127.0.0.1',resolve)});
  const api=(route,method='GET',body,extra={})=>fetch(`http://127.0.0.1:${port}${route}`,{method,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',...extra},body:body===undefined?undefined:JSON.stringify(body)});
  try{
    assert.equal((await fetch(`http://127.0.0.1:${port}/api/document`)).status,401);
    assert.equal((await api('/api/document','GET',undefined,{Origin:'https://attacker.example'})).status,403);
    const badHost=await new Promise((resolve,reject)=>{const req=http.get({host:'127.0.0.1',port,path:'/api/document',headers:{Host:'attacker.example',Authorization:'Bearer '+token}},res=>{res.resume();resolve(res.statusCode)});req.on('error',reject)});
    assert.equal(badHost,403);
    assert.equal((await api('/api/document','OPTIONS',undefined,{Origin:'null'})).status,204);
    const initial=await (await api('/api/document')).json();assert.equal(initial.text,'');
    const {lease}=await(await api('/api/lease','POST')).json();
    assert.equal((await api('/api/lease','POST')).status,409);
    const text='日本語📝\nabc123\n'.repeat(1300);
    const bytes=Buffer.from(JSON.stringify({lease,text,revision:initial.revision}));
    const split=bytes.indexOf(Buffer.from('日'))+1;
    const saved=await new Promise((resolve,reject)=>{const req=http.request({host:'127.0.0.1',port,path:'/api/document',method:'PUT',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'}},res=>{let body='';res.setEncoding('utf8');res.on('data',s=>body+=s);res.on('end',()=>{try{assert.equal(res.statusCode,200);resolve(JSON.parse(body))}catch(e){reject(e)}})});req.on('error',reject);req.write(bytes.subarray(0,split));setTimeout(()=>req.end(bytes.subarray(split)),20)});
    assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),text);
    const duplicate=await api('/api/document','PUT',{lease,text,revision:initial.revision});
    assert.equal(duplicate.status,200);assert.deepEqual(await duplicate.json(),saved);
    assert.equal((await api('/api/document','PUT',{lease,text:'stale',revision:initial.revision})).status,409);
    assert.equal((await api('/api/document','PUT',{lease,text:'',revision:saved.revision})).status,200);
    assert.equal(fs.readFileSync(path.join(dataDir,'web.txt.bak'),'utf8'),text);
    assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'');
    const current=await(await api('/api/document')).json();
    fs.writeFileSync(path.join(dataDir,'web.txt'),'外部の変更');
    assert.equal((await api('/api/document','PUT',{lease,text:'overwrite',revision:current.revision})).status,409);
    assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'外部の変更');
    assert.equal((await api('/api/launch','POST',{kind:'web'})).status,200);assert.deepEqual(launches,['web']);
    assert.equal((await api('/api/launch','POST',{kind:'powershell'})).status,404);
    assert.equal((await api('/api/lease','DELETE',{lease})).status,200);
    assert.equal((await api('/api/lease','POST')).status,200);
  }finally{await new Promise(resolve=>server.close(resolve))}
  fs.writeFileSync(path.join(dataDir,'web.txt'),Buffer.from([0xff]));
  const damaged=createEditorServer({port,token,dataDir,launch:async k=>launches.push(k)});
  await new Promise(resolve=>damaged.listen(port,'127.0.0.1',resolve));
  try{
    assert.equal((await api('/api/document')).status,500);
    assert.equal((await api('/api/status')).status,200);
    assert.equal((await api('/api/launch','POST',{kind:'native'})).status,200);
    assert.deepEqual([...fs.readFileSync(path.join(dataDir,'web.txt'))],[255]);
    fs.writeFileSync(path.join(dataDir,'web.txt'),'修復された本文');
    assert.equal((await(await api('/api/document')).json()).text,'修復された本文');
  }finally{await new Promise(resolve=>damaged.close(resolve))}
});
