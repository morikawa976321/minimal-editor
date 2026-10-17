import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {root} from '../scripts/server.mjs';
import {seedNative} from '../scripts/seed-native.mjs';

function fixture(){
  fs.mkdirSync(path.join(root,'.work'),{recursive:true});
  const dir=fs.mkdtempSync(path.join(root,'.work/native-seed-'));
  const source=path.join(dir,'browser'),target=path.join(dir,'native');fs.mkdirSync(path.join(source,'memos'),{recursive:true});
  const other='12345678-1234-4123-8123-123456789abc';
  const index={version:1,lastId:'legacy',memos:[{id:'legacy',createdAt:'2026-10-17T00:00:00.000Z',title:'名前 日本語'},{id:other,createdAt:'2026-10-18T00:00:00.000Z',archived:true}]};
  fs.writeFileSync(path.join(source,'index.json'),JSON.stringify(index));
  fs.writeFileSync(path.join(source,'web.txt'),'原稿\n本文');
  fs.writeFileSync(path.join(source,'memos',other+'.txt'),'アーカイブの本文');
  return {source,target,other,index};
}
test('first snapshot copies active, archived, manual titles without changing browser files',()=>{
  const {source,target,other}=fixture();
  const original=fs.readFileSync(path.join(source,'index.json'));
  assert.equal(seedNative(source,target).created,true);
  assert.equal(fs.readFileSync(path.join(target,'memos/web-legacy.txt'),'utf8'),'原稿\n本文');
  assert.equal(fs.readFileSync(path.join(target,'memos','web-'+other+'.txt'),'utf8'),'アーカイブの本文');
  const catalog=fs.readFileSync(path.join(target,'catalog.tsv'),'utf8');
  assert.ok(catalog.includes('active\tweb-legacy\n'));
  assert.ok(catalog.includes('\t1\t-\n'));
  assert.ok(catalog.includes(Buffer.from('名前 日本語').toString('hex')));
  assert.deepEqual(fs.readFileSync(path.join(source,'index.json')),original);
  fs.writeFileSync(path.join(target,'memos/web-legacy.txt'),'native edit');
  fs.writeFileSync(path.join(source,'web.txt'),'browser edit');
  assert.equal(seedNative(source,target).created,false);
  assert.equal(fs.readFileSync(path.join(target,'memos/web-legacy.txt'),'utf8'),'native edit');
});
test('missing or invalid UTF8 indexed memo refuses publication',()=>{
  for(const invalid of [false,true]){
    const {source,target}=fixture();
    if(invalid)fs.writeFileSync(path.join(source,'web.txt'),Buffer.from([255]));else fs.unlinkSync(path.join(source,'web.txt'));
    assert.throws(()=>seedNative(source,target));assert.equal(fs.existsSync(target),false);
  }
});
test('invalid metadata and unrelated native folder are retained',()=>{
  const {source,target,index}=fixture();
  index.memos[0].id='../escape';fs.writeFileSync(path.join(source,'index.json'),JSON.stringify(index));
  assert.throws(()=>seedNative(source,target));assert.equal(fs.existsSync(target),false);
  fs.mkdirSync(target);fs.writeFileSync(path.join(target,'important.txt'),'keep');
  assert.throws(()=>seedNative(source,target));
  assert.equal(fs.readFileSync(path.join(target,'important.txt'),'utf8'),'keep');
});
test('all archived snapshot preserves null active; fresh browser gives blank independent memo',()=>{
  const {source,target,index}=fixture();index.lastId=null;index.memos.forEach(m=>m.archived=true);
  fs.writeFileSync(path.join(source,'index.json'),JSON.stringify(index));seedNative(source,target);
  assert.ok(fs.readFileSync(path.join(target,'catalog.tsv'),'utf8').includes('active\t-\n'));
  const blankSource=path.join(path.dirname(source),'missing'),blankTarget=path.join(path.dirname(target),'blank');
  seedNative(blankSource,blankTarget);
  assert.equal(fs.readFileSync(path.join(blankTarget,'memos/web-legacy.txt'),'utf8'),'');
  assert.equal(fs.existsSync(blankSource),false);
});
