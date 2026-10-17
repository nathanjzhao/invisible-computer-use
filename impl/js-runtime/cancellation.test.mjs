import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import cp from 'node:child_process';
import readline from 'node:readline';
import {fileURLToPath} from 'node:url';

for (const approvalDelay of [0, 80]) test(`cancel refuses approval arriving ${approvalDelay ? 'after' : 'before'} stop`, async () => {
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'icu-cancel-'));
 const native=path.join(dir,'native'),effect=path.join(dir,'effect');
 fs.writeFileSync(native,`#!${process.execPath}
 const fs=require('fs'),rl=require('readline').createInterface({input:process.stdin});
 const send=m=>process.stdout.write(JSON.stringify(m)+'\\n');let waiting;
 rl.on('line',line=>{const m=JSON.parse(line);
 if(m.id==='approval'&&!m.method){fs.writeFileSync(${JSON.stringify(effect)},m.result.action);send({id:waiting,result:{content:[],structuredContent:{accepted:m.result.action==='accept'}}});return;}
 if(m.id===undefined)return;
 if(m.method==='initialize')send({id:m.id,result:{}});
 else if(m.method==='tools/list')send({id:m.id,result:{tools:[{name:'click'}]}});
 else if(m.method==='tools/call'){waiting=m.id;send({method:'test/dispatched'});setTimeout(()=>send({id:'approval',method:'elicitation/create',params:{message:'fixture only',requestedSchema:{type:'object'}}}),${approvalDelay});}
 });`,{mode:0o700});
 const child=cp.spawn(process.execPath,[fileURLToPath(new URL('./server.mjs',import.meta.url)),'--icu',native],{stdio:['pipe','pipe','pipe']});
 const send=m=>child.stdin.write(JSON.stringify({jsonrpc:'2.0',...m})+'\n');
 const pending=new Map();let seq=0,approval;
 const reader=readline.createInterface({input:child.stdout});
 reader.on('line',line=>{const m=JSON.parse(line);if(m.method==='elicitation/create'){approval=m.id;if(!approvalDelay)send({method:'notifications/cancelled',params:{requestId:2}});return;}
 const p=pending.get(m.id);if(p){pending.delete(m.id);clearTimeout(p.timer);p.resolve(m);}});
 const call=(method,params)=>new Promise((resolve,reject)=>{const id=++seq,timer=setTimeout(()=>reject(Error('timeout')),5000);pending.set(id,{resolve,reject,timer});send({id,method,params});});
 try {
  await call('initialize',{capabilities:{elicitation:{}}});
  const running=call('tools/call',{name:'js',arguments:{code:'await icu.call("click")',timeout_ms:approvalDelay?20:1000}});
  const result=await running;assert.equal(result.result.isError,true);
  const deadline=Date.now()+2000;while(!fs.existsSync(effect)&&Date.now()<deadline)await new Promise(r=>setTimeout(r,10));
  assert.equal(fs.readFileSync(effect,'utf8'),'cancel');
  if(approval)send({id:approval,result:{action:'accept'}});
  await new Promise(r=>setTimeout(r,30));assert.equal(fs.readFileSync(effect,'utf8'),'cancel');
 } finally {
  child.stdin.end();await new Promise(r=>{child.once('exit',r);setTimeout(()=>{child.kill();r()},1500).unref();});
  for(const p of pending.values())clearTimeout(p.timer);fs.rmSync(dir,{recursive:true,force:true});
 }
});
