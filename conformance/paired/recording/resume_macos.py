#!/usr/bin/env python3
"""Actual-model task segmentation and full action journaling in the packaged UI."""
import http.server,json,os,subprocess,sys,threading,time,urllib.request
from pathlib import Path
root=Path(sys.argv[1]);work=Path(sys.argv[2]);work.mkdir(parents=True,exist_ok=False)
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable host required')
sys.path.insert(0,str(root/'tests'));from macos_probe import Session,wait_for
calls=[];relay=os.environ['ICU_TEST_MODEL_URL'];report={'passed':False,'model':'gpt-6-astra','segments':[]};s=None
class Relay(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=self.rfile.read(int(self.headers['Content-Length']));row={'request':json.loads(body)};calls.append(row)
  try:
   with urllib.request.urlopen(urllib.request.Request(relay+self.path,body,{'content-type':'application/json'}),timeout=180) as r:status=r.status;data=r.read()
  except Exception:status=502;data=b'{"error":"model relay failed"}'
  row.update(status=status,response=json.loads(data));self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Relay);threading.Thread(target=server.serve_forever,daemon=True).start()
memory=work/'memory';memory.mkdir();(memory/'settings.json').write_text(json.dumps({'chatMemories':True,'skysight':False,'paused':True,'retentionDays':7,'idleMinutes':1,'provider':''}))
def budget(value):
 window=wait_for(lambda:s.find(s.named('Copilot','AXWindow')),8,'app window missing')['frame'];s.ui('click',window['x']+window['w']/2,window['y']+12)
 get=lambda:s.find(s.named('Decision budget','AXPopUpButton'))
 current=wait_for(get,8,'budget select missing').get('value');choices=['18','36','72'];difference=choices.index(str(value))-choices.index(str(current))
 if difference:
  s.ui('press',s.app_pid,json.dumps({'role':'AXPopUpButton','name':'Decision budget'}))
  for _ in range(abs(difference)):s.ui('key',125 if difference>0 else 126)
  s.ui('key',36)
 wait_for(lambda:get().get('value')==str(value),5,'decision budget did not change')

try:
 fixture=root/'ResumeLab.app/Contents/MacOS/ResumeLab';s=Session(work,root/'Copilot.app',fixture,root/'macos_ui',report)
 s.start_app(['ICU_APP_MEMORY_ROOT='+str(memory),'ICU_BRAIN=ollama','ICU_OLLAMA_MODEL=gpt-6-astra-via-local-relay'])
 label='Give this task the stored memories for its app';n=s.find(s.named(label,'AXCheckBox'))
 if n and n.get('value')=='1':s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':label}))
 n=s.find(s.named('Let memory learn from this task','AXCheckBox'))
 if n and n.get('value')!='1':s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':'Let memory learn from this task'}))
 s.launch([str(fixture),str(work/'state.json')],'fixture');wait_for(lambda:(work/'state.json').exists(),10,'fixture missing');s.pick_window('Resume Lab')
 budget(18)
 goal='Complete all 81 stages in Resume Lab with no wrong selections. Read the current stage instruction and choose its named control. Make only one selection per decision, then inspect the updated stage before deciding again.'
 s.enter_goal(goal);s.press('Send');wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'first segment did not start');wait_for(lambda:s.find(s.named('Stop','AXButton')) is None,300,'first segment did not stop')
 first=json.loads((work/'state.json').read_text());checkpoints=list((memory/'tasks').glob('*.json'));assert len(checkpoints)==1,'saved task missing';checkpoint=json.loads(checkpoints[0].read_text());report['segments'].append({'oracle':first,'checkpoint':checkpoint})
 assert first['stage']==18 and first['errors']==0 and checkpoint['decisions']==18,'first segment did not stop at its decision budget'
 assert 'element_token' not in json.dumps(checkpoint),'checkpoint retained executable targets'
 s.screenshot('saved-progress.png')
 before=len(calls);s.press('Saved tasks');s.press('Load saved task')
 budget(72)
 s.press('Send');wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'resume did not start');wait_for(lambda:s.find(s.named('Stop','AXButton')) is None,600,'resume did not end')
 final=json.loads((work/'state.json').read_text());report['segments'].append({'oracle':final})
 report['resumed_prompt']=calls[before]['request'] if len(calls)>before else None
 # Paused generation leaves the locally redacted journal available for inspection.
 def journals():
  files=list((memory/'runs').rglob('*.json'));return [json.loads(p.read_text()) for p in files if p.name!='batch.json']
 def complete_journals():
  rs=journals();return rs if sum(len(r['steps']) for r in rs)==81 else None
 runs=wait_for(complete_journals,30,'full action journal missing')
 report['journals']=runs;report['calls']=calls
 report['passed']=final['completed'] and final['errors']==0 and len(final['events'])==81 and not list((memory/'tasks').glob('*.json')) and sorted(len(r['steps']) for r in runs)==[18,23,40] and 'Earlier segment' in json.dumps(report['resumed_prompt'])
 s.screenshot('completed.png')
except Exception as e:report['error']=str(e)
finally:
 if s:s.close()
 server.shutdown();(work/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k not in ['calls','journals','segments','resumed_prompt']}))
raise SystemExit(0 if report['passed'] else 1)
