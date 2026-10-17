#!/usr/bin/env python3
"""Registered, paired actual-model trials through the packaged native UI."""
import argparse,http.server,json,os,shutil,subprocess,sys,threading,time,urllib.request
from pathlib import Path
from model_completion import candidate_completed
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--registration',type=Path,required=True);p.add_argument('--train-only',action='store_true');p.add_argument('--training',type=Path);p.add_argument('--controls',action='store_true');a=p.parse_args()
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable host required')
sys.path.insert(0,str(a.root/'tests'));from macos_probe import Session,wait_for,center
relay=os.environ['ICU_TEST_MODEL_URL'];a.out.mkdir(parents=True,exist_ok=False);calls=[]
class Relay(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers['Content-Length']));row={'request':json.loads(body),'started':time.time()};calls.append(row)
  try:
   with urllib.request.urlopen(urllib.request.Request(relay+self.path,body,{'content-type':'application/json'}),timeout=180) as r:status=r.status;data=r.read()
  except Exception:status=502;data=b'{"error":"model relay failed; inspect protected local relay log"}'
  row.update(status=status,response=json.loads(data),elapsed=time.time()-row['started']);self.send_response(status);self.send_header('Content-Length',str(len(data)));self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(data)
server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Relay);threading.Thread(target=server.serve_forever,daemon=True).start()
settings={'chatMemories':True,'skysight':False,'paused':False,'retentionDays':7,'idleMinutes':1,'provider':'','extractModel':None,'consolidationModel':None}
fixture=a.root/'MemoryLab.app/Contents/MacOS/MemoryLab';bundle=a.root/'Copilot.app';ui=a.root/'macos_ui';report={'model':'gpt-6-astra','transport':'authenticated loopback relay using production Codex adapter','passed':False,'cases':[]};session=None

def node(s,name,role):return wait_for(lambda:s.find(s.named(name,role)),15,name+' missing')
def press(s,name,role='AXButton'):
 n=node(s,name,role)
 if role=='AXDisclosureTriangle':s.ui('click',*center(n))
 else:s.ui('press',s.app_pid,json.dumps({'role':role,'name':name}))
 time.sleep(.2)
def fill(s,name,value):
 n=node(s,name,'AXTextField')
 for _ in range(15):
  frame=node(s,'Copilot','AXWindow')['frame'];f=n['frame'];top=frame['y']+160;bottom=frame['y']+frame['h']-55
  if f['y']>=top and f['y']+f['h']<=bottom:break
  s.ui('scroll',frame['x']+frame['w']-180,frame['y']+frame['h']*.6,-5 if f['y']+f['h']>bottom else 5);time.sleep(.15);n=node(s,name,'AXTextField')
 s.ui('click',*center(n));s.ui('key',0,'cmd');s.ui('type',value);wait_for(lambda:node(s,name,'AXTextField').get('value')==value,8,'text did not reach '+name)
def imported_entries(memory):
 try:return json.loads((memory/'durable.json').read_text())['entries']
 except (OSError,ValueError,KeyError):return []
def check(s,label,on):
 n=node(s,label,'AXCheckBox')
 if (n.get('value')=='1')!=on:press(s,label,'AXCheckBox')
def launch(work,memory):
 global session
 row={};session=Session(work,bundle,fixture,ui,row);session.start_app(['ICU_APP_MEMORY_ROOT='+str(memory),'ICU_BRAIN=ollama','ICU_OLLAMA_MODEL=gpt-6-astra-via-local-relay']);return session,row
try:
 if a.train_only:
  memory=a.out/'memory';memory.mkdir();(memory/'settings.json').write_text(json.dumps(settings));s,health=launch(a.out,memory)
  press(s,'Memory on');press(s,'Learn from a recording','AXDisclosureTriangle')
  families=list(dict.fromkeys(pair['family'] for pair in json.loads(a.registration.read_text())['pairs']))
  for family in families:
   fill(s,'Recording folder',str(a.root/'demos-1'/family/'recording-demo'));press(s,'Review recording');node(s,'App','AXPopUpButton')
   fill(s,'Project label',family);fill(s,'Task description','This demonstration shows the '+family.replace('-',' ')+' workflow in Workflow Lab.')
   press(s,'Recorded activity','AXPopUpButton');time.sleep(.1);press(s,'Test fixture','AXMenuItem')
   consent='I have permission to use this recording and share all eligible event labels from the selected app and the task description with the selected model.'
   check(s,consent,True);press(s,'Import into memory')
   wait_for(lambda:s.chat_text('The full recording will be summarized'),150,'import failed')
   report['cases'].append({'family':family,'imported_through_gui':True});s.screenshot('import.png')
  # Summaries have to cite every family before evaluation. Inclusion alone is not benefit.
  def trained():
   summaries=[json.loads(p.read_text()) for p in (memory/'summaries').glob('*.json')]
   covered={(rec.get('project') or '').casefold() for summary in summaries for rec in summary.get('recordings',[])}
   return summaries if set(families)<=covered and imported_entries(memory) else None
  summaries=wait_for(trained,360,'not every demo was summarized');report.update(summaries=summaries,entries=imported_entries(memory),passed=True)
  s.screenshot('trained-memory.png');s.close();session=None;report['cleanup']=health
 else:
  if not a.training:raise ValueError('verified training store required')
  registration=json.loads(a.registration.read_text());training_report=json.loads((a.training/('acceptance.json' if (a.training/'acceptance.json').exists() else 'result.json')).read_text());assert training_report['passed'],'training did not pass'
  pairs=registration['pairs']
  if a.controls:pairs=[{'family':family,'instance':'control','order':['wrong-app-memory','irrelevant-demo'],'value':'control-'+family,'decision_budget':18} for family in registration['controls']]
  for pair in pairs:
   for arm in pair['order']:
    work=a.out/(pair['family']+'-'+str(pair['instance'])+'-'+arm);work.mkdir();memory=work/'memory';shutil.copytree(a.training/'memory',memory)
    active_fixture=fixture
    if arm=='wrong-app-memory':active_fixture=a.root/'OtherMemoryLab.app/Contents/MacOS/MemoryLab'
    if arm=='irrelevant-demo':
     unrelated={'invoice-routing':'expense-review','inventory-transfer':'appointment-booking','support-triage':'document-export','contact-import':'report-filtering'}[pair['family']]
     ids=set()
     for path in (memory/'summaries').glob('*.json'):
      summary=json.loads(path.read_text())
      if any((r.get('project') or '').casefold()==unrelated for r in summary.get('recordings',[])):ids.add(summary['id'])
      else:path.unlink()
     durable=json.loads((memory/'durable.json').read_text());durable['entries']=[e for e in durable['entries'] if set(e['evidence'])<=ids]
     assert durable['entries'],'irrelevant-demo control needs actual imported notes'
     (memory/'durable.json').write_text(json.dumps(durable))
    (memory/'settings.json').write_text(json.dumps({**settings,'paused':True}))
    before=len(calls);s,health=launch(work,memory);row={**pair,'arm':arm,'passed':False,'executed':False};report['cases'].append(row)
    try:
     check(s,'Give this task the stored memories for its app',arm!='without-memory');check(s,'Let memory learn from this task',False)
     data=work/'fixture';data.mkdir();s.launch([str(active_fixture),str(data),pair['family']],'fixture');wait_for(lambda:(data/'state.json').exists(),10,'fixture did not start');s.pick_window('Workflow Lab')
     # Fix the same 18-decision budget in both arms.
     press(s,'Decision budget','AXPopUpButton');s.ui('key',115);s.ui('key',36)
     goal='Complete the '+pair['family'].replace('-',' ')+' workflow for reference '+pair['value']+'. Read the final confirmation before finishing.'
     row['goal']=goal;s.enter_goal(goal);s.press('Send');row['executed']=True;started=time.monotonic();wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'task did not start')
     row['confirmations']=[]
     def terminal():
      allow=s.find(s.named('Allow this step','AXButton'))
      if allow:
       text=' '.join(n.get('name','') for n in s.tree() if n.get('role')=='AXStaticText')
       # Only this synthetic fixture is authorized. Do not approve another app.
       if 'Workflow Lab' not in text:raise RuntimeError('confirmation does not name the owned fixture')
       row['confirmations'].append({'fixture':'Workflow Lab','answer':'allow_this_step','reason':'synthetic fixture operations explicitly authorized for both arms'})
       s.ui('press',s.app_pid,json.dumps({'role':'AXButton','name':'Allow this step'}))
      return s.find(s.named('Stop','AXButton')) is None and s.find(s.named('Send','AXButton'))
     wait_for(terminal,250,'task did not terminate')
     row['seconds']=time.monotonic()-started;row['oracle']=json.loads((data/'state.json').read_text());row['calls']=calls[before:]
     row['memory_present_in_prompt']=any('MEMORY (background from earlier runs' in m.get('content','') for c in row['calls'] for m in c['request'].get('messages',[]) if m.get('role')=='user')
     row['decisions']=sum(c['request'].get('messages',[{}])[0].get('content','').startswith('You control ONE desktop window') for c in row['calls'])
     row['passed']=row['oracle']['completed'] and row['oracle']['reference']==pair['value']
     if arm=='wrong-app-memory':row['passed'] &= not row['memory_present_in_prompt']
     row['chat']=[n.get('name') for n in s.tree() if n.get('role')=='AXStaticText'];row['outcome_passed']=row['passed'];row['model_completed']=candidate_completed(row['calls'],row['chat']);row['passed'] &= row['model_completed'];s.screenshot('after.png')
    except Exception as e:row['error']=str(e);row['calls']=calls[before:];s.screenshot('failure.png')
    finally:s.close();session=None;row['cleanup']=health;(work/'result.json').write_text(json.dumps(row,indent=2)+'\n');(a.out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in row.items() if k not in ['chat','calls','cleanup']}),flush=True)
    if row.get('error'):raise RuntimeError('trial harness failed; remaining trials unexecuted')
    if health.get('owned_daemon_exited') is not True or health.get('owned_processes_reaped') is not True:raise RuntimeError('desktop cleanup failed; remaining trials unexecuted')
  report['passed']=len(report['cases'])==(8 if a.controls else 32) and all(r.get('executed') and not r.get('error') for r in report['cases']) # Completion of the registered experiment, not a claim that every task succeeded.
except Exception as e:
 report['error']=str(e)
 if session:
  try:session.screenshot('failure.png');report['nodes']=session.tree()
  except Exception:pass
finally:
 if session:session.close()
 server.shutdown();(a.out/'calls.json').write_text(json.dumps(calls,indent=2)+'\n');(a.out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k not in ['cases','summaries','entries','nodes']}),flush=True)
raise SystemExit(0 if report['passed'] else 1)
