#!/usr/bin/env python3
"""Native semantic capture acceptance. Only runs on the owned disposable desktop."""
import argparse,json,os,subprocess,time,hashlib,signal
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--observer',default='observer');p.add_argument('--fault',action='store_true');p.add_argument('--count',type=int,default=0);p.add_argument('--burst',action='store_true');a=p.parse_args()
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('disposable host required')
a.out.mkdir(parents=True,exist_ok=False);root=a.root;logs=(a.out/'console.log').open('w');fixture=None;rec=None
report={'actor':'synthetic_fixture','observer_sha256':hashlib.sha256((root/a.observer).read_bytes()).hexdigest(),'fault_injection':a.fault,'passed':False}
try:
 fixture=subprocess.Popen([str(root/'PointerFixture'),str(a.out/'native.json')],stdout=logs,stderr=logs);time.sleep(1)
 tree=json.loads(subprocess.check_output([str(root/'macos_ui'),'tree',str(fixture.pid)],text=True))
 (a.out/'tree.json').write_text(json.dumps(tree,indent=2))
 pad=next(n for n in tree['nodes'] if n.get('name')=='Pointer pad');f=pad['frame'];x=f['x']+100;y=f['y']+120
 env=os.environ.copy()
 if a.fault:env['ICU_TEST_TAP_DISABLE_AFTER_MS']='4000'
 rec=subprocess.Popen([str(root/a.observer),'record','--out',str(a.out),'--session','pointer','--secs','18' if a.count else '12','--screen','false','--input','false','--net','false','--devices','false','--audio','false','--ui','true','--ui-values','false'],env=env,stdout=logs,stderr=logs);time.sleep(3)
 actions=[['click',x,y,0],['click',x+40,y,1],['click',x+80,y,2],['drag',x,y,0],['scroll',x,y,3,0],['scroll',x,y,0,-2]]
 if a.count:
  actions=[]
  subprocess.run([str(root/'pointer-input'),'burst' if a.burst else 'long',str(x),str(y),'0',str(a.count)],check=True)
 for args in actions:subprocess.run([str(root/'pointer-input'),*map(str,args)],check=True);time.sleep(.45)
 # A later click must survive native tap disable/re-enable in the fault build.
 if not a.count:subprocess.run([str(root/'pointer-input'),'click',str(x),str(y), '0'],check=True);time.sleep(.5)
 rec.wait(25)
 events=[json.loads(s) for s in (a.out/'recording-pointer/events.jsonl').read_text().splitlines() if s]
 captured=[e for e in events if e['kind'] in ['ui.click','ui.drag','ui.scroll'] and e['payload'].get('pid')==fixture.pid]
 report['captured']=[{'kind':e['kind'],'button':e['payload'].get('button'),'dx':e['payload'].get('dx'),'dy':e['payload'].get('dy'),'label':e['payload'].get('label')} for e in captured]
 report['native']=json.loads((a.out/'native.json').read_text());report['gaps']=[e for e in events if e['kind'].endswith('_gap')]
 report['recovery']=[e for e in events if e['kind']=='observer.ui_pointer_recovered'];report['faults']=[e for e in events if e['kind']=='observer.test_fault']
 report['raw_input_events']=sum(e['kind'].startswith('input.') for e in events);report['values']=sum('value' in e['payload'] for e in events if e['kind'].startswith('ui.'))
 kinds=[e['kind'] for e in captured];buttons=[e['payload'].get('button') for e in captured if e['kind']=='ui.click']
 report['passed']=kinds==['ui.click','ui.click','ui.click','ui.click','ui.drag','ui.scroll','ui.scroll','ui.click'] and buttons==['left','right','middle','left','left'] and sum(e['kind']=='down' for e in report['native'])==5 and sum(e['kind']=='scroll' for e in report['native'])==2 and not report['raw_input_events'] and not report['values']
 if a.count:
  native_down=sum(e['kind']=='down' for e in report['native']);lost=sum(e['payload'].get('count',0) for e in report['gaps'])
  report.update(requested_clicks=a.count,native_down=native_down,known_lost=lost)
  report['passed']=native_down==a.count and len(captured)+lost==a.count and not report['raw_input_events'] and not report['values'] and (a.burst or not lost)
 if a.fault:report['passed'] &= bool(report['recovery']) and bool(report['faults'])
 elif not a.burst:report['passed'] &= not report['gaps']
except Exception as e:report['error']=str(e)
finally:
 for proc in [rec,fixture]:
  if proc is not None and proc.poll() is None:proc.terminate();proc.wait(10)
 logs.close();(a.out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report));
raise SystemExit(0 if report['passed'] else 1)
