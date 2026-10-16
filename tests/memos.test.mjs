import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createEditorServer,root} from '../scripts/server.mjs';
import {createMemoStore} from '../scripts/memo-store.mjs';

function directory(){fs.mkdirSync(path.join(root,'.work'),{recursive:true});return fs.mkdtempSync(path.join(root,'.work/memos-test-'))}
test('initial legacy file is durable and a missing indexed file is never recreated',()=>{
  const dataDir=directory(),file=path.join(dataDir,'web.txt');
  const store=createMemoStore(dataDir),initial=store.get('legacy');
  assert.equal(initial.text,'');assert.equal(fs.readFileSync(file,'utf8'),'');
  assert.ok(fs.existsSync(path.join(dataDir,'index.json')));
  fs.unlinkSync(file);
  assert.throws(()=>store.save('legacy',{revision:initial.revision,text:'replacement'}),{code:'ENOENT'});
  const restarted=createMemoStore(dataDir);
  assert.throws(()=>restarted.get('legacy'),{code:'ENOENT'});
  assert.throws(()=>restarted.list(),{code:'ENOENT'});
  assert.equal(fs.existsSync(file),false);
  const oldData=directory();fs.writeFileSync(path.join(oldData,'web.txt.bak'),'recover this');
  assert.throws(()=>createMemoStore(oldData).get('legacy'),/recovery required/);
  assert.equal(fs.existsSync(path.join(oldData,'web.txt')),false);
  assert.equal(fs.readFileSync(path.join(oldData,'web.txt.bak'),'utf8'),'recover this');
});
test('memo persistence, legacy preservation, conflicts, last opened, and HTTP authorization',async()=>{
  const dataDir=directory(),port=43139,token='memo-test';
  fs.writeFileSync(path.join(dataDir,'web.txt'),'元の本文');fs.writeFileSync(path.join(dataDir,'web.txt.bak'),'前の本文');
  let server;
  async function start(){server=createEditorServer({port,token,dataDir});await new Promise((ok,no)=>{server.once('error',no);server.listen(port,'127.0.0.1',ok)})}
  const api=(route,method='GET',body)=>fetch(`http://127.0.0.1:${port}/api/${route}`,{method,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Connection:'close'},body:body===undefined?undefined:JSON.stringify(body)});
  await start();
  try{
    assert.equal((await(await api('status')).json()).protocol,2);
    const list=await(await api('memos')).json();assert.equal(list.lastId,'legacy');assert.equal(list.memos[0].title,'元の本文');
    assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'元の本文');assert.equal(fs.readFileSync(path.join(dataDir,'web.txt.bak'),'utf8'),'前の本文');
    assert.equal((await api('memos','POST',{})).status,409);
    const {lease}=await(await api('lease','POST')).json();
    const created=await(await api('memos','POST',{lease})).json();assert.equal(created.text,'');assert.ok(created.id!=='legacy');
    const saved=await api(`memos/${created.id}`,'PUT',{lease,text:'\n新しいメモ\n二行目',revision:created.revision});assert.equal(saved.status,200);
    assert.equal((await api(`memos/${created.id}`,'PUT',{lease,text:'\n新しいメモ\n二行目',revision:created.revision})).status,200);
    assert.equal((await api(`memos/${created.id}`,'PUT',{lease,text:'古い変更',revision:created.revision})).status,409);
    assert.equal((await(await api('document')).json()).text,'元の本文');
    assert.equal((await api('active','PUT',{lease,id:created.id})).status,200);
    assert.equal((await api('memos/%2e%2e%2fweb','GET')).status,404);
    assert.equal((await api('active','PUT',{lease,id:'../../web'})).status,404);
    assert.equal((await api('recovery','POST',{memoId:created.id,text:'復元する本文'})).status,200);
    assert.ok(fs.readdirSync(dataDir).some(n=>n.startsWith(created.id+'-recovery-')));
    await new Promise(ok=>server.close(ok));await start();
    const after=await(await api('memos')).json();assert.equal(after.lastId,created.id);assert.equal(after.memos[0].id,created.id);assert.equal(after.memos[0].title,'新しいメモ');
  }finally{await new Promise(ok=>server.close(ok))}
});

test('creation order, history cadence, restart checkpoint and external edits',()=>{
  const dataDir=directory();let time=2000000000000;
  fs.writeFileSync(path.join(dataDir,'web.txt'),'original');
  const store=createMemoStore(dataDir,{now:()=>time});const old=store.get('legacy');
  const a=store.create();time+=1;const b=store.create();
  const before=store.list().memos.map(m=>m.id);
  store.save(a.id,{revision:a.revision,text:'first'});assert.deepEqual(store.list().memos.map(m=>m.id),before);
  assert.equal(before[0],b.id);
  let saved=store.save('legacy',{revision:old.revision,text:'one'});
  time+=30000;saved=store.save('legacy',{revision:saved.revision,text:'two'});
  const history=path.join(dataDir,'history/legacy');assert.equal(fs.readdirSync(history).length,1);
  assert.equal(fs.readFileSync(path.join(history,fs.readdirSync(history)[0]),'utf8'),'original');
  time+=30000;saved=store.save('legacy',{revision:saved.revision,text:'three'});assert.equal(fs.readdirSync(history).length,2);
  const restarted=createMemoStore(dataDir,{now:()=>time+1});restarted.save('legacy',{revision:saved.revision,text:'four'});
  assert.equal(fs.readdirSync(history).length,3);
  const latest=restarted.get('legacy');fs.writeFileSync(path.join(dataDir,'web.txt'),'external');
  assert.throws(()=>restarted.save('legacy',{revision:latest.revision,text:'overwrite'}),/outside editor/);
  assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'external');
});

test('malformed or externally changed index and failed checkpoint cannot overwrite content',()=>{
  const dataDir=directory();fs.writeFileSync(path.join(dataDir,'web.txt'),'keep');
  fs.writeFileSync(path.join(dataDir,'index.json'),'{broken');
  assert.throws(()=>createMemoStore(dataDir).list());assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'keep');
  fs.unlinkSync(path.join(dataDir,'index.json'));
  const store=createMemoStore(dataDir),memo=store.get('legacy');
  fs.writeFileSync(path.join(dataDir,'history'),'blocking file');
  assert.throws(()=>store.save('legacy',{revision:memo.revision,text:'must not save'}));
  assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'keep');
  fs.appendFileSync(path.join(dataDir,'index.json'),' ');
  assert.throws(()=>store.create(),/outside editor/);
  assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'keep');
});
