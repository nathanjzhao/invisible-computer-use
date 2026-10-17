#!/usr/bin/env node
import cp from 'node:child_process';import path from 'node:path';import {Session} from './session.mjs';
const i=process.argv.indexOf('--icu');if(i<0||!path.isAbsolute(process.argv[i+1]||''))throw Error('usage: node server.mjs --icu /absolute/path/to/icu');
const executable=process.argv[i+1];const reply=message=>process.stdout.write(JSON.stringify(message)+'\n');
const requests=new Map(),approvals=new Map();let next=0,session,backend,backendReady,activeId,closed=false,clientElicitation=false,cellController;
function lines(stream,callback,max){let buffer='';stream.setEncoding('utf8');stream.on('data',chunk=>{buffer+=chunk;if(Buffer.byteLength(buffer)>max){stream.destroy();shutdown();return;}let at;while((at=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,at);buffer=buffer.slice(at+1);if(line)callback(line);}});}
function backendRequest(method,params,signal){return new Promise((resolve,reject)=>{const id=++next;const timer=setTimeout(()=>{requests.delete(id);reject(Error('native request deadline exceeded; effect may be uncertain'));},120000);requests.set(id,{resolve,reject,timer,signal});backend.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n');});}
async function connect(){if(backendReady)return backendReady;backendReady=(async()=>{
 backend=cp.spawn(executable,['mcp'],{stdio:['pipe','pipe','inherit'],windowsHide:true});
 const exited=error=>{for(const p of requests.values()){clearTimeout(p.timer);p.reject(Error(error));}requests.clear();session?.stop();};
 backend.on('error',e=>exited(String(e)));backend.on('exit',()=>exited('native MCP server exited'));
 lines(backend.stdout,line=>{let message;try{message=JSON.parse(line);}catch{exited('native MCP returned invalid JSON');backend.kill();return;}
  if(message.method&&message.id!==undefined){
   if(message.method!=='elicitation/create'){backend.stdin.write(JSON.stringify({jsonrpc:'2.0',id:message.id,error:{code:-32601,message:'Unsupported native request'}})+'\n');return;}
   if(closed||cellController?.signal.aborted||[...requests.values()].some(r=>r.signal?.aborted)){backend.stdin.write(JSON.stringify({jsonrpc:'2.0',id:message.id,result:{action:'cancel'}})+'\n');return;}
   const id='native-approval-'+(++next);approvals.set(id,message.id);reply({...message,id});return;
  }
  const p=requests.get(message.id);if(p){requests.delete(message.id);clearTimeout(p.timer);message.error?p.reject(Error(message.error.message)):p.resolve(message.result);}
 },12*1024*1024);
 await backendRequest('initialize',{protocolVersion:'2024-11-05',clientInfo:{name:'icu-javascript',version:'0.1.0'},capabilities:clientElicitation?{elicitation:{}}:{}});backend.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
 const {tools}=await backendRequest('tools/list',{});
 const observedGenerations=new Map();
 session=await Session.create({tools:tools.map(t=>t.name),dispatch:async(name,args,{signal})=>{
  if(signal.aborted||cellController?.signal.aborted||closed)throw Error('cell stopped before native dispatch');
  const cancel=()=>cancelApprovals();signal.addEventListener('abort',cancel,{once:true});
  if(name==='get_window_state'&&args.include_text){const key=JSON.stringify(args.window);if(observedGenerations.get(key)!==session.generation){args={...args,disableDiff:true};if(observedGenerations.size>=256)observedGenerations.clear();observedGenerations.set(key,session.generation);}}
  let r;try{r=await backendRequest('tools/call',{name,arguments:args},signal);}finally{signal.removeEventListener('abort',cancel);}
  if(r.isError)throw Error((r.content||[]).filter(c=>c.type==='text').map(c=>c.text).join('\n')||'native tool refused');
  return r.structuredContent??r;
 }});return session;
 })();return backendReady;}
const instructions='Persistent JavaScript through a WebAssembly interpreter. Use state to retain values between cells; local let/const declarations belong to one cell. Await icu.call(toolName,args) for every native operation. emit(value) returns JSON text; emitImage(base64,mimeType) returns an image. No Node, filesystem, network, module imports or implicit host APIs. At most 32 serial computer calls, 64 KiB code, 4 MiB output and 120 seconds per cell. Errors, stop and timeouts discard guest state; already admitted native actions may have occurred, so re-observe before retrying. App and consequential-action approvals are handled by the native MCP server and the human client; scripts cannot approve themselves.';
function cancelApprovals(){for(const [id,nativeId] of approvals){backend?.stdin.write(JSON.stringify({jsonrpc:'2.0',id:nativeId,result:{action:'cancel'}})+'\n');reply({jsonrpc:'2.0',method:'notifications/cancelled',params:{requestId:id,reason:'JavaScript cell stopped'}});}approvals.clear();}
function stopCell(){cellController?.abort();session?.stop();cancelApprovals();}
async function handle(m){
 if(closed)return;
 if(!m||typeof m!=='object'||Array.isArray(m)){reply({jsonrpc:'2.0',id:null,error:{code:-32600,message:'Invalid request'}});return;}
 let ownsSlot=false;
 if(approvals.has(m.id)&&!m.method){const id=approvals.get(m.id);approvals.delete(m.id);backend?.stdin.write(JSON.stringify({jsonrpc:'2.0',id,...(m.error?{error:m.error}:{result:m.result})})+'\n');return;}
 if(m.method==='notifications/cancelled'){if(m.params?.requestId===activeId)stopCell();return;}
 if(m.id===undefined)return;
 let result;
 try{
  if(m.method==='initialize'){clientElicitation=!!m.params?.capabilities?.elicitation;result={protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'icu-javascript',version:'0.1.0'},instructions};}
  else if(m.method==='ping')result={};
  else if(m.method==='tools/list')result={tools:[{name:'js',description:instructions,inputSchema:{type:'object',properties:{code:{type:'string',maxLength:65536},timeout_ms:{type:'integer',minimum:1,maximum:120000}},required:['code'],additionalProperties:false}},{name:'reset',description:'Discard persisted guest state after the current cell has finished.',inputSchema:{type:'object',properties:{},additionalProperties:false}}]};
  else if(m.method==='tools/call'){
   if(activeId!==undefined)throw Error('another cell is running');activeId=m.id;ownsSlot=true;cellController=new AbortController();const s=await connect();if(cellController.signal.aborted)throw Error('cell stopped during native initialization');
   if(m.params?.name==='reset'){s.reset();result={content:[{type:'text',text:'Guest state reset.'}]};}
   else if(m.params?.name==='js'){
    const r=await s.execute(m.params.arguments?.code,{timeoutMs:m.params.arguments?.timeout_ms??30000});
    result={content:[...r.outputs,{type:'text',text:JSON.stringify({ok:r.ok,calls:r.calls,generation:r.generation,error:r.error,state_reset:r.state_reset})}],isError:!r.ok};
   }else throw Error('unknown runtime tool');
  }else{reply({jsonrpc:'2.0',id:m.id,error:{code:-32601,message:'Method not found'}});return;}
  reply({jsonrpc:'2.0',id:m.id,result});
 }catch(error){reply({jsonrpc:'2.0',id:m.id,result:{isError:true,content:[{type:'text',text:String(error.message||error)}]}});}
 finally{if(ownsSlot&&activeId===m.id)activeId=undefined;}
}
function shutdown(){if(closed)return;closed=true;stopCell();if(backend){backend.stdin.end();setTimeout(()=>backend.kill(),300).unref();}setTimeout(()=>process.exit(0),500).unref();}
lines(process.stdin,line=>{let m;try{m=JSON.parse(line);}catch{return reply({jsonrpc:'2.0',id:null,error:{code:-32700,message:'Invalid JSON'}});}void handle(m);},128*1024);
process.stdin.on('end',shutdown);process.on('SIGTERM',shutdown);process.on('SIGINT',shutdown);
