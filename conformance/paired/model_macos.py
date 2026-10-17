#!/usr/bin/env python3
"""Real Codex model turns on the same owned native fixture as scripted comparison.

The reference uses the signed, logged-in Codex app-server and its real Computer
Use tool. Fixture state is read independently; no tool reply substitutes for it.
"""
import argparse,json,os,subprocess,sys,time
from pathlib import Path
from macos import APP,sha,wait,state

TASKS={
 'calculator':('Compute six times seven by using the calculator buttons. Read the result before finishing.',{'display':'42','draft':'seed','Alpha':'alpha'}),
 'text_replacement':('Replace the Draft field text with paired text. Keep the Alpha document unchanged.',{'display':'0','draft':'paired text','Alpha':'alpha'}),
}

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--work',type=Path,required=True);p.add_argument('--fixture',type=Path,required=True);p.add_argument('--host-harness',type=Path,default=Path('/Users/runner/golden/remote'));args=p.parse_args()
 if sys.platform!='darwin' or os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable Mac required')
 args.work.mkdir(parents=True,exist_ok=False);sys.path.insert(0,str(args.host_harness));from rpc import Rpc
 codex='/Applications/ChatGPT.app/Contents/Resources/codex';service=Path.home()/'.codex/computer-use/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService'
 approvals=[]
 def approve(msg):
  method=msg.get('method');params=msg.get('params',{});target=(params.get('_meta') or {}).get('tool_params',{}).get('app')
  if method=='mcpServer/elicitation/request':answer={'action':'accept' if target==APP else 'decline','content':{},'_meta':{'persist':'session'}}
  elif method=='item/tool/requestUserInput':answer={'answers':{}}
  else:answer={'decision':'decline'}
  approvals.append({'method':method,'target':target,'answer':answer});return answer
 report={'engine':'openai-native','mode':'model-driven-product','executed':False,'passed':False,'cases':[],
         'engine_sha256':sha(service),'codex_sha256':sha(codex),'fixture_sha256':sha(args.fixture),'harness_sha256':sha(__file__)}
 rpc=None;processes=[]
 try:
  rpc=Rpc([codex,'app-server'],args.work/'appserver.jsonl',cwd='/Users/runner/golden/work',jsonrpc_field=False,on_request=approve)
  rpc.request('initialize',{'clientInfo':{'name':'icu-paired-model','version':'1'},'capabilities':{'experimentalApi':True}},timeout=60);rpc.notify('initialized')
  account=rpc.request('account/read',{},timeout=60);assert (account.get('account') or {}).get('type')=='chatgpt','reference account not signed in'
  report['executed']=True
  for case,(goal,expected) in TASKS.items():
   work=args.work/case;work.mkdir();log=(work/'fixture.log').open('w');fx=subprocess.Popen([str(args.fixture),str(work)],stdout=log,stderr=log);processes.append(fx)
   result={'id':case,'goal':goal,'expected':expected,'executed':False};report['cases'].append(result)
   try:
    wait(lambda:state(work/'state.json'))
    thread=rpc.request('thread/start',{'cwd':'/Users/runner/golden/work','approvalPolicy':'on-request','sandbox':'workspace-write'},timeout=120)['thread'];tid=thread['id'];result['thread_id']=tid
    prompt=f'Use Computer Use through node_repl and @oai/sky for every UI action. The running app has bundle ID {APP}; its main window is Paired task fixture. Observe it with sky.get_app_state. Do not use shell commands, AppleScript, files, or other tools to act on the application. The app is a disposable test fixture. {goal}'
    result['executed']=True;started=time.monotonic()
    turn=rpc.request('turn/start',{'threadId':tid,'input':[{'type':'text','text':prompt,'text_elements':[]}]},timeout=120)['turn']
    complete=rpc.wait_note(lambda n:n.get('method')=='turn/completed' and n['params']['turn']['id']==turn['id'],600)['params']['turn']
    result.update(turn_id=turn['id'],status=complete.get('status'),error=complete.get('error'),wall_seconds=round(time.monotonic()-started,2))
    read=rpc.request('thread/read',{'threadId':tid,'includeTurns':True},timeout=60)
    (work/'thread.json').write_text(json.dumps(read,indent=2)+'\n')
    outcome=state(work/'state.json');result['outcome']={k:outcome[k] for k in expected};result['passed']=complete.get('status')=='completed' and result['outcome']==expected
    subprocess.run(['/usr/sbin/screencapture','-x',str(work/'after.png')],check=True,timeout=20)
   except Exception as e:result.update(passed=False,error=str(e))
   finally:fx.terminate();fx.wait(5);log.close()
  report['passed']=len(report['cases'])==len(TASKS) and all(x.get('passed') for x in report['cases'])
 except Exception as e:report['error']=str(e)
 finally:
  if rpc:rpc.close()
  report['cleanup_verified']=all(p.poll() is not None for p in processes) and (rpc is None or rpc.proc.poll() is not None)
  report['passed']=report['passed'] and report['cleanup_verified'];report['approvals']=approvals
  (args.work/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report),flush=True)
 return 0 if report['passed'] else 1
if __name__=='__main__':raise SystemExit(main())
