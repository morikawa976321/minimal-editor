import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {seedNative} from '../scripts/seed-native.mjs';
import {adoptNative} from '../scripts/adopt-native.mjs';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const exe=path.join(root,'build/native-adopt.exe');
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
function tree(dir,prefix=''){
  return Object.fromEntries(fs.readdirSync(path.join(dir,prefix),{withFileTypes:true}).flatMap(entry=>{
    const name=path.join(prefix,entry.name);return entry.isDirectory()?Object.entries(tree(dir,name)):[[name,hash(path.join(dir,name))]];
  }));
}
test('one-time adoption preserves browser and both differing variants, backs up native, and maps current memo',()=>{
  assert.ok(fs.existsSync(exe),'Build native-adopt.exe before running this test');
  fs.mkdirSync(path.join(root,'.work'),{recursive:true});
  const dir=fs.mkdtempSync(path.join(root,'.work/native-adopt-e2e-'));
  const source=path.join(dir,'browser'),target=path.join(dir,'native'),work=path.join(dir,'work');
  fs.mkdirSync(path.join(source,'memos'),{recursive:true});
  const memos=Array.from({length:15},(_,i)=>({id:i===0?'legacy':crypto.randomUUID(),createdAt:'2026-10-18T00:00:00.000Z'}));
  const fileFor=id=>path.join(source,id==='legacy'?'web.txt':`memos/${id}.txt`);
  for(const memo of memos)fs.writeFileSync(fileFor(memo.id),`private fixture ${memo.id}`);
  fs.writeFileSync(path.join(source,'index.json'),JSON.stringify({version:1,lastId:memos[14].id,memos}));
  seedNative(source,target);
  const originalNative=tree(target),changedId=memos[14].id,originalVariant=fs.readFileSync(fileFor(changedId),'utf8');
  fs.writeFileSync(fileFor(changedId),originalVariant+' browser latest');
  const beforeBrowser=tree(source);
  const report=adoptNative({sourceDir:source,targetDir:target,workDir:work,exe});
  assert.deepEqual({before:report.beforeCount,after:report.afterCount,kept:report.kept,imported:report.imported,conflicts:report.conflicts},{before:15,after:16,kept:14,imported:1,conflicts:1});
  assert.deepEqual(tree(source),beforeBrowser);
  assert.deepEqual(tree(report.backupDir),originalNative);
  const mapped=report.mapping['web-'+changedId];assert.equal(mapped,report.active);
  assert.equal(fs.readFileSync(path.join(target,'memos',mapped+'.txt'),'utf8'),originalVariant+' browser latest');
  assert.equal(fs.readFileSync(path.join(target,'memos','web-'+changedId+'.txt'),'utf8'),originalVariant);
  const old=fs.readFileSync(path.join(target,'catalog.tsv'),'utf8').split('\n').find(line=>line.startsWith('memo\tweb-'+changedId+'\t')).split('\t');
  assert.equal(old[3],'1');assert.ok(Buffer.from(old[4],'hex').toString('utf8').endsWith('（移行前の比較版）'));
  assert.ok(!JSON.stringify(report).includes('private fixture'));
  assert.throws(()=>adoptNative({sourceDir:source,targetDir:target,workDir:work,exe}),/already completed/);
});
