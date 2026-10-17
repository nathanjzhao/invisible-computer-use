// Same model/controller over two native engines. This is not desktop-app parity.
import fs from 'node:fs';import path from 'node:path';import cp from 'node:child_process';import readline from 'node:readline';import crypto from 'node:crypto';import {pathToFileURL,fileURLToPath} from 'node:url';
if(process.platform!=='win32'||process.env.ICU_DISPOSABLE_HOST!=='1')throw Error('owned Windows test desktop required');
const [root,work,resources]=process.argv.slice(2);fs.mkdirSync(work,{recursive:false});
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const fixture=path.join(root,'PairFixture.exe');
const find=(dir,name)=>{for(const entry of fs.readdirSync(dir,{withFileTypes:true})){const p=path.join(dir,entry.name);if(entry.isFile()&&entry.name===name)return p;if(entry.isDirectory()){const found=find(p,name);if(found)return found;}}};
const transportPath=find(path.join(resources,'cua_node'),'helper_transport.js');
const referenceExe=find(path.join(resources,'cua_node'),'codex-computer-use.exe');
const report={passed:false,mode:'same-model-native-engine-comparison',model:'gpt-6-astra',controller_sha256:hash(fileURLToPath(import.meta.url)),fixture_sha256:hash(fixture),reference_sha256:hash(referenceExe),reference_transport_sha256:hash(transportPath),icu_sha256:hash(path.join(root,'icu.exe')),cases:[],approvals:[]};
const save=()=>fs.writeFileSync(path.join(work,'result.json'),JSON.stringify(report,null,2));
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const ended=p=>p.exitCode!==null||p.signalCode!==null;
const wait=async(predicate,timeout=15000)=>{const deadline=Date.now()+timeout;while(Date.now()<deadline){const v=predicate();if(v)return v;await sleep(80);}throw Error('condition timed out');};
const tasks={calculator:{goal:'Compute six times seven by using the calculator buttons. Read the result before finishing.',expected:{display:'42',draft:'seed',Alpha:'alpha'}},text_replacement:{goal:'Replace the Draft field text with paired text. Keep the Alpha document unchanged.',expected:{display:'0',draft:'paired text',Alpha:'alpha'}}};
let sequence=0;
async function decide(state,goal,history,row){
 const content=[{type:'input_text',text:JSON.stringify({goal,accessibility:state.accessibility,history})}];
 const shot=state.screenshots?.[0];
 if(shot){let url=shot.url;if(url.startsWith('file:')){const file=fileURLToPath(url);if(fs.statSync(file).size>8*1024*1024)throw Error('screenshot too large');url='data:image/png;base64,'+fs.readFileSync(file).toString('base64');}if(!/^data:image\/(png|jpeg);base64,/.test(url))throw Error('unsupported screenshot origin');content.push({type:'input_image',image_url:url,detail:'original'});}
 const response=await fetch(process.env.ICU_REFERENCE_GATEWAY_URL+'/responses',{method:'POST',headers:{'Content-Type':'application/json','Authorization':'Bearer '+process.env.ICU_REFERENCE_GATEWAY_TOKEN},body:JSON.stringify({model:report.model,store:false,stream:true,instructions:'Control only the selected synthetic fixture. Return one JSON object with kind click, set_value, or done. For click and set_value, include element_index from the current numbered accessibility tree. For set_value include value. Use one action per response. Verify the goal in a fresh observation before done. Do not operate the other document. No markdown.',input:[{role:'user',content}]}),signal:AbortSignal.timeout(60000)});
 if(!response.ok)throw Error('model HTTP '+response.status);
 const wire=await response.text();if(wire.length>4*1024*1024)throw Error('model reply too large');let text='';
 for(const line of wire.split('\n')){if(!line.startsWith('data: '))continue;let e;try{e=JSON.parse(line.slice(6))}catch{continue;}if(e.type==='response.output_text.delta')text+=e.delta??'';if(e.type==='response.completed')row.response_id=e.response?.id;}
 const decision=JSON.parse(text);if(!['click','set_value','done'].includes(decision.kind))throw Error('unexpected model action');if(decision.kind!=='done'&&(!Number.isSafeInteger(decision.element_index)||decision.element_index<0))throw Error('invalid element index');if(decision.kind==='set_value'&&(typeof decision.value!=='string'||decision.value.length>1000))throw Error('invalid value');return decision;
}
function accepted(app){return typeof app==='string'&&['pairfixture.exe',fixture.toLowerCase()].includes(app.toLowerCase());}
async function connect(engine,dir){
 if(engine==='openai-native'){
  globalThis.nodeRepl={config:{},requestMeta:{'x-codex-turn-metadata':JSON.stringify({session_id:'round4-native-'+(++sequence),turn_id:'comparison'})},createElicitation:async q=>{const app=q.meta?.tool_params?.app;const allow=accepted(app);report.approvals.push({engine,app,accepted:allow});return {action:allow?'accept':'decline',content:{},_meta:{persist:'session'}};}};
  const {WindowsHelperTransport}=await import(pathToFileURL(transportPath));const helper=new WindowsHelperTransport({helperCommand:referenceExe,timeoutMs:15000});
  return {call:(name,args)=>helper.request(name,args),close:()=>helper.close()};
 }
 const child=cp.spawn(path.join(root,'icu.exe'),['mcp'],{env:{...process.env,ICU_HOME:path.join(dir,'icu-home'),ICU_DRIVER:'C:\\Program Files\\ICU Gap4\\cua-driver.exe',ICU_CURSOR:'off'},windowsHide:true,stdio:['pipe','pipe',fs.openSync(path.join(dir,'native.stderr'),'w')]});
 let id=0;const pending=new Map();const send=m=>child.stdin.write(JSON.stringify({jsonrpc:'2.0',...m})+'\n');
 readline.createInterface({input:child.stdout}).on('line',line=>{const m=JSON.parse(line);if(m.method==='elicitation/create'){const app=m.params?._meta?.tool_params?.app;const allow=accepted(app);report.approvals.push({engine,app,accepted:allow});send({id:m.id,result:{action:allow?'accept':'decline',content:{persist:'session'}}});return;}const p=pending.get(m.id);if(p){clearTimeout(p.timer);pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);}});
 const rpc=(method,params)=>new Promise((resolve,reject)=>{const n=++id,timer=setTimeout(()=>{pending.delete(n);reject(Error('native RPC timeout'))},25000);pending.set(n,{resolve,reject,timer});send({id:n,method,params});});
 await rpc('initialize',{protocolVersion:'2025-06-18',capabilities:{elicitation:{}}});send({method:'notifications/initialized'});
 return {call:async(name,args)=>{const result=await rpc('tools/call',{name,arguments:args});if(result.isError)throw Error(JSON.stringify(result));return result.structuredContent?.result??result.structuredContent;},close:async()=>{child.stdin.end();try{await wait(()=>ended(child),5000);}finally{for(const p of pending.values())clearTimeout(p.timer);if(!ended(child)){cp.spawnSync('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{stdio:'ignore'});throw Error('native cleanup needed forced termination');}}}};
}
for(const engine of ['openai-native','icu-native'])for(const [task,spec]of Object.entries(tasks)){
 const dir=path.join(work,engine+'-'+task);fs.mkdirSync(dir);const row={engine,task,goal:spec.goal,expected:spec.expected,passed:false,actions:[],observations:[]};report.cases.push(row);save();
 const fixtureDir=path.join(dir,'fixture');fs.mkdirSync(fixtureDir);const fx=cp.spawn(fixture,[fixtureDir],{stdio:'ignore'});let native;
 const state=()=>JSON.parse(fs.readFileSync(path.join(fixtureDir,'state.json')));
 try{
  await wait(()=>fs.existsSync(path.join(fixtureDir,'state.json')));native=await connect(engine,dir);
  const windows=await native.call('list_windows',{});const window=windows.find(w=>w.title==='Paired task fixture');if(!window)throw Error('fixture missing');
  const history=[];
  for(let step=0;step<12;step++){
   const observed=await native.call('get_window_state',{window,include_text:true,include_screenshot:true,disableDiff:true});
   row.observations.push({tree:observed.accessibility?.tree,screenshots:(observed.screenshots??[]).map(({url,...meta})=>meta)});
   const action=await decide(observed,spec.goal,history,row);row.actions.push(action);save();
   if(action.kind==='done'){row.model_completed=true;break;}
   const method=engine==='openai-native'&&action.kind==='click'?'click_element':action.kind;
   const result=await native.call(method,{window,element_index:action.element_index,...(action.kind==='set_value'?{value:action.value}:{})});
   if(result?.isError)throw Error(JSON.stringify(result));history.push(action);await sleep(150);
  }
  const actual=state();row.outcome=Object.fromEntries(Object.keys(spec.expected).map(k=>[k,actual[k]]));row.passed=row.model_completed===true&&JSON.stringify(row.outcome)===JSON.stringify(spec.expected);
 }catch(error){row.error=String(error);}finally{
  try{await native?.close();row.native_cleanup=true;}catch(error){row.cleanup_error=String(error);row.passed=false;}
  if(!ended(fx))fx.kill();await wait(()=>ended(fx));row.fixture_cleanup=true;save();
 }
}
report.passed=report.cases.length===4&&report.cases.every(c=>c.passed&&c.native_cleanup&&c.fixture_cleanup);save();console.log(JSON.stringify({passed:report.passed,cases:report.cases.map(({observations,...c})=>c)}));process.exitCode=report.passed?0:1;
