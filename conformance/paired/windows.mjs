// Runs only inside the explicitly disposable Windows interactive user session.
import fs from 'node:fs';import path from 'node:path';import cp from 'node:child_process';import net from 'node:net';import crypto from 'node:crypto';
if(process.platform!=='win32'||process.env.ICU_DISPOSABLE_HOST!=='1')throw Error('disposable Windows host required');
const [engine,root,work,ownershipMode,suite='basic']=process.argv.slice(2);const measureOwnership=ownershipMode==='with-ownership';if(!['openai-native','icu-native'].includes(engine))throw Error('engine required');fs.mkdirSync(work,{recursive:false});
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');const sleep=n=>new Promise(r=>setTimeout(r,n));
const fixture=path.join(root,'PairFixture.exe'),native=path.join(root,engine==='openai-native'?'codex-computer-use.exe':'cua-driver.exe');
const report={schema:1,engine,mode:'scripted-native',executed:false,cleanup_verified:false,os:`Windows ${cp.execFileSync('cmd.exe',['/c','ver'],{encoding:'utf8'}).trim()}`,display:null,fixture_sha256:sha(fixture),harness_sha256:sha(new URL(import.meta.url)),engine_sha256:sha(native),cases:[]};
const cases={calculator:{expected:{display:'42',draft:'seed',Alpha:'alpha'}},text_replacement:{expected:{display:'0',draft:'paired text',Alpha:'alpha'}},invalid_element:{expected:{display:'0',draft:'seed',Alpha:'alpha',refused:true}}};
if(suite==='expanded'){const extra=JSON.parse(fs.readFileSync(path.join(root,'expanded/cases.json'),'utf8'));for(const spec of Object.values(extra))spec.expected={display:'0',draft:'seed',Alpha:'alpha',...spec.expected};Object.assign(cases,extra);}
let helper,driver;const children=[];const pipe=`\\\\.\\pipe\\icu-paired-${process.pid}`;
async function wait(fn,ms=20000){const end=Date.now()+ms;while(Date.now()<end){const x=await fn();if(x)return x;await sleep(100);}throw Error('condition not ready');}
function samples(dir){try{return fs.readFileSync(path.join(dir,'ownership.jsonl'),'utf8').split(/\r?\n/).slice(0,-1).filter(Boolean).map(JSON.parse);}catch{return [];}}
function readGuard(dir){try{return JSON.parse(fs.readFileSync(path.join(dir,'guard.json'),'utf8'));}catch{return null;}}
function readState(dir){try{return JSON.parse(fs.readFileSync(path.join(dir,'state.json'),'utf8'));}catch{return null;}}
async function ours(method,args){return new Promise((resolve,reject)=>{let data='';const s=net.connect(pipe);const timer=setTimeout(()=>s.destroy(Error('driver timeout')),30000);s.on('connect',()=>s.write(JSON.stringify({method:'call',name:method,args,session_id:'paired',client_kind:'cli'})+'\n'));s.on('data',buf=>{data+=buf;if(data.length>16*1024*1024)s.destroy(Error('reply too large'));if(data.includes('\n')){clearTimeout(timer);s.end();try{const v=JSON.parse(data.split('\n')[0]);if(!v.ok)reject(Error(JSON.stringify(v)));else resolve(v.result);}catch(e){reject(e);}}});s.on('error',e=>{clearTimeout(timer);reject(e);});});}
try{
 if(engine==='openai-native'){
  globalThis.nodeRepl={config:{},requestMeta:{'x-codex-turn-metadata':JSON.stringify({session_id:'icu-paired-reference',turn_id:'paired-native-1'})},createElicitation:async request=>{
   const app=request.meta?.tool_params?.app;const allowed=typeof app==='string'&&(['pairfixture.exe',fixture.toLowerCase()].includes(app.toLowerCase()));
   fs.appendFileSync(path.join(work,'approvals.jsonl'),JSON.stringify({request,accepted:allowed})+'\n');return {action:allowed?'accept':'decline',content:{},_meta:{persist:'session'}};
  }};
  const {WindowsHelperTransport}=await import('./reference/helper_transport.js');helper=new WindowsHelperTransport({helperCommand:native,timeoutMs:30000});
 }else{const log=fs.openSync(path.join(work,'driver.log'),'w');driver=cp.spawn(native,['serve','--no-overlay','--socket',pipe],{env:{...process.env,CUA_DRIVER_EMBEDDED:'1'},stdio:['ignore',log,log],windowsHide:true});await sleep(2000);}
 report.executed=true;
 for(const id of Object.keys(cases)){
  const dir=path.join(work,id);fs.mkdirSync(dir);console.log(JSON.stringify({phase:'case-start',engine,id}));const record={id,executed:false,observations:[],actions:[]};report.cases.push(record);
  const fx=cp.spawn(fixture,[dir],{stdio:'ignore'});children.push(fx);let guard=null;
  try{
   const baseline=await wait(()=>readState(dir));report.display=baseline.screen;record.executed=true;let window,observed;
   if(measureOwnership){guard=cp.spawn(path.join(root,'PairGuard.exe'),[dir],{stdio:'ignore'});children.push(guard);const g=await wait(()=>readGuard(dir));record.ownership_baseline=await wait(()=>{const rows=samples(dir);const row=rows.at(-1);return row?.foreground===g.window?row:null;});await sleep(300);}
   const call=async(method,args)=>{const result=engine==='openai-native'?await helper.request(method,args):await ours(method,args);fs.appendFileSync(path.join(dir,'rpc.jsonl'),JSON.stringify({method,args,result})+'\n');return result;};
   if(engine==='openai-native'){const windows=await call('list_windows',{});window=windows.find(w=>w.title==='Paired task fixture');if(!window)throw Error('reference fixture window missing');}
   async function observe(){
    for(let attempt=0;attempt<2;attempt++){
     try{
      const result=await call('get_window_state',engine==='openai-native'?{window,include_text:true,include_screenshot:true}:{pid:baseline.pid,window_id:baseline.window,include_text:true,include_screenshot:false});
      if(result.isError)throw Error(JSON.stringify(result));
      observed=result.structuredContent??result;
      if(engine==='openai-native' && (!observed.accessibility?.tree || !observed.screenshots?.length))throw Error('reference returned incomplete window state');
      record.observations.push(observed);return observed;
     }catch(e){
      record.read_errors??=[];record.read_errors.push(String(e));
      if(engine!=='openai-native'||attempt===1)throw e;
      await sleep(2000);
      const windows=await call('list_windows',{});window=windows.find(w=>w.title==='Paired task fixture');if(!window)throw Error('reference fixture disappeared during recovery');
      const result=await call('activate_window',{window});record.recovery??=[];record.recovery.push({method:'activate_window',result});
     }
    }
   }
   await observe();
   const actions=cases[id].actions??(id==='calculator'?['6','×','7','='].map(label=>({kind:'click',label})):id==='text_replacement'?[{kind:'set_value',label:'Draft',value:'paired text'}]:[{kind:'click',index:99999}]);let refused=false;
   for(const action of actions){let index=action.index;
    if(index==null){if(engine==='icu-native'){const matches=observed.elements.filter(e=>[e.label,e.title,e.description].includes(action.label) && (action.label!=='Format'||e.role==='ComboBox'));if(matches.length!==1)throw Error('candidate target not unique');index=matches[0].element_index;}else{const text=observed.accessibility?.tree??'';const stable=action.kind==='set_value'?'draft':({'Enable option':'enabled','Detailed mode':'detailed','Format':'format','Records':'records','Last record':'last-record','Apply draft':'apply'}[action.label]??'button-'+action.label);const matches=text.split('\n').filter(line=>/\bID: ([^,\s]+)/.exec(line)?.[1]===stable || (action.label==='Markdown' && /\bMarkdown(?:,|$)/.test(line))).map(line=>/^\s*(\d+) /.exec(line)).filter(Boolean);if(matches.length!==1)throw Error('reference target not unique: '+action.label);index=Number(matches[0][1]);}}
    try{const args=engine==='openai-native'?{window,element_index:index}:{pid:baseline.pid,window_id:baseline.window,element_index:index,snapshot_id:observed.snapshot_id};if(action.kind==='set_value')args.value=action.value;if(action.kind==='scroll'){args.direction=action.direction;if(engine==='openai-native'){delete args.element_index;delete args.direction;args.x=100;args.y=400;args.scrollX=0;args.scrollY=action.amount*600;args.screenshotId=observed.screenshots?.[0]?.id;}else{args.by='page';args.amount=action.amount;}}const result=await call(engine==='openai-native'&&action.kind==='click'?'click_element':action.kind,args);record.actions.push({action,result});if(action.index!=null)refused=result.isError===true;else if(result.isError)throw Error(JSON.stringify(result));}catch(e){record.actions.push({action,error:String(e)});if(action.index!=null)refused=true;else throw e;}
    await sleep(200);await observe();
   }
   const final=readState(dir);record.outcome=Object.fromEntries(Object.keys(cases[id].expected).filter(k=>k!=='refused').map(k=>[k,final[k]]));if(id==='invalid_element')record.outcome.refused=refused;
  }catch(e){record.error=String(e);}finally{
   if(guard){await sleep(450);const rows=samples(dir),base=record.ownership_baseline,g=readGuard(dir);const changes=base?rows.filter(r=>r.foreground!==base.foreground||r.focus!==base.focus||r.x!==base.x||r.y!==base.y):rows;const resets=base?rows.filter(r=>r.lastInput!==base.lastInput):rows;record.ownership={sample_count:rows.length,violations:changes.length,hid_resets:resets.length,guard_text:g?.text,first_violation:changes.slice(0,1),first_hid_reset:resets.slice(0,1)};if(rows.length<20)record.error='insufficient ownership samples';guard.kill();await new Promise(resolve=>(guard.exitCode!==null||guard.signalCode!==null)?resolve():guard.once('exit',resolve));}
   console.log(JSON.stringify({phase:'case-end',engine,id,error:record.error,outcome:record.outcome}));fx.kill();await new Promise(resolve=>(fx.exitCode!==null||fx.signalCode!==null)?resolve():fx.once('exit',resolve));}
 }
}catch(e){report.error=String(e);}finally{
 if(helper)await helper.close();if(driver){driver.kill();await new Promise(resolve=>(driver.exitCode!==null||driver.signalCode!==null)?resolve():driver.once('exit',resolve));}
 report.cleanup_verified=children.every(p=>(p.exitCode!==null||p.signalCode!==null))&&(!driver||(driver.exitCode!==null||driver.signalCode!==null));
 fs.writeFileSync(path.join(work,'result.json'),JSON.stringify(report,null,2));fs.writeFileSync(path.join(work,'contract.json'),JSON.stringify({mode:'scripted-native',fixture_sha256:report.fixture_sha256,cases,ownership_required:measureOwnership},null,2));
 console.log(JSON.stringify({...report,cases:report.cases.map(({observations,actions,...rest})=>rest)}));
 process.exitCode=report.executed&&report.cleanup_verified&&report.cases.length===Object.keys(cases).length&&report.cases.every(c=>!c.error)?0:1;
}
