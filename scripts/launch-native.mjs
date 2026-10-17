import fs from 'node:fs';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {seedNative} from './seed-native.mjs';

export const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
export async function launchNative({sourceDir=path.join(root,'data'),targetDir=path.join(root,'data/native-next'),exe=path.join(root,'build/native-next.exe')}={}){
  if(!fs.existsSync(exe))throw Error('Native editor is not built. Run scripts/build-native.ps1 first.');
  seedNative(sourceDir,targetDir);
  await new Promise((resolve,reject)=>{
    const child=spawn(exe,['--data-dir',path.resolve(targetDir)],{cwd:root,detached:true,stdio:'ignore',windowsHide:false});
    child.once('error',reject);child.once('spawn',()=>{child.unref();resolve()});
  });
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{await launchNative()}
  catch(error){
    fs.mkdirSync(path.join(root,'.work'),{recursive:true});
    fs.appendFileSync(path.join(root,'.work/native-launch.log'),`${new Date().toISOString()} ${error.stack}\n`);
    console.error(error.message);process.exitCode=1;
  }
}
