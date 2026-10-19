#!/usr/bin/env node
// Read-only lookup of native-next memos for AI sessions (Claude Code etc.).
// Never writes and never takes `.store.lock`: the running editor treats any
// outside change to catalog.tsv or memos/*.txt as a conflict and stops saving.
//
//   node scripts/memos.mjs list [--archived|--all] [--data-dir DIR]
//   node scripts/memos.mjs show <ID|title> [--data-dir DIR]
//   node scripts/memos.mjs path <ID|title> [--data-dir DIR]
//
// <title> matches an exact ID, then an exact title, then a case-insensitive
// title substring. More than one match lists the candidates and exits 2.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const HEADER='minimal-editor-native-v1';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const decoder=new TextDecoder('utf-8',{fatal:true});
const readText=file=>decoder.decode(fs.readFileSync(file));
const fail=(message,code=1)=>{process.stderr.write(message+'\n');process.exit(code)};

const args=process.argv.slice(2);
const option=name=>{const i=args.indexOf(name);if(i<0)return;const v=args[i+1];if(!v)fail(`${name} needs a value`);args.splice(i,2);return v};
const flag=name=>{const i=args.indexOf(name);if(i<0)return false;args.splice(i,1);return true};
const dataDir=path.resolve(option('--data-dir')??path.join(root,'data/native-next'));
const archivedOnly=flag('--archived'),all=flag('--all');
const [command,...rest]=args;
const query=rest.join(' ').trim();

// Mirrors store.rs derived_title: first nonempty trimmed line, controls removed, 80 chars.
function derivedTitle(text){
  for(const line of text.split('\n')){
    const title=Array.from(line.replace(/\r$/,'').trim().replace(/\p{Cc}/gu,'')).slice(0,80).join('');
    if(line.trim())return title||'無題';
  }
  return '無題';
}
const decodeHex=hex=>decoder.decode(Uint8Array.from(hex.match(/../g),b=>parseInt(b,16)));

function load(){
  let raw;
  try{raw=readText(path.join(dataDir,'catalog.tsv'))}catch(e){fail(`一覧を読めません: ${dataDir} (${e.code??e.message})`)}
  const lines=raw.split('\n').filter(Boolean);
  if(lines[0]!==HEADER)fail('一覧の形式が想定と違います');
  const active=lines[1]?.replace(/^active\t/,'');
  return lines.slice(2).map(line=>{
    const [,id,created,archived,title]=line.split('\t');
    const file=path.join(dataDir,'memos',`${id}.txt`);
    let text='',updated=null;
    try{text=readText(file);updated=fs.statSync(file).mtime}catch{}
    return {id,file,text,updated,created:new Date(Number(created)),archived:archived==='1',active:id===active,
      title:title&&title!=='-'?decodeHex(title):derivedTitle(text)};
  });
}

const stamp=d=>d?new Date(d.getTime()-d.getTimezoneOffset()*6e4).toISOString().slice(0,16).replace('T',' '):'----------------';
const row=m=>`${m.id}\t${stamp(m.updated)}\t${Array.from(m.text).length}字\t${m.archived?'[保管] ':''}${m.active?'* ':''}${m.title}`;

function find(memos){
  if(!query)fail('ID かタイトルを指定してください');
  const lower=query.toLowerCase();
  for(const test of [m=>m.id===query,m=>m.title===query,m=>m.title.toLowerCase().includes(lower)]){
    const hits=memos.filter(test);
    if(hits.length===1)return hits[0];
    if(hits.length>1)fail(`候補が${hits.length}件あります:\n`+hits.map(row).join('\n'),2);
  }
  fail(`見つかりません: ${query}`);
}

const memos=load().sort((a,b)=>(b.updated??0)-(a.updated??0));
if(command==='list'){
  const shown=memos.filter(m=>all||m.archived===archivedOnly);
  process.stdout.write(shown.map(row).join('\n')+(shown.length?'\n':''));
}else if(command==='show'){
  const m=find(memos);
  process.stdout.write(`# ${m.title}\n# ${m.id} · 更新 ${stamp(m.updated)} · ${m.file}\n\n${m.text}${m.text.endsWith('\n')?'':'\n'}`);
}else if(command==='path'){
  process.stdout.write(find(memos).file+'\n');
}else fail('usage: memos.mjs list [--archived|--all] | show <ID|title> | path <ID|title> [--data-dir DIR]');
