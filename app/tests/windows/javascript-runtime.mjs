// Scripted native acceptance on the owned test desktop. No model is involved.
import fs from 'node:fs';
import path from 'node:path';
import cp from 'node:child_process';
import readline from 'node:readline';
import crypto from 'node:crypto';
if(process.platform!=='win32'||process.env.ICU_DISPOSABLE_HOST!=='1')throw Error('owned Windows test desktop required');
const [root,work]=process.argv.slice(2);
fs.mkdirSync(work,{recursive:false});
const report={passed:false,mode:'scripted-native',steps:[],approvals:[]};
const save=()=>fs.writeFileSync(path.join(work,'result.json'),JSON.stringify(report,null,2));
const wait=async predicate=>{const end=Date.now()+15000;while(Date.now()<end){if(predicate())return;await new Promise(r=>setTimeout(r,50));}throw Error('native condition timed out');};
const fixtureDir=path.join(work,'fixture');fs.mkdirSync(fixtureDir);
const fixture=cp.spawn(path.join(root,'PairFixture.exe'),[fixtureDir],{stdio:'ignore'});
let runtime,approvalAllowed=false,sequence=0;
const pending=new Map();
const oracle=()=>JSON.parse(fs.readFileSync(path.join(fixtureDir,'state.json')));
const sourceHashes=Object.fromEntries(['session.mjs','server.mjs'].map(name=>[name,crypto.createHash('sha256').update(fs.readFileSync(path.join(root,'js-runtime',name))).digest('hex')]));
report.source_sha256=sourceHashes;
try {
 await wait(()=>fs.existsSync(path.join(fixtureDir,'state.json')));
 process.env.ICU_HOME=path.join(work,'icu-home');
 process.env.ICU_DRIVER='C:\\Program Files\\ICU Gap4\\cua-driver.exe';
 process.env.ICU_CURSOR='off';
 runtime=cp.spawn(process.execPath,[path.join(root,'js-runtime','server.mjs'),'--icu',path.join(root,'icu.exe')],{stdio:['pipe','pipe',fs.openSync(path.join(work,'runtime.stderr'),'w')],windowsHide:true});
 const send=m=>runtime.stdin.write(JSON.stringify({jsonrpc:'2.0',...m})+'\n');
 readline.createInterface({input:runtime.stdout}).on('line',line=>{
  const m=JSON.parse(line);fs.appendFileSync(path.join(work,'runtime.jsonl'),line+'\n');
  if(m.method==='elicitation/create'){
   const app=m.params?._meta?.tool_params?.app;
   const allowed=approvalAllowed&&typeof app==='string'&&[path.join(root,'PairFixture.exe').toLowerCase(),'pairfixture.exe'].includes(app.toLowerCase());
   report.approvals.push({app,accepted:allowed});send({id:m.id,result:{action:allowed?'accept':'decline',content:{persist:'session'}}});return;
  }
  const p=pending.get(m.id);if(p){clearTimeout(p.timer);pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);}
 });
 const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error('runtime RPC timeout'))},35000);pending.set(id,{resolve,reject,timer});send({id,method,params});});
 const cell=async(name,code,expect=true)=>{const r=await call('tools/call',{name:'js',arguments:{code}});report.steps.push({name,result:r});save();if(Boolean(r.isError)===expect)throw Error(name+' failed: '+JSON.stringify(r));return r;};
 await call('initialize',{protocolVersion:'2025-06-18',capabilities:{elicitation:{}}});
 const observe='state.window=(await icu.call("list_windows")).result.find(w=>w.title==="Paired task fixture"); if(!state.window)throw Error("fixture unavailable");state.snapshot=await icu.call("get_window_state",{window:state.window,include_text:true,include_screenshot:false});emit(state.snapshot.accessibility.tree);';
 await cell('native observation',observe);
 const edit='const matches=state.snapshot.accessibility.tree.split("\\n").filter(line=>/\\bID: draft(?:,|\\s|$)/.test(line)||/\\bDraft(?:,|\\s|$)/.test(line)).map(line=>/^\\s*(\\d+) /.exec(line)).filter(Boolean);if(matches.length!==1)throw Error("Draft target not unique");await icu.call("set_value",{window:state.window,element_index:Number(matches[0][1]),value:"persistent runtime text"});state.edited=true;emit(state.edited);';
 await cell('human decline refuses native edit',edit,false);
 if(oracle().draft!=='seed')throw Error('declined action changed the fixture');
 approvalAllowed=true;
 await cell('recover after refused cell',observe);
 await cell('accepted native edit',edit);
 await cell('state survives native cell','emit(state.edited);emit(state.window.title);');
 await wait(()=>oracle().draft==='persistent runtime text');
 report.oracle=oracle();
 await cell('guest cannot approve native actions','await icu.call("approve_app",{app:state.window.app,action:"accept"});',false);
 if(report.oracle.Alpha!=='alpha'||report.oracle.display!=='0')throw Error('unrelated fixture changed');
 if(!report.approvals.some(a=>!a.accepted)||!report.approvals.some(a=>a.accepted))throw Error('approval route was not exercised');
 report.passed=true;
} catch(error){report.error=String(error);} finally {
 for(const p of pending.values())clearTimeout(p.timer);
 if(runtime){runtime.stdin.end();try{await wait(()=>(runtime.exitCode!==null||runtime.signalCode!==null));}catch{cp.spawnSync('taskkill.exe',['/PID',String(runtime.pid),'/T','/F'],{stdio:'ignore'});report.forced_cleanup=true;}}
 if(fixture.exitCode===null){fixture.kill();try{await wait(()=>(fixture.exitCode!==null||fixture.signalCode!==null));}catch{report.forced_cleanup=true;}}
 report.cleanup_verified=(!runtime||(runtime.exitCode!==null||runtime.signalCode!==null))&&(fixture.exitCode!==null||fixture.signalCode!==null)&&!report.forced_cleanup;
 report.passed=report.passed&&report.cleanup_verified;save();console.log(JSON.stringify(report));process.exitCode=report.passed?0:1;
}
