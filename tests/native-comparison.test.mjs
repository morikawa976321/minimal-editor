import {test} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {createNativeComparison} from '../scripts/native-comparison.mjs';

test('comparison serves both choices, authenticates launches, and rejects outside origins',async()=>{
  const port=43141,launched=[];let activity=0;
  const server=createNativeComparison({port,token:'comparison-test',launch:async kind=>launched.push(kind),onActivity:()=>activity++});
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(port,'127.0.0.1',resolve)});
  try{
    const base=`http://127.0.0.1:${port}`;
    const request=(route,body,headers={})=>fetch(base+route,{method:'POST',headers:{Authorization:'Bearer comparison-test','Content-Type':'application/json',...headers},body:JSON.stringify(body)});
    const page=await(await fetch(base+'/')).text();
    assert.ok(page.includes('data-kind="web"'));assert.ok(page.includes('data-kind="native"'));assert.ok(page.includes('どちらも問題ない'));
    assert.equal((await request('/api/launch',{kind:'native'},{Authorization:'wrong'})).status,401);
    assert.equal((await request('/api/launch',{kind:'native'},{Origin:'http://outside.example'})).status,403);
    const rejectedHost=await new Promise((resolve,reject)=>{
      const req=http.request(base+'/api/launch',{method:'POST',headers:{Host:'outside.example'}},res=>{res.resume();res.once('end',()=>resolve(res.statusCode))});
      req.once('error',reject);req.end('{}');
    });
    assert.equal(rejectedHost,403);
    assert.equal((await request('/api/launch',{kind:'tk'})).status,400);
    assert.deepEqual(launched,[]);
    assert.equal((await request('/api/launch',{kind:'native'})).status,200);
    assert.equal((await request('/api/launch',{kind:'web'})).status,200);
    assert.deepEqual(launched,['native','web']);
    assert.equal((await request('/api/heartbeat',{})).status,200);assert.equal(activity,3);
  }finally{await new Promise(resolve=>server.close(resolve));server.closeAllConnections()}
});
