import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const utf8=bytes=>new TextDecoder('utf-8',{fatal:true}).decode(bytes);
const validId=id=>id==='legacy'||/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id);
const validTitle=title=>typeof title==='string'&&title.trim()===title&&title.length>0&&title.length<=200&&!/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(title);
function write(file,bytes){const fd=fs.openSync(file,'wx');try{fs.writeFileSync(fd,bytes);fs.fsyncSync(fd)}finally{fs.closeSync(fd)}}

// A one-time snapshot, never a shared store or ongoing synchronization.
export function seedNative(sourceDir=path.join(root,'data'),targetDir=path.join(root,'data/native-next')){
  sourceDir=path.resolve(sourceDir);targetDir=path.resolve(targetDir);
  if(sourceDir===targetDir)throw Error('Native data must be separate from browser data');
  if(fs.existsSync(targetDir)){
    if(!fs.existsSync(path.join(targetDir,'catalog.tsv')))throw Error('Native data folder already exists without a catalog; refusing to replace it');
    return {created:false,targetDir};
  }
  const indexFile=path.join(sourceDir,'index.json'),inputs=new Map();
  const read=file=>{const bytes=fs.readFileSync(file);inputs.set(file,bytes);return utf8(bytes)};
  let index;
  if(fs.existsSync(indexFile)){
    index=JSON.parse(read(indexFile));
    if(index.version!==1||!Array.isArray(index.memos)||index.memos.length===0)throw Error('Invalid browser memo index');
  }else{
    if(fs.existsSync(indexFile+'.bak')||fs.existsSync(indexFile+'.tmp'))throw Error('Browser memo index needs recovery');
    const legacy=path.join(sourceDir,'web.txt');
    if(!fs.existsSync(legacy)&&(fs.existsSync(legacy+'.bak')||fs.existsSync(legacy+'.tmp')))throw Error('Browser memo needs recovery');
    const created=fs.existsSync(legacy)?fs.statSync(legacy).birthtimeMs:Date.now();
    index={version:1,lastId:'legacy',memos:[{id:'legacy',createdAt:new Date(created).toISOString()}]};
  }
  const ids=new Set();
  const memos=index.memos.map(memo=>{
    if(!memo||typeof memo.id!=='string'||!validId(memo.id)||ids.has(memo.id)||typeof memo.createdAt!=='string'||!Number.isFinite(Date.parse(memo.createdAt)))throw Error('Invalid browser memo metadata');
    if(memo.title!==undefined&&!validTitle(memo.title))throw Error('Invalid browser memo title');
    if(memo.archived!==undefined&&typeof memo.archived!=='boolean')throw Error('Invalid browser archive flag');
    ids.add(memo.id);
    const file=path.join(sourceDir,memo.id==='legacy'?'web.txt':`memos/${memo.id}.txt`);
    const text=!fs.existsSync(indexFile)&&!fs.existsSync(file)?'':read(file);
    return {...memo,id:'web-'+memo.id,text,created:Math.max(0,Math.floor(Date.parse(memo.createdAt)))};
  });
  if(!ids.has('legacy'))throw Error('Browser legacy memo missing');
  const active=memos.filter(m=>!m.archived);
  if(active.length?!active.some(m=>m.id==='web-'+index.lastId):index.lastId!==null)throw Error('Invalid browser active memo');
  // The browser may still be open. Reject observed changes. This is a best-effort
  // copy, not a cross-file transaction or ongoing synchronization with the browser.
  for(const [file,bytes] of inputs)if(!fs.readFileSync(file).equals(bytes))throw Error('Browser memos changed during snapshot; retry');
  if(!inputs.has(indexFile)&&fs.existsSync(indexFile))throw Error('Browser memo index appeared during snapshot; retry');
  const parent=path.dirname(targetDir);fs.mkdirSync(parent,{recursive:true});
  const stage=path.join(parent,'.native-seed-'+crypto.randomUUID());
  fs.mkdirSync(stage);
  let published=false;
  try{
    fs.mkdirSync(path.join(stage,'memos'));
    for(const memo of memos)write(path.join(stage,'memos',memo.id+'.txt'),memo.text);
    const catalog=['minimal-editor-native-v1','active\t'+(index.lastId===null?'-':'web-'+index.lastId),...memos.map(m=>['memo',m.id,m.created,m.archived?1:0,m.title===undefined?'-':Buffer.from(m.title,'utf8').toString('hex')].join('\t'))].join('\n')+'\n';
    write(path.join(stage,'catalog.tsv'),catalog);
    // Renaming a complete directory publishes no half-written catalog.
    // Competing launchers may have won; never merge or overwrite their store.
    if(fs.existsSync(targetDir)){
      if(!fs.existsSync(path.join(targetDir,'catalog.tsv')))throw Error('Native folder appeared without catalog');
      return {created:false,targetDir};
    }
    try{fs.renameSync(stage,targetDir);published=true}
    catch(error){if(!fs.existsSync(path.join(targetDir,'catalog.tsv')))throw error}
    return {created:published,targetDir,memos:memos.length};
  }finally{
    if(!published&&fs.existsSync(stage))fs.rmSync(stage,{recursive:true});
  }
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const option=name=>{const at=process.argv.indexOf(name);return at<0?undefined:process.argv[at+1]};
  console.log(JSON.stringify(seedNative(option('--source-dir'),option('--target-dir'))));
}
