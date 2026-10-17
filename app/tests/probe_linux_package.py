"""Probe an extracted package on an isolated Linux X11 display.

Run through a private session bus:
  dbus-run-session -- python3 probe_linux_package.py OUT APP_EXE [PROTOCOL_TEST_EXE]

No input actions are sent. OUT should be a dedicated temporary directory.
The optional test executable runs only metadata and tool discovery.
"""
import os,sys,time,json,subprocess,socket,signal,select,pathlib,hashlib
if sys.platform != 'linux': raise SystemExit('This probe is Linux-only')
work=pathlib.Path(sys.argv[1]);work.mkdir(parents=True,exist_ok=True)
app_exe=pathlib.Path(sys.argv[2]).resolve();driver_exe=app_exe.with_name('cua-driver')
assert app_exe.is_file() and driver_exe.is_file()
processes=[]; logs=[]; report={'app':str(app_exe),'driver':str(driver_exe),'scope':'isolated X11 package startup; no input actions'}
env={k:v for k,v in os.environ.items() if not k.startswith('ICU_')}
env.update({'XDG_CACHE_HOME':str(work/'cache'),'XDG_DATA_HOME':str(work/'data'),'XDG_CONFIG_HOME':str(work/'config'),'XDG_RUNTIME_DIR':str(work/'runtime'),'XDG_SESSION_TYPE':'x11','GTK_MODULES':'gail:atk-bridge','NO_AT_BRIDGE':'0','LIBGL_ALWAYS_SOFTWARE':'1','ICU_BRAIN':'ollama','ICU_OWN_DRIVER':'1'})
(work/'runtime').mkdir(exist_ok=True);(work/'runtime').chmod(0o700)
def launch(args,name,stdout=None):
 log=open(work/(name+'.log'),'wb');logs.append(log)
 p=subprocess.Popen(args,env=env,stdin=subprocess.DEVNULL,stdout=stdout or log,stderr=log,start_new_session=True)
 processes.append(p);return p
try:
 xvfb=launch(['Xvfb','-displayfd','1','-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',subprocess.PIPE)
 assert select.select([xvfb.stdout],[],[],8)[0], 'Xvfb did not return a private display'
 display_number=xvfb.stdout.readline().decode().strip();assert display_number.isdigit()
 env['DISPLAY']=':'+display_number;report['display']=env['DISPLAY']
 launch(['openbox'],'openbox');launch(['picom','--backend','xrender'],'picom')
 time.sleep(1)
 app=launch([str(app_exe)],'app');report['app_pid']=app.pid
 sock=work/'cache/icu'/('driver-'+str(app.pid)+'.sock')
 deadline=time.monotonic()+35
 meta=None
 while time.monotonic()<deadline:
  assert app.poll() is None,'packaged app exited during startup'
  try:
   with socket.socket(socket.AF_UNIX) as connection:
    connection.settimeout(1);connection.connect(str(sock));connection.sendall(b'{"method":"metadata"}\n')
    data=b''
    while b'\n' not in data:
     if time.monotonic() >= deadline: raise TimeoutError('metadata deadline')
     chunk=connection.recv(65536)
     if not chunk: raise ConnectionError('metadata peer closed')
     data+=chunk
     if len(data)>65536: raise ValueError('metadata frame too large')
    envelope=json.loads(data.split(b'\n')[0]);meta=envelope['result'];break
  except (OSError,KeyError,json.JSONDecodeError):time.sleep(.2)
 assert meta is not None,'app-owned driver never became ready'
 report['metadata']=meta
 actual=pathlib.Path('/proc')/str(meta['pid'])/'exe'
 report['actual_driver_exe']=os.readlink(actual)
 assert actual.resolve()==driver_exe,'app connected to a driver outside the extracted package'
 assert meta['embedded'] is True
 report['driver_sha256']=hashlib.sha256(driver_exe.read_bytes()).hexdigest()
 if len(sys.argv)>3:
  test_env=env.copy();test_env['ICU_TEST_PROTOCOL_SOCKET']=str(sock)
  checked=subprocess.run([sys.argv[3],'transport::compatibility_tests::native_daemon_handshake_and_discovery','--exact','--ignored','--nocapture'],env=test_env,capture_output=True,text=True,timeout=20)
  report['native_protocol_test']={'exit_code':checked.returncode,'stdout':checked.stdout,'stderr':checked.stderr}
  assert checked.returncode==0,'current app transport rejected native metadata or discovery'

 # Inspect accessibility and capture only this private X server. The process
 # cannot see or inject into Nathan's Mac desktop.
 script='''import gi,json,time,sys
from pathlib import Path
gi.require_version("Gtk","3.0")
gi.require_version("Gdk","3.0")
from gi.repository import Gdk
import pyatspi
end=time.monotonic()+12
apps=[];names=[]
while time.monotonic()<end:
 desktop=pyatspi.Registry.getDesktop(0)
 apps=[{"name":a.name,"pid":a.get_process_id()} for a in desktop]
 names=[]
 targets=[a for a in desktop if a.get_process_id()==int(sys.argv[2])]
 queue=[(a,0) for a in targets];count=0
 while queue and count<1000:
  item,depth=queue.pop();count+=1
  try:
   if item.name:names.append(item.name)
   if depth<15:queue.extend((item.getChildAtIndex(i),depth+1) for i in range(item.childCount))
  except Exception:pass
 if "Send" in names:break
 time.sleep(.2)
time.sleep(2)
root=Gdk.get_default_root_window()
pix=Gdk.pixbuf_get_from_window(root,0,0,root.get_width(),root.get_height())
pix.savev(sys.argv[1],"png",[],[])
print(json.dumps({"applications":apps,"names":names}))
'''
 result=subprocess.run(['/usr/bin/python3','-c',script,str(work/'desktop.png'),str(app.pid)],env=env,capture_output=True,text=True,timeout=20)
 report['accessibility_probe_exit']=result.returncode
 report['accessibility_probe_stdout']=result.stdout
 report['accessibility_probe_stderr']=result.stderr
 assert result.returncode==0,'accessibility/screenshot probe failed'
 observed=json.loads(result.stdout)
 assert any(a['pid']==app.pid for a in observed['applications']), 'packaged app absent from accessibility discovery'
 assert 'Send' in observed['names'], 'rendered app controls not exposed through accessibility'
 report['ok']=True
except Exception as error:
 report['ok']=False;report['error']=str(error)
finally:
 for p in reversed(processes):
  if p.poll() is None:
   try:os.killpg(p.pid,signal.SIGTERM)
   except ProcessLookupError:pass
 for p in reversed(processes):
  try:p.wait(timeout=4)
  except subprocess.TimeoutExpired:
   try:os.killpg(p.pid,signal.SIGKILL)
   except ProcessLookupError:pass
   p.wait()
 for f in logs:f.close()
 report['owned_processes_reaped']=all(p.poll() is not None for p in processes)
 (work/'result.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps(report,indent=2))
 sys.exit(0 if report.get('ok') else 1)
