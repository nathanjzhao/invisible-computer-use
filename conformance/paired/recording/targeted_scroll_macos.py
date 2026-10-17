#!/usr/bin/env python3
import http.server,json,os,subprocess,sys,threading,time,urllib.request
from pathlib import Path
root=Path(sys.argv[1]);work=Path(sys.argv[2]);work.mkdir(parents=True,exist_ok=False)
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable host required')
sys.path.insert(0,str(root/'tests'));from macos_probe import Session,wait_for
report={'passed':False,'model':'gpt-6-astra'};calls=[];s=None
class Relay(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers['Content-Length']));row={'request':json.loads(body)};calls.append(row)
  try:
   with urllib.request.urlopen(urllib.request.Request(os.environ['ICU_TEST_MODEL_URL']+self.path,body,{'content-type':'application/json'}),timeout=180) as r:status=r.status;data=r.read()
  except Exception:status=502;data=b'{"error":"model relay failed"}'
  row.update(status=status,response=json.loads(data));self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Relay);threading.Thread(target=server.serve_forever,daemon=True).start()
try:
 fixture=root/'Expanded.app/Contents/MacOS/Expanded';s=Session(work,root/'Copilot.app',fixture,root/'macos_ui',report);s.start_app(['ICU_APP_MEMORY_ROOT='+str(work/'memory'),'ICU_BRAIN=ollama','ICU_OLLAMA_MODEL=gpt-6-astra-via-local-relay'])
 for label in ['Give this task the stored memories for its app','Let memory learn from this task']:
  n=s.find(s.named(label,'AXCheckBox'))
  if n and n.get('value')=='1':s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':label}))
 data=work/'fixture';data.mkdir();s.launch([str(fixture),str(data)],'fixture');wait_for(lambda:(data/'state.json').exists(),10,'fixture missing');s.pick_window('Paired task fixture')
 goal='Target the Records scrolling area explicitly and scroll it down in page units until Last record is visible, then click Last record. Leave the Draft text and Alpha document unchanged.'
 s.enter_goal(goal);s.press('Send');wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'task did not start');wait_for(lambda:s.find(s.named('Stop','AXButton')) is None,240,'task did not finish')
 report['oracle']=json.loads((data/'state.json').read_text());report['calls']=calls
 actions=[]
 for call in calls:
  try:
   text=call['response']['message']['content'];decision=json.loads(text[text.index('{'):text.rfind('}')+1]);actions.extend(decision.get('actions',[]) or [decision.get('action',{})])
  except Exception:pass
 report['targeted_page_actions']=[a for a in actions if a.get('kind')=='scroll' and a.get('by')=='page' and (a.get('target') or a.get('element_index',-1)>=0)]
 o=report['oracle'];report['passed']=bool(report['targeted_page_actions']) and o['scrolled'] and o['last_record_clicked'] and o['draft']=='seed' and o['Alpha']=='alpha'
 s.screenshot('after.png')
except Exception as e:report['error']=str(e)
finally:
 if s:s.close()
 server.shutdown();(work/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='calls'}))
raise SystemExit(0 if report['passed'] else 1)
