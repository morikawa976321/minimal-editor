import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

export const hash=text=>crypto.createHash('sha256').update(text).digest('hex');
export function readText(file,{missing=false}={}){
  try{return new TextDecoder('utf-8',{fatal:true}).decode(fs.readFileSync(file))}
  catch(error){if(missing&&error.code==='ENOENT')return '';throw error}
}
export function durableWrite(file,text){
  const temporary=file+'.tmp';
  const fd=fs.openSync(temporary,'w');
  try{fs.writeFileSync(fd,text,'utf8');fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
  if(fs.existsSync(file)){
    fs.copyFileSync(file,file+'.bak.tmp');
    const backup=fs.openSync(file+'.bak.tmp','r+');try{fs.fsyncSync(backup)}finally{fs.closeSync(backup)}
    fs.renameSync(file+'.bak.tmp',file+'.bak');
  }
  fs.renameSync(temporary,file);
}
function fault(status,message){return Object.assign(Error(message),{status})}
const validId=id=>typeof id==='string'&&(id==='legacy'||/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id));
const validTitle=title=>typeof title==='string'&&title.length>0&&title.length<=200&&title===title.trim()&&!/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(title);

export function createMemoStore(dataDir,{now=Date.now}={}){
  const indexFile=path.join(dataDir,'index.json'),cache=new Map(),checkpoints=new Map();
  let index,indexRaw;
  const fileFor=id=>path.join(dataDir,id==='legacy'?'web.txt':`memos/${id}.txt`);
  function persist(next){
    if(indexRaw!==undefined&&readText(indexFile)!==indexRaw)throw fault(409,'Memo index changed outside editor');
    const raw=JSON.stringify(next,null,2)+'\n';durableWrite(indexFile,raw);indexRaw=raw;index=next;
  }
  function ensureIndex(){
    if(index){if(readText(indexFile)!==indexRaw)throw fault(409,'Memo index changed outside editor');return}
    if(!fs.existsSync(indexFile)){
      if(fs.existsSync(indexFile+'.bak')||fs.existsSync(indexFile+'.tmp'))throw Error('Memo index missing; recovery required');
      const file=fileFor('legacy');
      const stat=fs.existsSync(file)?fs.statSync(file):null;
      if(!stat){
        if(fs.existsSync(file+'.bak')||fs.existsSync(file+'.tmp'))throw Error('Legacy memo missing; recovery required');
        // Publish an empty initial file before the index. Once indexed, a missing
        // file is an error rather than a new blank document (including on restart).
        const fd=fs.openSync(file,'wx');try{fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
      }
      persist({version:1,lastId:'legacy',memos:[{id:'legacy',createdAt:new Date(stat?(stat.birthtimeMs||stat.mtimeMs):now()).toISOString()}]});return;
    }
    const raw=readText(indexFile),value=JSON.parse(raw);
    if(value.version!==1||!Array.isArray(value.memos)||!value.memos.length||!validId(value.lastId))throw Error('Invalid memo index');
    const ids=new Set();
    for(const memo of value.memos){
      if(!memo||!validId(memo.id)||typeof memo.createdAt!=='string'||!Number.isFinite(Date.parse(memo.createdAt))||ids.has(memo.id))throw Error('Invalid memo index');
      if(memo.title!==undefined&&!validTitle(memo.title))throw Error('Invalid memo title');
      ids.add(memo.id);
    }
    if(!ids.has('legacy')||!ids.has(value.lastId))throw Error('Invalid memo index');
    index=value;indexRaw=raw;
  }
  function requireMemo(id){ensureIndex();if(!validId(id)||!index.memos.some(m=>m.id===id))throw fault(404,'Memo not found')}
  function get(id){
    requireMemo(id);
    if(!cache.has(id)){const text=readText(fileFor(id));cache.set(id,{text,revision:hash(text)})}
    return {id,...cache.get(id)};
  }
  function checkpoint(id,text){
    const time=now(),previous=checkpoints.get(id);
    if(previous!==undefined&&time-previous<60000)return;
    const directory=path.join(dataDir,'history',id);fs.mkdirSync(directory,{recursive:true});
    const file=path.join(directory,`${time}-${hash(text)}.txt`);
    if(fs.existsSync(file)){
      if(readText(file)!==text)throw Error('History checkpoint conflict');
      const fd=fs.openSync(file,'r+');try{fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
    }
    else{const fd=fs.openSync(file,'wx');try{fs.writeFileSync(fd,text,'utf8');fs.fsyncSync(fd)}finally{fs.closeSync(fd)}}
    checkpoints.set(id,time);
  }
  return {
    get,
    list(){ensureIndex();return {lastId:index.lastId,memos:index.memos.map(m=>{const text=get(m.id).text;return {...m,title:m.title??(text.split(/\r?\n/).find(line=>line.trim())?.trim().slice(0,80)||'無題')}}).sort((a,b)=>Date.parse(b.createdAt)-Date.parse(a.createdAt)||a.id.localeCompare(b.id))}},
    rename(id,title){
      requireMemo(id);
      if(typeof title!=='string'||/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(title)||!validTitle(title.trim()))throw fault(400,'Invalid memo title');
      title=title.trim();
      if(index.memos.find(m=>m.id===id).title!==title)persist({...index,memos:index.memos.map(m=>m.id===id?{...m,title}:m)});
      return {id,title};
    },
    create(){
      ensureIndex();fs.mkdirSync(path.join(dataDir,'memos'),{recursive:true});
      const id=crypto.randomUUID(),file=fileFor(id),fd=fs.openSync(file,'wx');
      try{fs.fsyncSync(fd)}finally{fs.closeSync(fd)}
      // The complete text exists before its index entry is published. An interrupted
      // creation can leave an orphan, but cannot replace any existing memo.
      persist({...index,memos:[...index.memos,{id,createdAt:new Date(now()).toISOString()}]});
      return get(id);
    },
    activate(id){requireMemo(id);get(id);if(index.lastId!==id)persist({...index,lastId:id})},
    save(id,body){
      if(typeof body.text!=='string')throw fault(400,'Text required');
      const current=get(id),file=fileFor(id);
      if(readText(file)!==current.text)throw fault(409,'File changed outside editor; original retained');
      if(body.revision!==current.revision){if(body.text===current.text)return {id,revision:current.revision};throw fault(409,'Document changed')}
      if(body.text!==current.text){
        checkpoint(id,current.text);durableWrite(file,body.text);
        cache.set(id,{text:body.text,revision:hash(body.text)});
      }
      return {id,revision:hash(body.text)};
    },
    recover(text,id){
      if(id!==undefined){requireMemo(id)}
      durableWrite(path.join(dataDir,`${id===undefined?'web':id}-recovery-${now()}-${crypto.randomUUID()}.txt`),text);
    }
  };
}
