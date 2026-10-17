#!/usr/bin/env python3
"""Compare stock and patched observer clicks against native fixture button events."""
import argparse,hashlib,json,os,subprocess,sys,time
from pathlib import Path


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--work',type=Path,required=True);a=p.parse_args()
 if sys.platform!='darwin' or os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable Mac required')
 a.work.mkdir(parents=True,exist_ok=False);root=a.root;ui=root/'macos_ui';fixture=root/'PairFixture.app/Contents/MacOS/PairFixture'
 report={'actor':'synthetic_fixture','fixture_sha256':sha(fixture),'harness_sha256':sha(Path(__file__)),'runs':[],'passed':False}
 def call(*args):return json.loads(subprocess.check_output([str(ui),*map(str,args)],text=True))
 for name,observer,hold,interval in [('stock-fast',root/'observer-stock',12,0.4),('patched-fast-1',root/'observer-patched',12,0.4),('patched-fast-2',root/'observer-patched',12,0.4),('patched-held',root/'observer-patched',200,0.4)]:
  work=a.work/name;work.mkdir();fx=None;record=None;log=(work/'console.log').open('w')
  row={'name':name,'observer_sha256':sha(observer),'hold_ms':hold,'between_clicks_seconds':interval,'passed':False};report['runs'].append(row)
  try:
   fx=subprocess.Popen([str(fixture),str(work)],stdout=log,stderr=log)
   end=time.monotonic()+10
   while not (work/'state.json').exists():
    if time.monotonic()>end:raise TimeoutError('fixture not ready')
    time.sleep(.1)
   nodes=call('tree',fx.pid)['nodes'];window=next(n for n in nodes if n.get('role')=='AXWindow' and n.get('name')=='Paired task fixture');f=window['frame'];call('click',f['x']+f['w']/2,f['y']+12);time.sleep(.4)
   record=subprocess.Popen([str(observer),'record','--out',str(work/'recordings'),'--session',name,'--secs','12','--screen','false','--input','false','--net','false','--devices','false','--audio','false','--ui','true','--ui-values','false'],stdout=log,stderr=log)
   time.sleep(3)
   for label in ['6','×','7','=']:
    nodes=call('tree',fx.pid)['nodes'];n=next(n for n in nodes if n.get('role')=='AXButton' and n.get('name')==label);f=n['frame']
    subprocess.run([str(root/'fast-click'),str(f['x']+f['w']/2),str(f['y']+f['h']/2),str(hold)],check=True);time.sleep(interval)
   record.wait(20);row['observer_exit']=record.returncode
   state=json.loads((work/'state.json').read_text());row['outcome']=state
   events=[json.loads(line) for line in (work/'recordings'/('recording-'+name)/'events.jsonl').read_text().splitlines() if line]
   clicks=[e for e in events if e['kind']=='ui.click' and e['payload'].get('pid')==fx.pid]
   row['captured_labels']=[e['payload']['label'] for e in clicks];row['sources']=[e['payload'] for e in events if e['kind']=='observer.ui_click_source'];row['gaps']=[e['payload'] for e in events if e['kind']=='ui.click_gap']
   row['raw_input_events']=sum(e['kind'].startswith('input.') for e in events)
   row['value_fields']=sum('value' in e['payload'] for e in events if e['kind'].startswith('ui.'))
   row['passed']=state['display']=='42' and state['buttons']==['6','×','7','='] and row['captured_labels']==state['buttons'] and not row['raw_input_events'] and not row['value_fields'] and record.returncode==0
   if name.startswith('patched'):row['passed']=row['passed'] and any(s['source']=='listen_only_left_button_tap' for s in row['sources']) and not row['gaps']
  except Exception as e:row['error']=str(e)
  finally:
   for proc in [record,fx]:
    if proc is not None and proc.poll() is None:proc.terminate();proc.wait(10)
   row['cleanup_verified']=all(p is None or p.poll() is not None for p in [record,fx]);log.close()
   (work/'result.json').write_text(json.dumps(row,indent=2)+'\n');print(json.dumps({k:v for k,v in row.items() if k!='outcome'}),flush=True)
 report['passed']=all(r['passed'] and r['cleanup_verified'] for r in report['runs'] if r['name'].startswith('patched')) and len(report['runs'])==4
 (a.work/'result.json').write_text(json.dumps(report,indent=2)+'\n');return 0 if report['passed'] else 1
if __name__=='__main__':raise SystemExit(main())
