import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {seedNative} from './seed-native.mjs';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
export function adoptNative({sourceDir=path.join(root,'data'),targetDir=path.join(root,'data/native-next'),workDir=path.join(root,'data/native-adoption'),exe=path.join(root,'build/native-adopt.exe')}={}){
  const reportFile=path.join(workDir,'native-adoption-completed.json');
  if(fs.existsSync(reportFile))throw Error('Native adoption was already completed; refusing to repeat it.');
  if(!fs.existsSync(exe))throw Error('Build native-adopt.exe from prototypes/native-next/adopt.rs first.');
  fs.mkdirSync(workDir,{recursive:true});
  const runDir=fs.mkdtempSync(path.join(workDir,'native-adoption-'));
  const sourceSnapshot=path.join(runDir,'browser-snapshot'),backupDir=path.join(runDir,'native-before');
  seedNative(sourceDir,sourceSnapshot);
  const result=spawnSync(exe,['--source-dir',sourceSnapshot,'--target-dir',path.resolve(targetDir),'--backup-dir',backupDir,'--report-file',reportFile],{cwd:root,windowsHide:true,encoding:'utf8'});
  if(result.error)throw result.error;
  if(result.status!==0)throw Error((result.stderr||'Adoption failed.').trim()+` Recovery files: ${runDir}`);
  const report=JSON.parse(fs.readFileSync(reportFile,'utf8'));
  const resultFile=path.join(runDir,'result.json');
  fs.writeFileSync(resultFile,JSON.stringify({...report,sourceSnapshot,backupDir},null,2),{flag:'wx'});
  return {...report,sourceSnapshot,backupDir,resultFile};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{console.log(JSON.stringify(adoptNative()))}
  catch(error){console.error(error.message);process.exitCode=1;}
}
