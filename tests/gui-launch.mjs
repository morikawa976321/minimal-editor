import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {root,startProcess} from '../scripts/server.mjs';

// The real GUI launcher and executable, but --probe never shows its window.
// Inspect Windows STARTUPINFO rather than stealing the user's focus.
const folder=fs.mkdtempSync(path.join(root,'.work/gui-launch-'));
const native=path.join(root,'build/native.exe');
const control=path.join(folder,'hidden-control');
const child=spawn(native,['--probe','--data-dir',control],{stdio:'ignore',windowsHide:true});
assert.equal((await once(child,'exit'))[0],0);
const hidden=JSON.parse(fs.readFileSync(path.join(control,'probe-result.json'),'utf8'));
assert.ok((hidden.startup_flags&1)!==0&&hidden.startup_show===0,'Control must reproduce SW_HIDE');
const actual=path.join(folder,'actual');
await startProcess(native,['--probe','--data-dir',actual]);
const report=path.join(actual,'probe-result.json');
for(let i=0;i<100&&!fs.existsSync(report);i++)await new Promise(r=>setTimeout(r,25));
const result=JSON.parse(fs.readFileSync(report,'utf8'));
assert.ok((result.startup_flags&1)===0||result.startup_show!==0,'GUI launcher must not request SW_HIDE');
console.log(JSON.stringify({passed:true,hidden_control:{flags:hidden.startup_flags,show:hidden.startup_show},gui_launch:{flags:result.startup_flags,show:result.startup_show}}));
