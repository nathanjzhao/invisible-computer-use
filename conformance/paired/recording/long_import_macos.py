#!/usr/bin/env python3
"""Review every page of a real long recording, import it, and verify all sources survive."""
import argparse,collections,hashlib,http.server,json,os,re,subprocess,sys,threading,time,urllib.request
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--source',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable host required')
sys.path.insert(0,str(a.root/'tests'));from macos_probe import Session,wait_for,center
out=a.out;out.mkdir(parents=True,exist_ok=False);calls=[];report={'passed':False,'mode':'recording-gui-import-and-model-summary','model':'gpt-6-astra'};s=None
class Relay(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  data=self.rfile.read(int(self.headers['Content-Length']));row={'request':json.loads(data)};calls.append(row)
  try:
   with urllib.request.urlopen(urllib.request.Request(os.environ['ICU_TEST_MODEL_URL']+self.path,data,{'content-type':'application/json'}),timeout=180) as response:status=response.status;body=response.read()
  except Exception:status=502;body=b'{"error":"model relay unavailable"}'
  row.update(status=status,response=json.loads(body));self.send_response(status);self.send_header('Content-Length',str(len(body)));self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(body)
def node(name,role):return wait_for(lambda:s.find(s.named(name,role)),10,name+' absent')
def press(name,role='AXButton'):
 n=node(name,role)
 if role=='AXDisclosureTriangle':s.ui('click',*center(n))
 else:s.ui('press',s.app_pid,json.dumps({'role':role,'name':name}))
 time.sleep(.15)
def fill(name,value):
 for _ in range(25):
  n=node(name,'AXTextField');f=n['frame'];w=node('Copilot','AXWindow')['frame']
  if w['y']+150<=f['y'] and f['y']+f['h']<=w['y']+w['h']-55:break
  s.ui('scroll',w['x']+w['w']-180,w['y']+w['h']*.6,-5 if f['y']+f['h']>w['y']+w['h']-55 else 5);time.sleep(.1)
 s.ui('click',*center(n));s.ui('key',0,'cmd');s.ui('type',value);wait_for(lambda:node(name,'AXTextField').get('value')==value,5,'input mismatch')
def page_lines():return [int(m[1]) for n in s.tree() if n.get('role')=='AXStaticText' and (m:=re.match(r'^Line (\d+):',n.get('name','')))]
server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Relay);threading.Thread(target=server.serve_forever,daemon=True).start()
try:
 events=[json.loads(line) for line in (a.source/'events.jsonl').read_text().splitlines() if line]
 def identity(e):
  bundle=e['payload'].get('bundle_id','')
  return bundle if re.fullmatch(r'[A-Za-z0-9._-]+',bundle) else e['payload'].get('app','')
 app=collections.Counter(identity(e) for e in events if e['kind']=='ui.click').most_common(1)[0][0]
 selected=[(i+1,e) for i,e in enumerate(events) if e['kind'] in ['ui.click','ui.focus','ui.value','ui.drag','ui.scroll'] and identity(e)==app and not e['payload'].get('secure')]
 expected=[i for i,e in sorted(selected,key=lambda row:(row[1]['ts_mono_ns'],row[0]))];assert len(expected)>80
 report.update(app=app,expected_lines=expected,source_sha256=hashlib.sha256((a.source/'events.jsonl').read_bytes()).hexdigest())
 memory=out/'memory';memory.mkdir();(memory/'settings.json').write_text(json.dumps({'chatMemories':True,'skysight':False,'paused':False,'retentionDays':7,'idleMinutes':1,'provider':''}))
 s=Session(out,a.root/'Copilot.app',a.root/'MemoryLab.app/Contents/MacOS/MemoryLab',a.root/'macos_ui',report)
 s.start_app(['ICU_APP_MEMORY_ROOT='+str(memory),'ICU_BRAIN=ollama','ICU_OLLAMA_MODEL=gpt-6-astra-via-local-relay'])
 press('Memory on');press('Learn from a recording','AXDisclosureTriangle');fill('Recording folder',str(a.source));press('Review recording')
 press('App','AXPopUpButton');option=wait_for(lambda:s.find(s.named(app+' (','AXMenuItem',exact=False)),10,'recorded app option absent');s.ui('press',s.app_pid,json.dumps({'role':'AXMenuItem','name':option['name']}))
 pages=(len(expected)+39)//40;reviewed=[];report['pages']=[]
 for page in range(pages):
  lines=wait_for(lambda:page_lines() if s.chat_text('Showing chunk '+str(page+1)+'.') else None,12,'recording page missing')
  report['pages'].append(lines);reviewed.extend(lines)
  if page+1<pages:press('Next events')
 assert reviewed==expected,'review pagination lost or repeated a source event'
 fill('Project label','long-recording');fill('Task description','This recording shows repeated pointer interactions with the Pointer pad.')
 press('Recorded activity','AXPopUpButton');press('Test fixture','AXMenuItem')
 consent='I have permission to use this recording and share all eligible event labels from the selected app and the task description with the selected model.'
 press(consent,'AXCheckBox');press('Import into memory');wait_for(lambda:s.chat_text('The full recording will be summarized'),90,'import failed')
 runs=[json.loads(p.read_text()) for p in (memory/'runs').rglob('*.json')];runs=sorted([r for r in runs if r.get('trajectory')],key=lambda r:r['trajectory']['chunk_index']);report['runs']=runs
 assert len(runs)==pages and [line for r in runs for line in r['trajectory']['source_lines']]==expected,'persisted chunks lost events'
 assert all(r['trajectory']['recording_sha256']==report['source_sha256'] and r['trajectory']['actor']=='synthetic_fixture' for r in runs)
 assert all(r.get('bundle_id')!='[ NULL ]' and r['app_name']==app for r in runs),'unknown bundle placeholder became an app scope'
 def summaries_ready():
  summaries=[json.loads(p.read_text()) for p in (memory/'summaries').glob('*.json')]
  records=[r for summary in summaries for r in summary.get('recordings',[])];lines=[line for r in sorted(records,key=lambda r:r['chunk_index']) for line in r['source_lines']]
  return summaries if lines==expected and all(s['consolidated'] for s in summaries) else None
 report['summaries']=wait_for(summaries_ready,240,'not every chunk was summarized');assert all(s['untrusted'] for s in report['summaries'])
 report['passed']=bool(calls) and any(c['status']==200 for c in calls);s.screenshot('after.png')
except Exception as error:report['error']=str(error)
finally:
 if s:s.close()
 server.shutdown();report['calls']=calls;report['passed'] &= report.get('owned_daemon_exited') is True and report.get('owned_processes_reaped') is True
 (out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k not in ['calls','runs','summaries']}))
raise SystemExit(0 if report['passed'] else 1)
