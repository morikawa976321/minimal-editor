import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const files=['scripts/app.vbs','scripts/native-app.vbs','scripts/browser-app.vbs','ネイティブ版.vbs','ブラウザ版.vbs'];
test('daily and explicit entry points route to isolated native/browser launchers', {skip:process.platform!=='win32'},()=>{
  fs.mkdirSync(path.join(root,'.work'),{recursive:true});
  const folder=fs.mkdtempSync(path.join(root,'.work/native-launch space-'));
  for(const relative of files){
    const target=path.join(folder,relative);fs.mkdirSync(path.dirname(target),{recursive:true});
    fs.copyFileSync(path.join(root,relative),target);
  }
  // Stub only the final launch, so real VBS wrappers, quoting, exit codes and
  // the native first-launch fallback execute without creating any GUI window.
  fs.writeFileSync(path.join(folder,'scripts/launch-native.mjs'),
    `import fs from 'node:fs'; import {fileURLToPath} from 'node:url'; fs.appendFileSync(fileURLToPath(new URL('../routes.txt',import.meta.url)),'native\\n');`);
  fs.writeFileSync(path.join(folder,'scripts/launch-editor.ps1'),
    `Add-Content -LiteralPath (Join-Path (Split-Path $PSScriptRoot -Parent) 'routes.txt') -Value 'browser'`);
  const launch=relative=>spawnSync('cscript.exe',['//Nologo',path.join(folder,relative)],{windowsHide:true,encoding:'utf8',timeout:15000});
  for(const relative of ['scripts/app.vbs','ネイティブ版.vbs','ブラウザ版.vbs']){
    const result=launch(relative);assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr+result.stdout);
  }
  assert.deepEqual(fs.readFileSync(path.join(folder,'routes.txt'),'utf8').trim().split(/\r?\n/),['native','native','browser']);
  assert.ok(!fs.existsSync(path.join(folder,'data')),'A launcher stub must not touch any drafts');
  fs.writeFileSync(path.join(folder,'scripts/native-app.vbs'),'WScript.Quit 7\r\n');
  assert.equal(launch('scripts/app.vbs').status,7,'Default entry must preserve launch failure status');
  fs.writeFileSync(path.join(folder,'scripts/browser-app.vbs'),'WScript.Quit 8\r\n');
  assert.equal(launch('ブラウザ版.vbs').status,8,'Browser wrapper must preserve launch failure status');
});
