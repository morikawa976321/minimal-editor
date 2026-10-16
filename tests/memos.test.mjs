import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createEditorServer,root} from '../scripts/server.mjs';
import {createMemoStore} from '../scripts/memo-store.mjs';

function directory(){fs.mkdirSync(path.join(root,'.work'),{recursive:true});return fs.mkdtempSync(path.join(root,'.work/memos-test-'))}
test('archive preserves every body, backup and history; empty state and restore survive restart',()=>{
  const dataDir=directory();let time=2000000000000;
  let store=createMemoStore(dataDir,{now:()=>time});
  const old=store.get('legacy');store.save(old.id,{revision:old.revision,text:'first draft'});
  const first=store.get(old.id);store.save(old.id,{revision:first.revision,text:'legacy body'});store.rename(old.id,'manual title');
  const a=store.create();time++;const b=store.create();
  store.save(a.id,{revision:a.revision,text:'A body'});store.save(b.id,{revision:b.revision,text:'B body'});
  const before=store.list();
  function files(folder){return fs.readdirSync(folder,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?files(path.join(folder,entry.name)):[path.join(folder,entry.name)])}
  const snapshots=new Map(files(dataDir).filter(file=>!path.basename(file).startsWith('index.json')).map(file=>[file,fs.readFileSync(file)]));
  let result=store.archive(a.id,true);assert.equal(result.lastId,'legacy');assert.equal(result.current.text,'legacy body');
  assert.deepEqual(result.memos.map(m=>m.id),[b.id,'legacy']);assert.equal(result.archivedMemos[0].id,a.id);
  const indexPath=path.join(dataDir,'index.json'),raw=fs.readFileSync(indexPath,'utf8');
  assert.deepEqual(store.archive(a.id,true),result);assert.equal(fs.readFileSync(indexPath,'utf8'),raw);
  for(const action of [()=>store.save(a.id,{revision:a.revision,text:'bad'}),()=>store.rename(a.id,'bad'),()=>store.activate(a.id)])assert.throws(action,{status:409});
  assert.equal(store.get(a.id).text,'A body');
  result=store.archive('legacy',true);assert.equal(result.lastId,b.id);assert.equal(result.current.text,'B body');
  result=store.archive(b.id,true);assert.equal(result.current,null);assert.equal(result.lastId,null);assert.deepEqual(result.memos,[]);
  store=createMemoStore(dataDir,{now:()=>time});assert.equal(store.list().lastId,null);assert.equal(store.list().archivedMemos.length,3);
  result=store.archive(a.id,false);assert.equal(result.lastId,a.id);assert.equal(result.current.text,'A body');
  result=store.archive('legacy',false);assert.equal(result.lastId,a.id);assert.equal(result.memos.find(m=>m.id==='legacy').title,'manual title');
  for(const memo of result.memos)assert.equal(memo.createdAt,before.memos.find(m=>m.id===memo.id).createdAt);
  store.archive(a.id,true);store.archive('legacy',true);
  const created=store.create();assert.equal(store.list().lastId,created.id);
  for(const [file,bytes] of snapshots)assert.deepEqual(fs.readFileSync(file),bytes);
});

test('archive HTTP requires lease, validates input and blocks legacy writes',async()=>{
  const dataDir=directory(),port=43140,token='archive-test',server=createEditorServer({port,token,dataDir});
  await new Promise((ok,no)=>{server.once('error',no);server.listen(port,'127.0.0.1',ok)});
  const api=(route,method='GET',body)=>fetch(`http://127.0.0.1:${port}/api/${route}`,{method,headers:{Authorization:'Bearer '+token,'Content-Type':'application/json',Connection:'close'},body:body===undefined?undefined:JSON.stringify(body)});
  try{
    const memo=await(await api('memos/legacy')).json();
    assert.equal((await api('memos/legacy/archive','PUT',{archived:true})).status,409);
    const {lease}=await(await api('lease','POST')).json();
    for(const archived of [null,0,'true'])assert.equal((await api('memos/legacy/archive','PUT',{lease,archived})).status,400);
    const archived=await api('memos/legacy/archive','PUT',{lease,archived:true});assert.equal(archived.status,200);assert.equal((await archived.json()).current,null);
    assert.equal((await api('document','PUT',{lease,text:'bad',revision:memo.revision})).status,409);
    assert.equal((await api('memos/legacy/title','PUT',{lease,title:'bad'})).status,409);
    assert.equal((await api('active','PUT',{lease,id:'legacy'})).status,409);
    const restored=await(await api('memos/legacy/archive','PUT',{lease,archived:false})).json();assert.equal(restored.current.id,'legacy');
  }finally{await new Promise(ok=>server.close(ok))}
});

test('archive validates next current before commit and invalid stored states fail closed',()=>{
  const dataDir=directory(),store=createMemoStore(dataDir);store.get('legacy');const a=store.create();
  const indexPath=path.join(dataDir,'index.json'),raw=fs.readFileSync(indexPath,'utf8');
  fs.unlinkSync(path.join(dataDir,'memos',a.id+'.txt'));
  assert.throws(()=>createMemoStore(dataDir).archive('legacy',true),{code:'ENOENT'});
  assert.equal(fs.readFileSync(indexPath,'utf8'),raw);
  const value=JSON.parse(raw);
  for(const malformed of [{...value,lastId:null},{...value,memos:value.memos.map(m=>({...m,archived:true}))},{...value,memos:value.memos.map(m=>({...m,archived:'yes'}))}]){
    fs.writeFileSync(indexPath,JSON.stringify(malformed));assert.throws(()=>createMemoStore(dataDir).list(),/Invalid memo/);
  }
});
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
    assert.equal((await(await api('status')).json()).protocol,4);
    const list=await(await api('memos')).json();assert.equal(list.lastId,'legacy');assert.equal(list.memos[0].title,'元の本文');
    assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'元の本文');assert.equal(fs.readFileSync(path.join(dataDir,'web.txt.bak'),'utf8'),'前の本文');
    assert.equal((await api('memos','POST',{})).status,409);
    const {lease}=await(await api('lease','POST')).json();
    assert.equal((await api('memos/legacy/title','PUT',{title:'denied'})).status,409);
    const original=await(await api('memos/legacy')).json();
    const created=await(await api('memos','POST',{lease})).json();assert.equal(created.text,'');assert.ok(created.id!=='legacy');
    const saved=await api(`memos/${created.id}`,'PUT',{lease,text:'\n新しいメモ\n二行目',revision:created.revision});assert.equal(saved.status,200);
    assert.equal((await api(`memos/${created.id}`,'PUT',{lease,text:'\n新しいメモ\n二行目',revision:created.revision})).status,200);
    assert.equal((await api(`memos/${created.id}`,'PUT',{lease,text:'古い変更',revision:created.revision})).status,409);
    assert.equal((await(await api('document')).json()).text,'元の本文');
    assert.equal((await api('active','PUT',{lease,id:created.id})).status,200);
    const orderBefore=await(await api('memos')).json();
    for(const title of ['', '  ', null, 42, 'x'.repeat(201), 'a\nb', 'a\tb', '\nvalid', 'bad\u007f', 'bad\u0085', 'bad\u2028'])assert.equal((await api('memos/legacy/title','PUT',{lease,title})).status,400);
    const renamed=await api('memos/legacy/title','PUT',{lease,title:'  独立した名前  '});
    assert.equal(renamed.status,200);assert.deepEqual(await renamed.json(),{id:'legacy',title:'独立した名前'});
    const indexAfter=fs.readFileSync(path.join(dataDir,'index.json'),'utf8');
    assert.equal((await api('memos/legacy/title','PUT',{lease,title:'独立した名前'})).status,200);
    assert.equal(fs.readFileSync(path.join(dataDir,'index.json'),'utf8'),indexAfter,'retry leaves metadata unchanged');
    assert.deepEqual(await(await api('memos/legacy')).json(),original,'body and revision unchanged');
    const orderAfter=await(await api('memos')).json();
    assert.equal(orderAfter.lastId,orderBefore.lastId);
    assert.deepEqual(orderAfter.memos.map(({id,createdAt})=>({id,createdAt})),orderBefore.memos.map(({id,createdAt})=>({id,createdAt})));
    assert.equal((await api(`memos/${created.id}/title`,'PUT',{lease,title:'独立した名前'})).status,200,'duplicate titles allowed');
    assert.equal((await api('memos/%2e%2e%2fweb','GET')).status,404);
    assert.equal((await api('active','PUT',{lease,id:'../../web'})).status,404);
    assert.equal((await api('recovery','POST',{memoId:created.id,text:'復元する本文'})).status,200);
    assert.ok(fs.readdirSync(dataDir).some(n=>n.startsWith(created.id+'-recovery-')));
    await new Promise(ok=>server.close(ok));await start();
    const after=await(await api('memos')).json();assert.equal(after.lastId,created.id);assert.equal(after.memos[0].id,created.id);assert.equal(after.memos[0].title,'独立した名前');assert.equal(after.memos[1].title,'独立した名前');
    assert.deepEqual(await(await api('memos/legacy')).json(),original);
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
  assert.throws(()=>store.rename('legacy','change'),/outside editor/);
  assert.equal(fs.readFileSync(path.join(dataDir,'web.txt'),'utf8'),'keep');
});

test('persisted manual titles are validated before use',()=>{
  const dataDir=directory(),store=createMemoStore(dataDir);store.get('legacy');
  const indexPath=path.join(dataDir,'index.json'),index=JSON.parse(fs.readFileSync(indexPath,'utf8'));
  for(const title of ['',null,42,' padded ','bad\nname','a'.repeat(201)]){
    index.memos[0].title=title;fs.writeFileSync(indexPath,JSON.stringify(index));
    assert.throws(()=>createMemoStore(dataDir).list(),/Invalid memo title/);
  }
});
