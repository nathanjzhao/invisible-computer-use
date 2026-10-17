#!/usr/bin/env python3
"""Packaged recording-import GUI, actual model summaries, and a later native task.

Requires an explicitly owned disposable desktop, an authenticated relay, and an
actual observer fixture recording. Does not collect a person's desktop activity.
"""
import http.server,json,os,signal,sys,threading,time,urllib.request
from pathlib import Path
from macos_probe import Session,wait_for,center

if sys.platform!='darwin' or os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('disposable macOS host required')
work=Path(sys.argv[1]);bundle=Path(sys.argv[2]);fixture=Path(sys.argv[3]);ui=Path(sys.argv[4]);recording=Path(sys.argv[5]);relay=os.environ['ICU_TEST_MODEL_URL']
work.mkdir(parents=True,exist_ok=False);memory=work/'memory';memory.mkdir(mode=0o700)
(memory/'settings.json').write_text(json.dumps({'chatMemories':True,'skysight':False,'paused':False,'retentionDays':7,'idleMinutes':1,'provider':'','extractModel':None,'consolidationModel':None}))
report={'passed':False,'stage':'setup'};s=Session(work,bundle,fixture,ui,report);calls=[];server=None
class Relay(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers['Content-Length']));entry={'request':json.loads(body)};calls.append(entry)
  try:
   with urllib.request.urlopen(urllib.request.Request(relay+self.path,body,{'content-type':'application/json'}),timeout=180) as r:status=r.status;data=r.read()
  except Exception as e:status=502;data=json.dumps({'error':str(e).replace(relay,'<relay>')}).encode()
  entry.update(status=status,response=json.loads(data),complete=True)
  self.send_response(status);self.send_header('Content-Length',str(len(data)));self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(data)
def node(name,role):return wait_for(lambda:s.find(s.named(name,role)),15,f'{name} unavailable')
def click(name,role='AXButton'):
 n=node(name,role)
 # The memory panel scrolls independently of the main app. Keep user setup
 # clicks inside the visible panel instead of aiming at an offscreen AX frame.
 for _ in range(0 if name == 'Memory on' else 12):
  f=n['frame']
  if f['y']>=180 and f['y']+f['h']<=860:break
  s.ui('scroll',1450,500,-5 if f['y']+f['h']>860 else 5);time.sleep(.1);n=node(name,role)
 s.ui('click',*center(n))
def fill(name,value):
 f=node(name,'AXTextField');s.ui('click',*center(f));s.ui('key',0,'cmd');s.ui('type',value)
 wait_for(lambda:node(name,'AXTextField').get('value')==value,8,f'{name} not filled')
def selected(name,downs):
 s.ui('press',s.app_pid,json.dumps({'role':'AXPopUpButton','name':name}))
 time.sleep(.2)
 if name == 'Recorded activity':
  item=s.find(s.named('Test fixture','AXMenuItem'))
  if item:
   s.ui('press',s.app_pid,json.dumps({'role':'AXMenuItem','name':'Test fixture'}))
   return
 for _ in range(downs):s.ui('key',125)
 s.ui('key',36)
def state():
 try:return json.loads((work/'fixture/state.json').read_text())
 except (OSError,ValueError):return {}
def deadline(*_):raise TimeoutError('trajectory UI deadline')
signal.signal(signal.SIGALRM,deadline);signal.alarm(600)
try:
 # Resolve only this test's known capture-consent notices before user setup.
 s.screenshot('setup.png')
 for _ in range(4):
  for pid in {w['owner'] for w in s.ui('onscreen')['windows'] if w['owner_name']=='UserNotificationCenter'}:
   nodes=s.tree(pid);allows=[n for n in nodes if n.get('role')=='AXButton' and n.get('name')=='Allow']
   for window in [n for n in nodes if n.get('role')=='AXWindow']:
    descendants=[n for n in nodes if n.get('path','').startswith(window['path']+'.')]
    text=' '.join(n.get('name','') for n in descendants if n.get('role')=='AXStaticText')
    if 'requesting to bypass the system private window picker' in text and any(name in text for name in ['“vmguest”','“Codex Computer Use”','“Copilot”']):
     target=next((n for n in descendants if n.get('role')=='AXButton' and n.get('name')=='Allow'),None)
     if target:s.ui('press',pid,json.dumps({'role':'AXButton','name':'Allow','index':allows.index(target)}));break
  time.sleep(.3)
 server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Relay);threading.Thread(target=server.serve_forever,daemon=True).start()
 s.start_app(['ICU_APP_MEMORY_ROOT='+str(memory),'ICU_BRAIN=ollama','ICU_OLLAMA_MODEL=relay-to-real-model'])
 click('Memory on');click('Learn from a recording','AXDisclosureTriangle')
 fill('Recording folder',str(recording));click('Review recording')
 wait_for(lambda:node('App','AXPopUpButton'),10,'preview missing')
 report['stage']='review'
 s.screenshot('review.png')
 fill('Project label','paired-benchmark');fill('Task description','This demonstration shows the calculation controls for multiplication in PairFixture.')
 selected('Recorded activity',3)
 wait_for(lambda:node('Recorded activity','AXPopUpButton').get('value')=='Test fixture',5,'test actor selection did not take')
 consent_name='I have permission to use this recording and share all eligible event labels from the selected app and the task description with the selected model.'
 # Use the checkbox's own accessibility action. Its frame includes a wrapped
 # label, whose geometric center need not be on the checkable control.
 s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':consent_name}))
 wait_for(lambda:node(consent_name,'AXCheckBox').get('value')=='1',5,'recording consent was not checked')
 click('Import into memory')
 wait_for(lambda:any('The full recording will be summarized' in n.get('name','') for n in s.tree()),150,'import did not finish')
 report['stage']='summary'
 def durable():
  try:return json.loads((memory/'durable.json').read_text())['entries']
  except (OSError,ValueError,KeyError):return []
 entries=wait_for(durable,200,'recording produced no durable memory')
 summaries=[json.loads(p.read_text()) for p in (memory/'summaries').glob('*.json')]
 report.update(entries=entries,summaries=summaries)
 assert all(r['actor']=='synthetic_fixture' for sm in summaries for r in sm['recordings'])
 assert all(e['untrusted'] for e in entries)
 click('Memory on')
 # Fresh native fixture for the later task. Setup uses the same packaged test app.
 d=work/'fixture';d.mkdir();fx=s.launch([str(fixture),str(d)],'reuse-fixture');wait_for(state,10,'fixture not ready')
 s.pick_window('Paired task fixture')
 s.enter_goal('Compute six times seven using the calculation controls.');s.press('Send')
 wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'later task did not start')
 report['stage']='reuse'
 wait_for(lambda:state().get('display')=='42',200,'later task did not reach 42')
 wait_for(lambda:s.find(s.named('Stop','AXButton')) is None and s.find(s.named('Send','AXButton')),60,'task not terminal')
 wait_for(lambda:all(c.get('complete') for c in calls),180,'model requests unfinished')
 report['fixture_outcome']=state();s.screenshot('reused-memory.png')
 from trajectory_verdict import memory_decisions
 report['decisions_with_memory']=memory_decisions(calls,entries)
 assert report['decisions_with_memory']>0,'later user prompt lacked the actual imported memory note'
 def reused_journal():
  rows=[json.loads(p.read_text()) for p in (memory/'runs').glob('*.json')]
  return next((r for r in rows if r.get('brain')!='recorded-demonstration' and r.get('used_memory')),None)
 journal=wait_for(reused_journal,120,'task journal did not cite the imported memory')
 assert journal.get('bundle_id')=='dev.icu.paired.fixture','task lost exact application identity'
 assert set(journal['used_memory']) & {e['id'] for e in entries},'journal cited different memory'
 report['reused_journal']=journal
 report['passed']=True
except Exception as e:
 report['error']=str(e);report['ui_nodes']=s.tree() if s.app_pid else []
 s.screenshot('failure.png');raise
finally:
 signal.alarm(0)
 if server:server.shutdown();server.server_close()
 s.close();report['calls']=calls
 report['passed']=report['passed'] and report.get('owned_daemon_exited') is True and report.get('owned_processes_reaped') is True
 (work/'result.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({k:v for k,v in report.items() if k not in ['calls','ui_nodes','entries','summaries']}))
raise SystemExit(0 if report['passed'] else 1)
