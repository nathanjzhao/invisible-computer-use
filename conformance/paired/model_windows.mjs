// Signed Codex app-server, real model turns, and an independent native fixture oracle.
// Run only as the interactive user on an explicitly owned disposable Windows host.
import fs from 'node:fs';
import path from 'node:path';
import cp from 'node:child_process';
import crypto from 'node:crypto';
import readline from 'node:readline';

if (process.platform !== 'win32' || process.env.ICU_DISPOSABLE_HOST !== '1') throw Error('disposable Windows host required');
const [root, work, model = 'gpt-6-astra'] = process.argv.slice(2);
fs.mkdirSync(work, {recursive: false});
const codex = process.env.ICU_REFERENCE_CODEX || path.join(root, 'official/unpacked/app/resources/codex.exe');
const fixture = path.join(root, 'PairFixture.exe');
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const sleep = ms => new Promise(r => setTimeout(r, ms));
const readState = dir => { try { return JSON.parse(fs.readFileSync(path.join(dir, 'state.json'))); } catch { return null; } };
const tasks = {
  calculator: ['Compute six times seven by using the calculator buttons. Read the result before finishing.', {display:'42',draft:'seed',Alpha:'alpha'}],
  text_replacement: ['Replace the Draft field text with paired text. Keep the Alpha document unchanged.', {display:'0',draft:'paired text',Alpha:'alpha'}],
};
const report = {engine:'openai-native',mode:'model-driven-product',model,executed:false,passed:false,cases:[],approvals:[],fixture_sha256:sha(fixture),codex_sha256:sha(codex),harness_sha256:sha(new URL(import.meta.url))};
const pending = new Map(), notes = [], children = [];
let seq = 0, server;
const requestMs=Number(process.env.ICU_REFERENCE_REQUEST_MS||60000);
const turnMs=Number(process.env.ICU_REFERENCE_TURN_MS||180000);
function receipt(){fs.writeFileSync(path.join(work,'partial-result.json'),JSON.stringify(report,null,2));}
function rejectPending(reason){for(const p of pending.values()){clearTimeout(p.timer);p.reject(Error(reason));}pending.clear();}
function write(v) { server.stdin.write(JSON.stringify(v)+'\n'); }
function request(method, params, timeout=requestMs) {
  const id=++seq;
  return new Promise((resolve,reject) => {
    const timer=setTimeout(() => { pending.delete(id); reject(Error('RPC timeout: '+method)); },timeout);
    pending.set(id,{method,resolve,reject,timer});write({id,method,params});
  });
}
async function wait(predicate, timeout=30000) {
  const end=Date.now()+timeout;
  while(Date.now()<end) { const v=predicate();if(v)return v;await sleep(100); }
  throw Error('condition timed out');
}
function approve(msg) {
  const meta=msg.params?._meta ?? msg.params?.meta ?? {};
  const target=meta.tool_params?.app;
  const allowed=typeof target==='string' && ['pairfixture.exe',fixture.toLowerCase()].includes(target.toLowerCase());
  const answer=msg.method==='mcpServer/elicitation/request'
    ? {action:allowed?'accept':'decline',content:{},_meta:{persist:'session'}}
    : msg.method==='item/tool/requestUserInput' ? {answers:{}} : {decision:'decline'};
  report.approvals.push({method:msg.method,target,answer});write({id:msg.id,result:answer});
}
async function stop(p) {
  if(!p || p.exitCode!==null || p.signalCode!==null)return;
  // Child tree only. The server owns its node_repl and helper descendants.
  cp.spawnSync('taskkill.exe',['/PID',String(p.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});
  await wait(()=>p.exitCode!==null || p.signalCode!==null,10000);
}
try {
  const login=cp.spawnSync(codex,['login','status'],{encoding:'utf8',windowsHide:true});
  report.login={status:login.status,signed_in:/Logged in using ChatGPT/.test(login.stdout+login.stderr)};
  report.model_transport=process.env.ICU_REFERENCE_GATEWAY_TOKEN?'bounded authenticated Responses gateway':'host ChatGPT login';
  if(!report.login.signed_in&&!process.env.ICU_REFERENCE_GATEWAY_TOKEN)throw Error('reference account not signed in');
  report.phase='starting_app_server';receipt();
  server=cp.spawn(codex,['app-server'],{cwd:path.join(root,'model-work'),windowsHide:true,stdio:['pipe','pipe',fs.openSync(path.join(work,'server-stderr.log'),'w')]});
  server.on('error',error=>{report.server_error=String(error);rejectPending('reference server spawn failed: '+error);receipt();});
  server.on('exit',(code,signal)=>{report.server_exit={code,signal};rejectPending('reference server exited before its response');receipt();});
  readline.createInterface({input:server.stdout}).on('line',line=>{
    let msg;try {msg=JSON.parse(line);}catch{return;}
    // Do not retain account identifiers or credentials in the audit stream.
    const p=pending.get(msg.id);
    if(p?.method!=='account/read'&&!msg.method?.startsWith('account/'))fs.appendFileSync(path.join(work,'appserver.jsonl'),JSON.stringify(msg)+'\n');
    if(msg.method && msg.id!==undefined){approve(msg);return;}
    if(p){clearTimeout(p.timer);pending.delete(msg.id);msg.error?p.reject(Error(JSON.stringify(msg.error))):p.resolve(msg.result);}
    else notes.push(msg);
  });
  await request('initialize',{clientInfo:{name:'icu-paired-model',version:'1'},capabilities:{experimentalApi:true}});
  write({method:'initialized'});
  const account=await request('account/read',{});
  if(account.account?.type!=='chatgpt'&&!process.env.ICU_REFERENCE_GATEWAY_TOKEN)throw Error('wrong account type');
  report.phase='model_tasks';receipt();
  report.executed=true;
  for(const [id,[goal,expected]] of Object.entries(tasks)) {
    const dir=path.join(work,id);fs.mkdirSync(dir);
    const r={id,goal,expected,passed:false};report.cases.push(r);
    const fx=cp.spawn(fixture,[dir],{stdio:'ignore'});children.push(fx);
    try {
      await wait(()=>readState(dir));
      const {thread}=await request('thread/start',{cwd:path.join(root,'model-work'),model,approvalPolicy:'on-request',sandbox:'workspace-write'});
      r.thread_id=thread.id;
      const prompt='Use Computer Use through node_repl and @oai/sky for every UI action. Import sky from @oai/sky, use sky.list_windows() to find PairFixture.exe with window title Paired task fixture, then sky.get_window_state({window,include_screenshot:true,include_text:true}). Use only that disposable native fixture. Do not use shell commands, PowerShell, files, or other tools to act on the application. '+goal;
      const started=Date.now();r.executed=true;
      const {turn}=await request('turn/start',{threadId:thread.id,input:[{type:'text',text:prompt,text_elements:[]}]});
      const done=await wait(()=>{
        if(server.exitCode!==null||server.signalCode!==null)throw Error('reference server exited during the task');
        return notes.find(n=>n.method==='turn/completed'&&n.params.turn.id===turn.id);
      },turnMs);
      r.status=done.params.turn.status;r.error=done.params.turn.error;r.wall_seconds=(Date.now()-started)/1000;
      const full=await request('thread/read',{threadId:thread.id,includeTurns:true});
      fs.writeFileSync(path.join(dir,'thread.json'),JSON.stringify(full,null,2));
      const state=readState(dir);r.outcome=Object.fromEntries(Object.keys(expected).map(k=>[k,state[k]]));
      r.passed=r.status==='completed'&&JSON.stringify(r.outcome)===JSON.stringify(expected);
    } catch(e) {r.error=String(e);} finally {await stop(fx);receipt();}
  }
  report.passed=report.cases.length===2&&report.cases.every(r=>r.passed);
} catch(e) {report.error=String(e);} finally {
  for(const p of children)await stop(p);
  await stop(server);
  report.cleanup_verified=children.every(p=>p.exitCode!==null||p.signalCode!==null)&&(!server||server.exitCode!==null||server.signalCode!==null);
  report.passed=report.passed&&report.cleanup_verified;
  fs.writeFileSync(path.join(work,'result.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report));
  process.exitCode=report.passed?0:1;
}
