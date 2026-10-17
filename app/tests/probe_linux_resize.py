"""Verify that capturing a sibling cannot change a window's pixel coordinates."""
import hashlib
import json
import os
import pathlib
import select
import signal
import socket
import subprocess
import sys
import threading
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
exe = pathlib.Path(sys.argv[2]).resolve()
fixture = pathlib.Path(sys.argv[3]).resolve()
mode = sys.argv[4] if len(sys.argv) > 4 else 'resize'
if mode not in ('resize', 'same-window'): raise SystemExit('expected resize or same-window')
processes = []; logs = []; samples = []; drag_result = {}; drag_thread = None
monitor_stop = threading.Event(); monitor = None; driver_identity = None
report = {'source_commit':os.environ.get('ICU_TEST_APP_REVISION','unknown'),'mode':mode,
    'probe_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
env = {k:v for k,v in os.environ.items() if not k.startswith('ICU_')}
env.update({'XDG_CACHE_HOME':str(work/'cache'),'XDG_DATA_HOME':str(work/'data'),
    'XDG_CONFIG_HOME':str(work/'config'),'XDG_RUNTIME_DIR':str(work/'runtime'),
    'XDG_SESSION_TYPE':'x11','GTK_MODULES':'gail:atk-bridge','NO_AT_BRIDGE':'0',
    'LIBGL_ALWAYS_SOFTWARE':'1','ICU_BRAIN':'ollama'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)

def identity(pid):
    try: return pathlib.Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
    except FileNotFoundError: return None

def launch(args, name, stdout=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args,env=env,stdin=subprocess.DEVNULL,
        stdout=stdout if stdout is not None else log,stderr=log,start_new_session=True)
    processes.append(child); return child

def wait_for(predicate, seconds, message):
    deadline = time.monotonic()+seconds
    while time.monotonic()<deadline:
        result = predicate()
        if result: return result
        time.sleep(.02)
    raise RuntimeError(message)

def protocol(path, request, timeout=4):
    deadline = time.monotonic()+timeout
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(timeout); connection.connect(str(path))
        connection.sendall((json.dumps(request)+'\n').encode()); data=b''
        while b'\n' not in data:
            remaining=deadline-time.monotonic()
            if remaining<=0: raise TimeoutError('protocol deadline')
            connection.settimeout(remaining); chunk=connection.recv(65536)
            if not chunk: raise ConnectionError('peer closed')
            data+=chunk
            if len(data)>4*1024*1024: raise ValueError('oversized probe response')
    response=json.loads(data.split(b'\n')[0]); assert response.get('ok'),response
    return response['result']

def events():
    path=work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]

def deadline_signal(*_): raise TimeoutError('native shutdown probe deadline')
signal.signal(signal.SIGTERM,deadline_signal); signal.signal(signal.SIGALRM,deadline_signal); signal.alarm(75)
try:
    report['host_network_namespace']=os.environ['ICU_PROBE_HOST_NETNS']
    report['test_network_namespace']=os.readlink('/proc/self/ns/net')
    assert report['host_network_namespace']!=report['test_network_namespace'],'private network namespace required'
    xvfb=launch(['Xvfb','-displayfd','1','-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',subprocess.PIPE)
    assert select.select([xvfb.stdout],[],[],8)[0],'Xvfb startup timeout'
    number=xvfb.stdout.readline().decode().strip(); assert number.isdigit()
    env['DISPLAY']=':'+number; report['display']=env['DISPLAY']
    subprocess.run(['dbus-update-activation-environment','DISPLAY','XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME'],env=env,check=True,timeout=5)
    launch(['openbox'],'openbox'); launch(['picom','--backend','xrender'],'picom'); time.sleep(.5)
    receiver=launch(['/usr/bin/python3',str(fixture),str(work),'resize'],'receiver')
    def receiver_ready():
        if receiver.poll() is not None: raise RuntimeError('receiver exited; see receiver.log')
        return (work/'fixture-ready.json').exists()
    wait_for(receiver_ready,5,'receiver not ready')
    ready=json.loads((work/'fixture-ready.json').read_text()); assert ready['pid']==receiver.pid
    app=launch([str(exe)],'app'); report.update(app_pid=app.pid,receiver=ready)
    sock=work/'cache/icu'/f'driver-{app.pid}.sock'
    def metadata():
        if app.poll() is not None: raise RuntimeError('app exited during startup')
        try: return protocol(sock,{'method':'metadata'},1)
        except (OSError,ValueError): return None
    meta=wait_for(metadata,30,'owned driver not ready'); report['metadata']=meta
    assert pathlib.Path(f'/proc/{meta["pid"]}/exe').resolve()==exe.with_name('cua-driver'),'wrong driver binary'
    driver_identity=(meta['pid'],identity(meta['pid']))
    assert driver_identity[1] is not None
    def call(name,args,timeout=4):
        result=protocol(sock,{'method':'call','name':name,'args':args,'session_id':'resize-probe','client_kind':'cli'},timeout)
        assert result.get('isError') is not True,result
        return result.get('structuredContent',result)
    windows=call('list_windows',{})['windows']
    apps=[w for w in windows if w.get('pid')==app.pid and w.get('title')=='Copilot']
    assert len(apps)==1,'app window is ambiguous'
    assert any(w.get('pid')==receiver.pid and w.get('window_id')==ready['target'] for w in windows),'receiver identity missing'
    call('set_agent_cursor_enabled',{'session':'resize-probe','enabled':False})
    os.environ.update(env)
    from Xlib import display, X
    connection=display.Display(env['DISPLAY']); root=connection.screen().root
    atom=connection.intern_atom('_NET_ACTIVE_WINDOW')
    subprocess.run(['xdotool','windowactivate','--sync',str(ready['guard']),'mousemove','25','25'],env=env,check=True,timeout=4)
    def sample(connection,root):
        prop=root.get_full_property(atom,X.AnyPropertyType); pointer=root.query_pointer(); focus=connection.get_input_focus().focus
        return {'active':int(prop.value[0]) if prop is not None else 0,
            'focus':getattr(focus,'id',focus),'pointer':[pointer.root_x,pointer.root_y]}
    wait_for(lambda:sample(connection,root)['active']==ready['guard'],3,'guard not active')
    baseline=sample(connection,root); report['baseline']=baseline
    def monitor_ownership():
        observer=display.Display(env['DISPLAY']); observed_root=observer.screen().root
        try:
            while not monitor_stop.is_set():
                samples.append(sample(observer,observed_root)); monitor_stop.wait(.02)
        finally: observer.close()
    monitor=threading.Thread(target=monitor_ownership,daemon=True); monitor.start()
    def capture(wid, max_dimension=640, session='resize-probe'):
        import base64, struct
        response=protocol(sock,{'method':'call','name':'get_window_state',
            'args':{'pid':receiver.pid,'window_id':wid,'include_screenshot':True,'include_accessibility_tree':False,'max_dimension':max_dimension},
            'session_id':session,'client_kind':'cli'},8)
        assert not response.get('isError'),response
        image=next(c for c in response['content'] if c.get('type')=='image')
        png=base64.b64decode(image['data']); assert png[:8]==b'\x89PNG\r\n\x1a\n'
        (work/f'capture-{wid}-{max_dimension}.png').write_bytes(png)
        return list(struct.unpack('!II',png[16:24]))
    report['target_capture']=capture(ready['target'])
    assert report['target_capture']==[640,400],'large fixture was not downscaled by two'
    point={'session':'resize-probe','pid':receiver.pid,'window_id':ready['target'],'x':160,'y':100,'delivery_mode':'background'}
    call('click',point,8)
    wait_for(lambda:len(events())>=2,3,'initial click did not reach receiver')
    first=events(); report['first_events']=first
    assert [(e['kind'],e['x'],e['y']) for e in first]==[('down',320,200),('up',320,200)],'initial downscaled click missed its point'
    if mode == 'same-window':
        # another client (the app's agent) captures at its own resolution; an
        # unbound pixel reads only the calling session's capture (c9-unify)
        report['competing_capture']=capture(ready['target'],1200,'competing-client')
        assert report['competing_capture']==[1200,750],'competing capture did not use the app agent resolution'
    else:
        report['sibling_capture']=capture(ready['guard'])
        assert report['sibling_capture']==[360,240],'small sibling was unexpectedly resized'
    call('click',point,8)
    wait_for(lambda:len(events())>=4,3,'second click did not reach receiver')
    second=events()[2:]; report['second_events']=second
    assert [(e['kind'],e['x'],e['y']) for e in second]==[('down',320,200),('up',320,200)],'competing capture changed the target coordinate scale'
    assert all(e['active']==ready['guard'] for e in events()),'receiver took foreground ownership'
    monitor_stop.set(); monitor.join(2)
    assert samples and all(s==baseline for s in samples),'capture/input disturbed focus or pointer'
    report.update(ok=True)
except Exception as error:
    report.update(ok=False,error=str(error),error_type=type(error).__name__,traceback=traceback.format_exc())
finally:
    signal.alarm(0); monitor_stop.set()
    if monitor: monitor.join(2)
    report['samples']=samples; report['events']=events(); report['drag_result']=drag_result
    for child in reversed(processes):
        if child.poll() is None:
            try: os.killpg(child.pid,signal.SIGTERM)
            except ProcessLookupError: pass
    if driver_identity and identity(driver_identity[0])==driver_identity[1]:
        try: os.kill(driver_identity[0],signal.SIGKILL)
        except ProcessLookupError: pass
    for child in reversed(processes):
        try: child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid,signal.SIGKILL); child.wait()
    if drag_thread: drag_thread.join(2)
    for log in logs: log.close()
    report['owned_children_reaped']=all(child.poll() is not None for child in processes)
    (work/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
    sys.exit(0 if report.get('ok') else 1)
