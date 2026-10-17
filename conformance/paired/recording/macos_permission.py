#!/usr/bin/env python3
"""Missing permission and recovery on an owned disposable Mac, scoped to one test binary."""
import json,os,subprocess,sys,time,plistlib,shutil,hashlib
from pathlib import Path
root=Path(sys.argv[1]);out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=False)
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable host required')
bundle=out/'PermissionObserver.app';observer=bundle/'Contents/MacOS/observer';observer.parent.mkdir(parents=True)
shutil.copy2(root/'observer-permission',observer)
identity='dev.icu.permission-observer'
plistlib.dump({'CFBundleIdentifier':identity,'CFBundleName':'Permission Observer','CFBundleExecutable':'observer','CFBundlePackageType':'APPL','CFBundleVersion':'1','LSUIElement':True},open(bundle/'Contents/Info.plist','wb'))
subprocess.run(['codesign','--force','--sign','-',str(bundle)],check=True)
db='/Library/Application Support/com.apple.TCC/TCC.db';log=(out/'console.log').open('w');rec=None;fx=None;report={'passed':False,'observer_sha256':hashlib.sha256(observer.read_bytes()).hexdigest(),'responsible_bundle':identity}
def sql(statement):subprocess.run(['sudo','sqlite3',db,statement],check=True)
def permission(allowed):
 for service in ['Accessibility','ListenEvent']:
  for client,kind in [(identity,0),(str(observer),1)]:
   sql("INSERT OR REPLACE INTO access(service,client,client_type,auth_value,auth_reason,auth_version,flags,indirect_object_identifier) VALUES('kTCCService"+service+"','"+client+"',"+str(kind)+","+('2' if allowed else '0')+",4,1,0,'UNUSED');")
 subprocess.run(['sudo','killall','-HUP','tccd'],check=False,stdout=log,stderr=log)
def ui(*args):return json.loads(subprocess.check_output([str(root/'macos_ui'),*map(str,args)],text=True))
try:
 permission(False)
 fx=subprocess.Popen([str(root/'PointerFixture'),str(out/'native.json')],stdout=log,stderr=log);time.sleep(1)
 n=next(n for n in ui('tree',fx.pid)['nodes'] if n.get('name')=='Pointer pad');f=n['frame'];x=f['x']+100;y=f['y']+120
 subprocess.run(['open','-g','-n','-a',str(bundle),'--stdout',str(out/'observer.out'),'--stderr',str(out/'observer.err'),'--args','record','--out',str(out),'--session','permission','--secs','20','--screen','false','--input','false','--net','false','--devices','false','--audio','false','--ui','true','--ui-values','false'],check=True)
 for _ in range(100):
  pids=ui('pid-of',identity)['pids']
  if len(pids)==1:rec=pids[0];break
  time.sleep(.1)
 if rec is None:raise RuntimeError('responsible observer bundle did not launch')
 time.sleep(3);subprocess.run([str(root/'pointer-input'),'click',str(x),str(y),'0'],check=True);time.sleep(.5)
 permission(True);time.sleep(3)
 for button in range(3):subprocess.run([str(root/'pointer-input'),'click',str(x),str(y),str(button)],check=True);time.sleep(.5)
 for _ in range(250):
  if rec not in ui('pid-of',identity)['pids']:break
  time.sleep(.1)
 else:raise RuntimeError('observer did not stop')
 events=[json.loads(s) for s in (out/'recording-permission/events.jsonl').read_text().splitlines() if s]
 disabled=[e for e in events if e['kind']=='observer.ui_start' and e['payload'].get('enabled') is False];recovered=[e for e in events if e['kind']=='observer.ui_accessibility_recovered'];clicks=[e for e in events if e['kind']=='ui.click' and e['payload'].get('pid')==fx.pid]
 report.update(disabled=disabled,recovered=recovered,clicks=clicks,native=json.loads((out/'native.json').read_text()),raw_input=sum(e['kind'].startswith('input.') for e in events))
 report['live_recovery']=bool(recovered)
 if disabled and not recovered:
  # macOS may retain a process's denied TCC decision until it restarts. This
  # is a separate, explicit recovery attempt, not a live-regrant pass.
  subprocess.run(['open','-g','-n','-a',str(bundle),'--stdout',str(out/'restart.out'),'--stderr',str(out/'restart.err'),'--args','record','--out',str(out),'--session','restart','--secs','12','--screen','false','--input','false','--net','false','--devices','false','--audio','false','--ui','true','--ui-values','false'],check=True)
  time.sleep(3)
  pids=ui('pid-of',identity)['pids'];rec=pids[0] if len(pids)==1 else None
  if rec is None:raise RuntimeError('explicit recorder restart did not launch')
  for button in range(3):subprocess.run([str(root/'pointer-input'),'click',str(x),str(y),str(button)],check=True);time.sleep(.5)
  for _ in range(200):
   if rec not in ui('pid-of',identity)['pids']:break
   time.sleep(.1)
  else:raise RuntimeError('restarted recorder did not stop')
  resumed=[json.loads(s) for s in (out/'recording-restart/events.jsonl').read_text().splitlines() if s]
  report['restart_clicks']=[e for e in resumed if e['kind']=='ui.click' and e['payload'].get('pid')==fx.pid]
  report['restart_enabled']=any(e['kind']=='observer.ui_start' and e['payload'].get('enabled') is True for e in resumed)
  report['raw_input']+=sum(e['kind'].startswith('input.') for e in resumed)
  report['recovery_requires_restart']=report['restart_enabled']
  clicks=report['restart_clicks']
 report['passed']=bool(disabled) and (bool(recovered) or report.get('restart_enabled',False)) and [e['payload'].get('button') for e in clicks]==['left','right','middle'] and not report['raw_input']
except Exception as e:report['error']=str(e)
finally:
 permission(True)
 if rec is not None and rec in ui('pid-of',identity)['pids']:os.kill(rec,15)
 if fx is not None and fx.poll() is None:fx.terminate();fx.wait(10)
 log.close();(out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
raise SystemExit(0 if report['passed'] else 1)
