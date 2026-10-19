import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const cli=path.join(root,'scripts/memos.mjs');
const run=(...args)=>spawnSync(process.execPath,[cli,...args],{encoding:'utf8'});
const hex=text=>Buffer.from(text,'utf8').toString('hex');

function fixture(){
  fs.mkdirSync(path.join(root,'.work'),{recursive:true});
  const dir=fs.mkdtempSync(path.join(root,'.work/memos-cli-'));
  fs.mkdirSync(path.join(dir,'memos'));
  const memos={a1:'\n  派生タイトル  \r\n本文\n',b2:'hello',c3:'old note\n',d4:''};
  for(const [id,text] of Object.entries(memos))fs.writeFileSync(path.join(dir,'memos',`${id}.txt`),text);
  fs.writeFileSync(path.join(dir,'catalog.tsv'),['minimal-editor-native-v1','active\ta1',
    'memo\ta1\t1\t0\t-',`memo\tb2\t2\t0\t${hex('原稿 Draft')}`,'memo\tc3\t3\t1\t-','memo\td4\t4\t0\t-',''].join('\n'));
  return dir;
}
const snapshot=dir=>fs.readdirSync(dir,{recursive:true}).sort().map(name=>{
  const file=path.join(dir,name);
  return fs.statSync(file).isFile()?name+':'+crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'):name;
});

test('titles match the native store: manual hex, derived first line, empty as 無題',()=>{
  const dir=fixture(),out=run('list','--all','--data-dir',dir);
  assert.equal(out.status,0);
  const titles=out.stdout.trim().split('\n').map(line=>line.split('\t')[3]).sort();
  assert.deepEqual(titles,['* 派生タイトル','[保管] old note','原稿 Draft','無題']);
  assert.equal(run('list','--data-dir',dir).stdout.trim().split('\n').length,3);
  assert.equal(run('list','--archived','--data-dir',dir).stdout.trim().split('\n').length,1);
});

test('show resolves ID, exact title, then substring; ambiguity exits 2 without guessing',()=>{
  const dir=fixture();
  assert.match(run('show','b2','--data-dir',dir).stdout,/# 原稿 Draft\n[^\n]*\n\nhello\n$/);
  assert.match(run('show','draft','--data-dir',dir).stdout,/hello/);
  assert.equal(run('path','派生','--data-dir',dir).stdout.trim(),path.join(dir,'memos','a1.txt'));
  const many=run('show','t','--data-dir',dir);
  assert.equal(many.status,2);
  assert.match(many.stderr,/候補が2件/);
  assert.equal(run('show','zzz','--data-dir',dir).status,1);
});

test('lookups never touch the store, including the editor lock',()=>{
  const dir=fixture(),before=snapshot(dir);
  for(const args of [['list','--all'],['show','b2'],['path','a1'],['show','zzz']])run(...args,'--data-dir',dir);
  assert.deepEqual(snapshot(dir),before);
  assert.equal(fs.existsSync(path.join(dir,'.store.lock')),false);
});
