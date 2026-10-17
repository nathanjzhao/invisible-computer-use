// Diagnostic only: stock signed CLI and node_repl with the installed configuration.
import fs from 'node:fs';import path from 'node:path';import cp from 'node:child_process';import readline from 'node:readline';
if(process.platform!=='win32'||process.env.ICU_DISPOSABLE_HOST!=='1')throw Error('owned Windows desktop required');
const [resources,work]=process.argv.slice(2);fs.mkdirSync(work,{recursive:false});
const report={passed:false,steps:[]};
async function probe(name,command,args,steps){
 const row={name,passed:false};report.steps.push(row);const pending=new Map();let id=0;
 const child=cp.spawn(command,args,{windowsHide:true,stdio:['pipe','pipe',fs.openSync(path.join(work,name+'.stderr'),'w')]});
 const events=fs.openSync(path.join(work,name+'.jsonl'),'w');
 const reject=why=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(Error(why));}pending.clear();};
 child.on('error',e=>reject(String(e)));child.on('exit',(code,signal)=>{row.exit={code,signal};reject('child exited');});
 readline.createInterface({input:child.stdout}).on('line',line=>{let v;try{v=JSON.parse(line)}catch{return;}
  if(!v.method?.startsWith('account/'))fs.writeSync(events,JSON.stringify(v)+'\n');
  const p=pending.get(v.id);if(p){pending.delete(v.id);clearTimeout(p.timer);v.error?p.reject(Error(JSON.stringify(v.error))):p.resolve(v.result);}
 });
 const send=(method,params)=>new Promise((resolve,reject)=>{const n=++id;const timer=setTimeout(()=>{pending.delete(n);reject(Error('timeout: '+method));},25000);pending.set(n,{resolve,reject,timer});child.stdin.write(JSON.stringify({jsonrpc:'2.0',id:n,method,params})+'\n');});
 const notify=(method,params={})=>child.stdin.write(JSON.stringify({jsonrpc:'2.0',method,params})+'\n');
 try{row.results=await steps(send,notify);row.passed=true;}catch(e){row.error=String(e);}finally{child.stdin.end();await Promise.race([new Promise(r=>child.once('exit',r)),new Promise(r=>setTimeout(r,1500))]);if(child.exitCode===null&&child.signalCode===null)cp.spawnSync('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});fs.closeSync(events);fs.writeFileSync(path.join(work,'result.json'),JSON.stringify(report,null,2));}
}
await probe('appserver',path.join(resources,'codex.exe'),['app-server'],async(send,notify)=>{const init=await send('initialize',{clientInfo:{name:'icu-preflight',version:'1'},capabilities:{experimentalApi:true}});notify('initialized');return {initialize:init};});
await probe('node-repl',path.join(resources,'cua_node/bin/node_repl.exe'),[],async(send,notify)=>{const init=await send('initialize',{protocolVersion:'2024-11-05',clientInfo:{name:'icu-preflight',version:'1'},capabilities:{elicitation:{}}});notify('notifications/initialized');const tools=await send('tools/list',{});const js=tools.tools.find(t=>t.name==='js');if(!js)throw Error('js tool missing');const plain=await send('tools/call',{name:'js',arguments:{code:'nodeRepl.write({kernel:true});',title:'Kernel preflight'}});if(plain.isError)throw Error(JSON.stringify(plain));const native=await send('tools/call',{name:'js',arguments:{code:'var sky= (await import("@oai/sky")).sky; nodeRepl.write(await sky.list_windows());',title:'Native window preflight'}});if(native.isError)throw Error(JSON.stringify(native));return {initialize:init,plain,native};});
report.passed=report.steps.every(s=>s.passed);fs.writeFileSync(path.join(work,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));process.exitCode=report.passed?0:1;
