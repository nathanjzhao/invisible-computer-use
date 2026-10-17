#!/usr/bin/env python3
"""Record synthetic native demonstrations, never expected held-out outcomes."""
import argparse,json,os,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable desktop required')
a.out.mkdir(parents=True,exist_ok=False)
families=['invoice-routing','expense-review','inventory-transfer','appointment-booking','support-triage','document-export','contact-import','report-filtering']
choices=[['Review','Prepare','Validate'],['Assign','Route','Categorize'],['Finish','Archive','Approve']]
routes=[[0,1,2],[2,0,1],[1,2,0],[2,1,0],[0,2,1],[1,0,2],[2,2,0],[0,0,1]]
results=[]
def ui(*args):return json.loads(subprocess.check_output([str(a.root/'macos_ui'),*map(str,args)],text=True))
def node(pid,role,name):
 nodes=ui('tree',pid)['nodes'];return next(n for n in nodes if n.get('role')==role and n.get('name')==name)
def click(n):f=n['frame'];ui('click',f['x']+f['w']/2,f['y']+f['h']/2)
for family,route in zip(families,routes):
 work=a.out/family;work.mkdir();log=(work/'console.log').open('w');fx=None;rec=None;row={'family':family,'actor':'synthetic_fixture','passed':False};results.append(row)
 try:
  fx=subprocess.Popen([str(a.root/'MemoryLab.app/Contents/MacOS/MemoryLab'),str(work),family],stdout=log,stderr=log);time.sleep(.8)
  window=node(fx.pid,'AXWindow','Workflow Lab');f=window['frame'];ui('click',f['x']+f['w']/2,f['y']+12);time.sleep(.4)
  rec=subprocess.Popen([str(a.root/'observer'),'record','--out',str(work),'--session','demo','--secs','10','--screen','false','--input','false','--net','false','--devices','false','--audio','false','--ui','true','--ui-values','false'],stdout=log,stderr=log);time.sleep(3)
  click(node(fx.pid,'AXTextField','Reference'));ui('type','demonstration-reference');time.sleep(.4)
  for stage,index in enumerate(route):click(node(fx.pid,'AXButton',choices[stage][index]));time.sleep(.65)
  row['oracle']=json.loads((work/'state.json').read_text());rec.wait(20)
  events=[json.loads(s) for s in (work/'recording-demo/events.jsonl').read_text().splitlines() if s]
  row['semantic_labels']=[e['payload'].get('label') for e in events if e['kind']=='ui.click' and e['payload'].get('pid')==fx.pid]
  row['raw_input_events']=sum(e['kind'].startswith('input.') for e in events);row['values']=sum('value' in e['payload'] for e in events if e['kind'].startswith('ui.'))
  row['passed']=row['oracle']['completed'] and row['oracle']['path']==[choices[i][v] for i,v in enumerate(route)] and all(label in row['semantic_labels'] for label in row['oracle']['path']) and not row['raw_input_events'] and not row['values']
 except Exception as e:row['error']=str(e)
 finally:
  for proc in [rec,fx]:
   if proc is not None and proc.poll() is None:proc.terminate();proc.wait(10)
  log.close();(work/'result.json').write_text(json.dumps(row,indent=2)+'\n');print(json.dumps(row),flush=True)
 if not row['passed']:break
report={'passed':len(results)==8 and all(r['passed'] for r in results),'families':results};(a.out/'result.json').write_text(json.dumps(report,indent=2)+'\n')
raise SystemExit(0 if report['passed'] else 1)
