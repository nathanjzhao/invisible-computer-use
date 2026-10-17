#!/usr/bin/env python3
"""Identical goals on real macOS apps, independently graded from AX/file state."""
import argparse,http.server,json,os,signal,subprocess,sys,threading,time,urllib.request,unicodedata,shutil
from pathlib import Path
from model_completion import candidate_completed
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--engine',choices=['candidate','reference'],required=True);a=p.parse_args()
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable desktop required')
sys.path.insert(0,str(a.root/'tests'));sys.path.insert(0,'/Users/runner/golden/remote');from macos_probe import Session,wait_for,HARNESS_CAPTURE_PROMPT;from rpc import Rpc
ui=a.root/'macos_ui';bundle=a.root/'Copilot.app';fixture=a.root/'MemoryLab.app/Contents/MacOS/MemoryLab';a.out.mkdir(parents=True,exist_ok=False)
model='gpt-6-astra';calls=[];report={'engine':a.engine,'model':model,'cases':[],'passed':False};s=None;rpc=None;server=None
class Relay(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers['Content-Length']));row={'request':json.loads(body)};calls.append(row)
  try:
   with urllib.request.urlopen(urllib.request.Request(os.environ['ICU_TEST_MODEL_URL']+self.path,body,{'content-type':'application/json'}),timeout=180) as r:status=r.status;data=r.read()
  except Exception:status=502;data=b'{"error":"model relay failed"}'
  row.update(status=status,response=json.loads(data));self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
def native(*args):return json.loads(subprocess.check_output([str(ui),*map(str,args)],text=True))
def app_pid(bundle):return wait_for(lambda:(native('pid-of',bundle)['pids'] or [None])[0],15,'application missing')
def stop_app(bundle):
 for pid in native('pid-of',bundle)['pids']:
  native('quit',pid);time.sleep(.2)
  try:os.kill(pid,0)
  except ProcessLookupError:continue
  os.kill(pid,signal.SIGTERM)
  wait_for(lambda:pid not in native('pid-of',bundle)['pids'],8,'application did not exit')
def approve(msg):
 target=(msg.get('params',{}).get('_meta') or {}).get('tool_params',{}).get('app');allowed=isinstance(target,str) and target.lower() in ['com.apple.calculator','com.apple.textedit','calculator','textedit']
 answer={'action':'accept' if allowed else 'decline','content':{},'_meta':{'persist':'session'}} if msg.get('method')=='mcpServer/elicitation/request' else {'answers':{}} if msg.get('method')=='item/tool/requestUserInput' else {'decision':'decline'}
 report.setdefault('approvals',[]).append({'method':msg.get('method'),'target':target,'answer':answer});return answer
try:
 if a.engine=='candidate':server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Relay);threading.Thread(target=server.serve_forever,daemon=True).start()
 else:
  rpc=Rpc(['/Applications/ChatGPT.app/Contents/Resources/codex','app-server'],a.out/'appserver.jsonl',cwd='/Users/runner/golden/work',jsonrpc_field=False,on_request=approve)
  rpc.request('initialize',{'clientInfo':{'name':'icu-real-apps','version':'1'},'capabilities':{'experimentalApi':True}},timeout=60);rpc.notify('initialized')
 for case in ['calculator','textedit_save','textedit_two_documents']:
  work=a.out/case;work.mkdir();row={'case':case,'executed':False,'passed':False};report['cases'].append(row);before=len(calls)
  target='com.apple.calculator' if case=='calculator' else 'com.apple.TextEdit';stop_app(target)
  if target=='com.apple.TextEdit':
   subprocess.run(['defaults','write',target,'NSQuitAlwaysKeepsWindows','-bool','false'],check=True)
   for index,saved in enumerate([Path.home()/'Library/Saved Application State/com.apple.TextEdit.savedState',Path.home()/'Library/Containers/com.apple.TextEdit/Data/Library/Saved Application State/com.apple.TextEdit.savedState']):
    if saved.exists():shutil.move(str(saved),str(work/('saved-state-before-'+str(index))))
  try:
   if case=='calculator':
    subprocess.run(['open','-a','Calculator'],check=True);pid=app_pid(target);title='Calculator';goal='Clear the calculator, compute six times seven, and read the result before finishing.'
    def calculator_ready():
     ns=native('tree',pid)['nodes'];return ns if any(n.get('role')=='AXButton' and 'clear' in n.get('name','').lower() for n in ns) else None
    nodes=wait_for(calculator_ready,12,'calculator controls did not become ready')
    clears=[n for n in nodes if n.get('role')=='AXButton' and ('clear' in n.get('name','').lower() or n.get('name','').lower()=='ac')]
    reset=next((n for n in clears if 'all' in n.get('name','').lower()),clears[0] if len(clears)==1 else None)
    if not reset:raise RuntimeError('calculator reset control not identified')
    native('press',pid,json.dumps({'role':'AXButton','name':reset['name']}));time.sleep(.3);row['before_nodes']=native('tree',pid)['nodes']
    before_values=[''.join(c for c in str(n.get('value',n.get('name',''))) if unicodedata.category(c)!='Cf') for n in row['before_nodes'] if n.get('role') in ['AXStaticText','AXTextField']]
    if '0' not in before_values or '42' in before_values:raise RuntimeError('calculator did not reset to an independently verified zero')
   else:
    document=work/'Beta.txt';document.write_text('seed text\n');alpha=work/'Alpha.txt';alpha.write_text('keep this document unchanged\n')
    if case=='textedit_two_documents':subprocess.run(['open','-a','TextEdit',str(alpha)],check=True);time.sleep(.5)
    subprocess.run(['open','-a','TextEdit',str(document)],check=True);pid=app_pid(target);title='Beta.txt'
    wait_for(lambda:any(n.get('role')=='AXWindow' and 'Beta' in n.get('name','') for n in native('tree',pid)['nodes']),15,'TextEdit document did not become ready')
    goal='Replace Beta.txt with exactly two lines: first line "Round-three note"; second line "Native save verified." (omit the quotation marks). Save the document.' 
    if case=='textedit_two_documents':goal+=' Keep Alpha.txt unchanged.'
    row['expected_text']='Round-three note\nNative save verified.'
   row.update(goal=goal,target_bundle=target)
   if a.engine=='candidate':
    s=Session(work,bundle,fixture,ui,row);s.start_app(['ICU_APP_MEMORY_ROOT='+str(work/'memory'),'ICU_BRAIN=ollama','ICU_OLLAMA_MODEL=gpt-6-astra-via-local-relay'])
    subprocess.run(['/usr/sbin/screencapture','-x',str(work/'capture-setup.png')],check=True)
    s.approve_capture_prompt(prompt=HARNESS_CAPTURE_PROMPT)
    for label in ['Give this task the stored memories for its app','Let memory learn from this task']:
     n=s.find(s.named(label,'AXCheckBox'))
     if n and n.get('value')=='1':s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':label}))
    row['available_windows']=s.windows()
    if case=='calculator':
     target_windows=[w for w in row['available_windows'] if w.get('pid')==pid]
     if len(target_windows)==1:title=target_windows[0].get('title') or target_windows[0].get('app_name') or title
    s.pick_window(title)
    focus=next((n for n in s.tree() if n.get('role')=='AXCheckBox' and ('foreground' in n.get('name','').lower() or 'This task may bring' in n.get('name',''))),None)
    if focus:
     s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':focus['name']}));row['foreground_explicitly_allowed']=True
    s.enter_goal(goal);s.press('Send');row['executed']=True;started=time.monotonic()
    wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'task did not start');wait_for(lambda:s.find(s.named('Stop','AXButton')) is None and s.find(s.named('Send','AXButton')),240,'task did not end')
    row['seconds']=time.monotonic()-started;row['calls']=calls[before:];row['chat']=[n.get('name') for n in s.tree() if n.get('role')=='AXStaticText']
    s.screenshot('after.png');s.close();s=None
   else:
    thread=rpc.request('thread/start',{'cwd':'/Users/runner/golden/work','model':model,'approvalPolicy':'on-request','sandbox':'workspace-write'},timeout=120)['thread'];row['thread_id']=thread['id']
    prompt=f'Use only Computer Use through node_repl and @oai/sky for this task. The running app bundle ID is {target}; target the window titled {title}. Use the native app for every action and observation. Do not run shell commands, AppleScript, or other code or file tools. Do not read the filesystem or fixture/oracle files. These documents belong to an isolated acceptance test. '+goal
    row['executed']=True;started=time.monotonic();turn=rpc.request('turn/start',{'threadId':thread['id'],'input':[{'type':'text','text':prompt,'text_elements':[]}]},timeout=120)['turn']
    terminal=rpc.wait_note(lambda n:n.get('method')=='turn/completed' and n['params']['turn']['id']==turn['id'],400)['params']['turn'];row['seconds']=time.monotonic()-started;row['status']=terminal.get('status');row['error']=terminal.get('error')
    trace=rpc.request('thread/read',{'threadId':thread['id'],'includeTurns':True},timeout=60);(work/'thread.json').write_text(json.dumps(trace,indent=2)+'\n')
    subprocess.run(['/usr/sbin/screencapture','-x',str(work/'after.png')],check=True)
   if case=='calculator':
    nodes=native('tree',pid)['nodes'];row['oracle_nodes']=nodes;row['passed']=any(''.join(c for c in str(n.get(k,'')) if unicodedata.category(c)!='Cf')=='42' for n in nodes for k in ['name','value'] if n.get('role') in ['AXStaticText','AXTextField'])
   else:
    row['document']=document.read_text();row['alpha']=alpha.read_text();row['passed']=row['document'] in [row['expected_text'],row['expected_text']+'\n'] and row['alpha']=='keep this document unchanged\n'
   row['outcome_passed']=row['passed']
   if a.engine=='reference':row['model_completed']=row.get('status')=='completed'
   else:row['model_completed']=candidate_completed(row.get('calls',[]),row.get('chat',[]))
   row['passed'] &= row['model_completed']
  except Exception as e:
   row['error']=str(e)
   if s:
    try:row['ui_nodes']=s.tree();s.screenshot('failure.png')
    except Exception:pass
  finally:
   if s:s.close();s=None
   stop_app(target);(work/'result.json').write_text(json.dumps(row,indent=2)+'\n');(a.out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in row.items() if k not in ['before_nodes','oracle_nodes','calls','chat','available_windows','ui_nodes']}),flush=True)
 report['passed']=len(report['cases'])==3 and all(r['passed'] for r in report['cases'])
finally:
 if s:s.close()
 if rpc:rpc.close()
 if server:server.shutdown()
 (a.out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'engine':a.engine,'passed':report['passed']}))
raise SystemExit(0 if report['passed'] else 1)
