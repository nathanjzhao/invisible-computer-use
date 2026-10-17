import {getQuickJS} from 'quickjs-emscripten';
const MAX_CODE=65536,MAX_OUTPUT=4*1024*1024,MAX_CALLS=32;
const OPERATOR=/^(approve_app|approve_prepared|confirm_secure_input|stop_session|resume_session)$/;
/** Persistent guest state with no Node globals, module loader, filesystem or network. */
export class Session{
 static async create(options){const s=new Session(options);s.module=await getQuickJS();s.reset();return s;}
 constructor({dispatch,tools=[]}){this.dispatch=dispatch;this.tools=new Set(tools.filter(n=>!OPERATOR.test(n)));this.generation=0;this.active=null;this.inFlight=0;}
 reset(){if(this.active)throw Error('cannot reset a running cell');this.destroy();this.runtime=this.module.newRuntime();this.runtime.setMemoryLimit(32*1024*1024);this.runtime.setMaxStackSize(512*1024);this.context=this.runtime.newContext();this.generation++;
  const c=this.context;this.runtime.setModuleLoader(()=>{throw Error('module imports are unavailable; use icu.call for computer APIs');});this.runtime.setInterruptHandler(()=>!this.active||Date.now()>this.active.deadline||this.active.stopped);
  c.newFunction('__emit',h=>{const a=this.requireActive();const text=c.getString(h);a.bytes+=Buffer.byteLength(text);if(a.bytes>MAX_OUTPUT)throw Error('cell output exceeds 4 MiB');const output=JSON.parse(text);
   if(output?.type==='text'&&typeof output.text==='string')a.outputs.push({type:'text',text:output.text});
   else if(output?.type==='image'&&['image/png','image/jpeg'].includes(output.mimeType)&&typeof output.data==='string'){
    if(!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(output.data))throw Error('invalid image base64');
    const bytes=Buffer.from(output.data,'base64');
    if(output.mimeType==='image/png'?bytes.length<24||bytes.subarray(0,8).toString('hex')!=='89504e470d0a1a0a':bytes.length<4||bytes[0]!==255||bytes[1]!==216||bytes.at(-2)!==255||bytes.at(-1)!==217)throw Error('image signature mismatch');
    a.outputs.push({type:'image',data:output.data,mimeType:output.mimeType});
   }else throw Error('unsupported cell output');}).consume(h=>c.setProp(c.global,'__emit',h));
  c.newFunction('__call',(nameH,argsH)=>{
   const a=this.requireActive();const name=c.getString(nameH),wire=c.getString(argsH);
   if(!this.tools.has(name))throw Error('tool is unavailable to evaluated code: '+name);
   if(wire.length>1024*1024)throw Error('tool arguments exceed 1 MiB');
   if(++a.callCount>MAX_CALLS)throw Error('cell exceeds 32 tool calls');
   if(a.pending.size)throw Error('await the previous computer call before starting another');
   const args=JSON.parse(wire);if(!args||Array.isArray(args)||typeof args!=='object')throw Error('tool arguments must be an object');
   const deferred=c.newPromise();a.pending.add(deferred);const generation=this.generation;
   this.inFlight++;
   Promise.resolve().then(()=>{if(this.active!==a||a.stopped)throw Error('cell stopped before dispatch');return this.dispatch(name,args,{signal:a.controller.signal});}).then(value=>{
    if(this.generation!==generation||this.active!==a)return;
    const encoded=JSON.stringify(value??null);if(Buffer.byteLength(encoded)>8*1024*1024)throw Error('tool result exceeds 8 MiB');
    c.newString(encoded).consume(h=>deferred.resolve(h));
   }).catch(error=>{if(this.generation===generation&&this.active===a)c.newError(String(error.message||error)).consume(h=>deferred.reject(h));}).finally(()=>{this.inFlight--;if(this.generation===generation&&this.active===a)a.pending.delete(deferred);});
   return deferred.handle;
  }).consume(h=>c.setProp(c.global,'__call',h));
  // Bootstrap has its own small budget; host functions still reject calls outside a cell.
  this.active={deadline:Date.now()+1000,stopped:false};
  try{c.unwrapResult(c.evalCode(`
   Object.defineProperty(globalThis,'state',{value:Object.create(null)});
   Object.defineProperty(globalThis,'icu',{value:Object.freeze({call:async(name,args={})=>JSON.parse(await __call(name,JSON.stringify(args)))} )});
   Object.defineProperty(globalThis,'emit',{value:(value)=>__emit(JSON.stringify({type:'text',text:JSON.stringify(value)}))});
   Object.defineProperty(globalThis,'emitImage',{value:(data,mimeType='image/png')=>{if(!['image/png','image/jpeg'].includes(mimeType)||typeof data!=='string')throw Error('PNG or JPEG base64 required');__emit(JSON.stringify({type:'image',data,mimeType}));}});
  `)).dispose();}finally{this.active=null;}
 }
 requireActive(){const a=this.active;if(!a||a.stopped||Date.now()>a.deadline)throw Error('cell is no longer active');return a;}
 stop(){if(this.active){this.active.stopped=true;this.active.controller?.abort();}}
 async execute(code,{timeoutMs=30000}={}){
  if(this.active)throw Error('a cell is already running');
  if(this.inFlight)throw Error('a previous native call is still settling; do not replay it');
  if(typeof code!=='string'||Buffer.byteLength(code)>MAX_CODE)throw Error('code exceeds 64 KiB');
  if(!Number.isInteger(timeoutMs)||timeoutMs<1||timeoutMs>120000)throw Error('timeout must be 1..120000 milliseconds');
  const a={deadline:Date.now()+timeoutMs,stopped:false,controller:new AbortController(),outputs:[],bytes:0,callCount:0,pending:new Set()};this.active=a;const c=this.context;let handle;let failure;
  try{
   handle=c.unwrapResult(c.evalCode(`(async()=>{\n${code}\n})()`, 'cell.js'));
   for(;;){
    if(a.stopped||Date.now()>a.deadline)throw Error(a.stopped?'cell stopped':'cell deadline exceeded');
    const jobs=this.runtime.executePendingJobs(32);if(jobs.error){const error=c.dump(jobs.error);jobs.error.dispose();throw Error(error.message||String(error));}
    const status=c.getPromiseState(handle);
    if(status.type==='fulfilled'){status.value.dispose();if(a.pending.size)throw Error('cell left a computer call unawaited; its effect may be uncertain');break;}
    if(status.type==='rejected'){const error=c.dump(status.error);status.error.dispose();throw Error(error.message||String(error));}
    await new Promise(r=>setTimeout(r,1));
   }
  }catch(error){failure=String(error.message||error);}
  finally{a.controller.abort();if(handle?.alive)handle.dispose();for(const p of a.pending)p.dispose();this.active=null;}
  const result={ok:!failure,outputs:a.outputs,calls:a.callCount,generation:this.generation,...(failure?{error:failure,state_reset:true}: {})};
  if(failure)this.reset();return result;
 }
 destroy(){if(this.active)throw Error('stop and await the current cell before closing');this.context?.dispose();this.runtime?.dispose();this.context=null;this.runtime=null;}
}
