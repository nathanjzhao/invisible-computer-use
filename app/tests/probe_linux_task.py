"""Native GUI-to-driver task on an owned X server and private network namespace."""
import os,sys,time,json,re,subprocess,socket,signal,select,pathlib,threading,http.server,traceback,hashlib
if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work=pathlib.Path(sys.argv[1]);work.mkdir(parents=True,exist_ok=True)
exe=pathlib.Path(sys.argv[2]).resolve();fixture=pathlib.Path(sys.argv[3]).resolve()
processes=[];logs=[];report={'source_commit':os.environ.get('ICU_TEST_APP_REVISION','unknown'),'scope':'packaged native GUI, deterministic model stub, owned GTK fixture, isolated X11/network'}
mode=os.environ.get('ICU_TEST_MODE','complete')
check_references=os.environ.get('ICU_TEST_REFERENCES')=='1'
check_no_overlay=os.environ.get('ICU_TEST_NO_OVERLAY')=='1'
check_repaint=os.environ.get('ICU_TEST_REPAINT')=='1'
check_key_refusal=os.environ.get('ICU_TEST_KEY_REFUSAL')=='1'
clear_draft=os.environ.get('ICU_TEST_DRAFT_CLEAR')=='1'
expected_draft='' if clear_draft else 'replacement'
draft_summary='Observed empty value' if clear_draft else 'Observed replacement'
if mode not in ('complete','stop_model','tool_error','key_chord','draft_value'):raise SystemExit('unsupported native task mode')
report.update(mode=mode,check_references=check_references,check_no_overlay=check_no_overlay,check_repaint=check_repaint,check_key_refusal=check_key_refusal,clear_draft=clear_draft,probe_sha256=hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest())
env={k:v for k,v in os.environ.items() if not k.startswith('ICU_')}
env.update({'XDG_CACHE_HOME':str(work/'cache'),'XDG_DATA_HOME':str(work/'data'),'XDG_CONFIG_HOME':str(work/'config'),'XDG_RUNTIME_DIR':str(work/'runtime'),'XDG_SESSION_TYPE':'x11','GTK_MODULES':'gail:atk-bridge','NO_AT_BRIDGE':'0','LIBGL_ALWAYS_SOFTWARE':'1','ICU_BRAIN':'ollama'})
if mode in ('key_chord','draft_value'):env['ICU_FIXTURE_KEYBOARD']='1'
if mode=='draft_value':env['ICU_FIXTURE_PRIVATE_CONTROL']='1'
(work/'runtime').mkdir(exist_ok=True);(work/'runtime').chmod(0o700)
first_request=threading.Event();release_model=threading.Event();model_requests=[];model_outcome={"fresh42":False,"terminal":False};monitor_stop=threading.Event();samples=[];monitor=None;monitor_errors=[]
class Model(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  model_requests.append(body);(work/f'model-request-{len(model_requests)}.json').write_text(json.dumps(body,indent=2))
  if len(model_requests)==1:
   first_request.set()
   if not release_model.wait(12): decision={'action':{'kind':'fail','reason':'focus guard was not established'}}
   elif mode=='tool_error': decision={'thought':'Exercise a native tool failure before a second action','actions':[{'kind':'set_value','target':'6','value':'not supported'},{'kind':'click','target':'7'}]}
   elif mode=='key_chord': decision={'thought':'Select all of the draft with a keyboard shortcut','actions':[{'kind':'click','target':'Draft'},{'kind':'key','key':'Ctrl+a'}]}
   elif mode=='draft_value': decision={'thought':'Set the draft through its native editable value','action':{'kind':'set_value','target':'Draft','value':expected_draft}}
   else: decision={'thought':'Compute six times seven using the fixture controls','actions':[{'kind':'click','target':s} for s in ['6','×','7','=']]}
  else:
   text=next(m['content'] for m in body['messages'] if m['role']=='user')
   # A GTK readout is passive text, so the driver does not give it an action
   # index. Accept its exact native label only inside the current observation.
   observation=text.partition('\n\nWINDOW ELEMENT TREE:\n')[2].rpartition('\n\nNext action as JSON:')[0]
   if mode=='draft_value':
    draft=json.loads((work/'draft-state.json').read_text())
    model_outcome['native_draft']=draft
    observed='"Draft" value=""' in observation if clear_draft else 'replacement' in observation
    model_outcome['fresh_value']=observed and draft['text']==expected_draft
    model_outcome['terminal']=True
    decision={'action':{'kind':'done','summary':draft_summary}} if model_outcome['fresh_value'] else {'action':{'kind':'fail','reason':'draft value absent from fresh observation'}}
   elif mode=='key_chord':
    draft=json.loads((work/'draft-state.json').read_text())
    if len(model_requests)==2:
     model_outcome['selection_before_type']=draft
     selected=draft['text']=='seed' and draft['selection']==[0,4]
     model_outcome['selected_all']=selected
     if not selected:model_outcome['terminal']=True
     decision={'action':{'kind':'type','text':'replacement'}} if selected else {'action':{'kind':'fail','reason':'native full selection absent before typing'}}
    else:
     model_outcome['fresh_replacement']='replacement' in observation and draft['text']=='replacement'
     model_outcome['terminal']=True
     decision={'action':{'kind':'done','summary':'Observed replacement'}} if model_outcome['fresh_replacement'] else {'action':{'kind':'fail','reason':'replacement absent from fresh observation'}}
   else:
    model_outcome['fresh42']=bool(re.search(r'^\s*(?:\[\d+\][^\n]* \"42\"|- label = \"42\")\s*$',observation,re.M))
    model_outcome['terminal']=True
    decision={'action':{'kind':'done','summary':'Observed 42'}} if model_outcome['fresh42'] else {'action':{'kind':'fail','reason':'42 absent from fresh observation'}}
  encoded=json.dumps({'message':{'role':'assistant','content':json.dumps(decision,ensure_ascii=False)}}).encode()
  self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(encoded)));self.end_headers();self.wfile.write(encoded)
server=None
driver_identity=None

def process_identity(pid):
 try:return pathlib.Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
 except FileNotFoundError:return None

def launch(args,name,stdout=None):
 f=open(work/(name+'.log'),'wb');logs.append(f)
 p=subprocess.Popen(args,env=env,stdin=subprocess.DEVNULL,stdout=stdout if stdout is not None else f,stderr=f,start_new_session=True)
 processes.append(p);return p

def wait_for(predicate, seconds, message):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  value=predicate()
  if value:return value
  time.sleep(.1)
 raise RuntimeError(message)

def protocol(path, request, timeout=4):
 end=time.monotonic()+timeout
 with socket.socket(socket.AF_UNIX) as conn:
  conn.settimeout(timeout);conn.connect(str(path));conn.sendall((json.dumps(request)+'\n').encode());data=b''
  while b'\n' not in data:
   if time.monotonic()>=end:raise TimeoutError('protocol deadline')
   chunk=conn.recv(65536)
   if not chunk:raise ConnectionError('peer closed')
   data+=chunk
   if len(data)>4*1024*1024:raise ValueError('probe frame too large')
  value=json.loads(data.split(b'\n')[0]);assert value.get('ok'),value
  return value['result']
def deadline_signal(_signum,_frame):raise TimeoutError('overall native probe deadline')
signal.signal(signal.SIGTERM,deadline_signal);signal.signal(signal.SIGALRM,deadline_signal);signal.alarm(120 if check_references else 90)
try:
 # The launcher must create a private network namespace so this standard
 # Ollama endpoint cannot answer unrelated clients on the host.
 host_namespace=os.environ['ICU_PROBE_HOST_NETNS']
 report['host_network_namespace']=host_namespace;report['test_network_namespace']=os.readlink('/proc/self/ns/net')
 assert report['test_network_namespace'] != host_namespace,'private network namespace required'
 server=http.server.ThreadingHTTPServer(('127.0.0.1',11434),Model)
 threading.Thread(target=server.serve_forever,daemon=True).start()
 xvfb=launch(['Xvfb','-displayfd','1','-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',subprocess.PIPE)
 assert select.select([xvfb.stdout],[],[],8)[0],'private display startup timed out'
 number=xvfb.stdout.readline().decode().strip();assert number.isdigit()
 env['DISPLAY']=':'+number;report['display']=env['DISPLAY']
 subprocess.run(['dbus-update-activation-environment','DISPLAY','XDG_RUNTIME_DIR','XDG_DATA_HOME','XDG_CONFIG_HOME'],env=env,check=True,timeout=5)
 launch(['openbox'],'openbox');launch(['picom','--backend','xrender'],'picom');time.sleep(.7)
 fx=launch(['/usr/bin/python3',str(fixture),str(work)],'fixture')
 wait_for(lambda:(work/'fixture-ready.json').exists(),8,'fixture not ready')
 app=launch([str(exe)],'app');report['app_pid']=app.pid;report['fixture_pid']=fx.pid
 sock=work/'cache/icu'/f'driver-{app.pid}.sock'
 def metadata():
  if app.poll() is not None:raise RuntimeError('packaged app exited')
  try:return protocol(sock,{'method':'metadata'},1)
  except (OSError,ValueError,KeyError):return None
 meta=wait_for(metadata,30,'owned driver not ready');report['metadata']=meta
 assert (pathlib.Path('/proc')/str(meta['pid'])/'exe').resolve()==exe.with_name('cua-driver'),'wrong driver binary'
 driver_identity=(meta['pid'],process_identity(meta['pid']))
 assert driver_identity[1] is not None,'owned driver exited before identity capture'
 report['driver_argv']=pathlib.Path(f'/proc/{meta["pid"]}/cmdline').read_bytes().decode().rstrip('\0').split('\0')
 raw=protocol(sock,{'method':'call','name':'list_windows','args':{},'session_id':'icu-test-observer','client_kind':'cli'})
 (work/'native-windows.json').write_text(json.dumps(raw,indent=2))
 os.environ.update(env)
 import pyatspi
 from gi.repository import Atspi
 Atspi.set_timeout(1500,3000)
 from Xlib import display, X
 from Xlib.error import BadWindow
 from Xlib.ext import shape
 connection=display.Display(env['DISPLAY']);root=connection.screen().root
 active_atom=connection.intern_atom('_NET_ACTIVE_WINDOW')
 def active_window():
  prop=root.get_full_property(active_atom,X.AnyPropertyType)
  return int(prop.value[0]) if prop is not None and len(prop.value) else 0
 def nodes():
  apps=[a for a in pyatspi.Registry.getDesktop(0) if a.get_process_id()==app.pid]
  queue=[(a,0) for a in apps];result=[]
  while queue and len(result)<1500:
   item,depth=queue.pop();result.append(item)
   try:
    if depth<18:queue.extend((item.getChildAtIndex(i),depth+1) for i in range(item.childCount))
   except Exception:pass
  return result
 def find_name(text, exact=True):
  for item in nodes():
   try:
    if (item.name==text if exact else text in item.name) and item.getState().contains(pyatspi.STATE_SHOWING):return item
   except Exception:pass
  return None
 def node_text(item):
  try:
   text=item.queryText()
   return text.getText(0,min(text.characterCount,4096))
  except (NotImplementedError,RuntimeError):return ''
 def find_text(text):
  for item in nodes():
   try:
    if item.getState().contains(pyatspi.STATE_SHOWING) and text in node_text(item):return item
   except Exception:pass
  return None
 def press(item):
  for _ in range(10):
   try:
    if item.getRoleName() in ('push button','button','menu item'):
     action=item.queryAction()
     if action.nActions:
      assert action.doAction(0),'accessible action refused'
      return
   except NotImplementedError:pass
   item=item.parent
  raise RuntimeError('no button action in the selected control ancestry')
 press(wait_for(lambda:find_name('+ window'),12,'window picker unavailable'))
 choice=wait_for(lambda:find_name('ICU Calculator',False),10,'fixture missing from window picker')
 press(choice)
 entry=wait_for(lambda:find_name('e.g. in @Calculator',False),8,'command input unavailable')
 report['stage']='enter-goal'
 assert xvfb.poll() is None,'private X server exited'
 structured=raw.get('structuredContent',raw)
 app_windows=[w for w in structured['windows'] if w.get('pid')==app.pid and w.get('title')=='Copilot']
 assert len(app_windows)==1,'packaged app window is ambiguous'
 app_window=app_windows[0]['window_id']
 subprocess.run(['xdotool','windowactivate','--sync',str(app_window)],env=env,check=True,timeout=3)
 ext=entry.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
 assert ext.width>0 and ext.height>0,'command entry has no visible bounds'
 assert active_window()==app_window,'wrong window active before setup input'
 subprocess.run(['xdotool','mousemove','--sync',str(ext.x+ext.width//2),str(ext.y+ext.height//2),'click','1'],env=env,check=True,timeout=3)
 wait_for(lambda:entry.getState().contains(pyatspi.STATE_FOCUSED),3,'command entry did not acquire focus')
 goal='Compute six times seven using the calculator buttons.'
 if mode=='tool_error':goal='Verify that a rejected value change stops the task.'
 if mode=='key_chord':goal='Select all text in Draft with Ctrl+a and replace it with replacement.'
 if mode=='draft_value':goal='Clear Draft and verify its empty value.' if clear_draft else 'Set Draft to replacement and verify its new value.'
 if check_references:
  def replace_command(text):
   # --sync waits for a motion event even when the pointer is already here.
   subprocess.run(['xdotool','mousemove',str(ext.x+ext.width//2),str(ext.y+ext.height//2),'click','1'],env=env,check=True,timeout=3)
   wait_for(lambda:entry.getState().contains(pyatspi.STATE_FOCUSED),3,'command entry did not regain focus')
   subprocess.run(['xdotool','key','--clearmodifiers','ctrl+a'],env=env,check=True,timeout=3)
   subprocess.run(['xdotool','type','--clearmodifiers','--delay','5',text],env=env,check=True,timeout=8)
  def mention_button(title):
   matches=[]
   for item in nodes():
    try:
     if item.getRoleName() in ('push button','button') and title in item.name and item.getState().contains(pyatspi.STATE_SHOWING):matches.append(item)
    except Exception:pass
   return matches[0] if len(matches)==1 else None
  def select_reference(title):
   replace_command('@')
   row=wait_for(lambda:mention_button(title),6,'mention suggestion missing or ambiguous: '+title)
   box=row.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
   assert box.width>0 and box.height>0,'mention row has no visible bounds'
   subprocess.run(['xdotool','mousemove',str(box.x+box.width//2),str(box.y+box.height//2),'click','1'],env=env,check=True,timeout=3)
   alias=wait_for(lambda:node_text(entry).strip() if re.fullmatch(r'@[^\s@]+',node_text(entry).strip()) else None,3,'mention was not inserted')
   return alias
  target_record=next(w for w in structured['windows'] if w.get('pid')==fx.pid and w.get('title')=='ICU Calculator')
  sibling_record=next(w for w in structured['windows'] if w.get('pid')==fx.pid and w.get('title')=='ICU Focus Guard')
  assert target_record['app_name']==sibling_record['app_name'],'fixture does not exercise same-name aliases'
  first=select_reference('ICU Calculator')
  second=select_reference('ICU Focus Guard')
  assert first.lower()!=second.lower(),'same-name windows share a reference alias'
  for draft,message in [('in @UnknownAuditWindow type hello','Select each @window'),(first+' '+second+' '+goal,'Choose one window')]:
   replace_command(draft)
   press(wait_for(lambda:find_name('Send'),3,'Send unavailable for reference validation'))
   wait_for(lambda:find_text(message),4,'reference validation did not reach GUI')
   assert node_text(entry)==draft,'invalid-reference submission lost its draft'
   assert not first_request.is_set(),'invalid reference reached the model'
  assert json.loads((work/'fixture-state.json').read_text())=={'display':'0','buttons':[]},'invalid reference caused input'
  report['references']={'first':first,'second':second,'target_window':target_record['window_id'],'sibling_window':sibling_record['window_id'],'unresolved_rejected':True,'multiple_rejected':True}
  replace_command(first+' '+goal)
 else:
  subprocess.run(['xdotool','type','--clearmodifiers','--delay','5',goal],env=env,check=True,timeout=8)
 report['stage']='submit-goal'
 if mode in ('key_chord','draft_value'):
  report['initial_draft']=json.loads((work/'draft-state.json').read_text())
  assert report['initial_draft']['text']=='seed' and report['initial_draft']['selection']==[],'draft was already selected before the shortcut task'
  assert report['initial_draft']['select_on_focus'] is False,'click/focus must not select the draft on its own'
 fixture_windows=[w for w in structured['windows'] if w.get('pid')==fx.pid and w.get('title')=='ICU Calculator']
 assert len(fixture_windows)==1,'fixture window is ambiguous'
 fixture_window=fixture_windows[0]['window_id']
 before=protocol(sock,{'method':'call','name':'get_window_state','args':{'pid':fx.pid,'window_id':fixture_window,'include_screenshot':False},'session_id':'icu-test-observer','client_kind':'cli'})
 (work/'initial-driver-state.json').write_text(json.dumps(before,indent=2))
 before_state=before.get('structuredContent',before)
 if mode=='draft_value':
  report['initial_value_observed']=any(e.get('label')=='Draft' and e.get('value')=='seed' for e in before_state['elements'])
  assert 'fixture-private-value' not in json.dumps(before),'protected entry value leaked in native observation'
 passive=before_state.get('passive_elements',[])
 assert any(n.get('label')=='0' or n.get('value')=='0' for n in passive),'initial passive readout missing from native structured state'
 assert all('element_index' not in n and 'element_token' not in n for n in passive),'passive text received native input handles'
 assert all('Focus Guard' not in str(n) and 'Keep focus here' not in str(n) for n in passive),'passive text leaked from the sibling window'
 if mode=='tool_error':
  target=next(e for e in before_state['elements'] if e.get('label')=='6' and e.get('element_token'))
  hidden=protocol(sock,{'method':'call','name':'set_agent_cursor_enabled','args':{'session':'icu-test-observer','enabled':False},'session_id':'icu-test-observer','client_kind':'cli'})
  assert not hidden.get('isError'),hidden
  native_error=protocol(sock,{'method':'call','name':'set_value','args':{'session':'icu-test-observer','pid':fx.pid,'window_id':fixture_window,'element_token':target['element_token'],'value':'not supported'},'session_id':'icu-test-observer','client_kind':'cli'})
  (work/'native-tool-error.json').write_text(json.dumps(native_error,indent=2))
  assert native_error.get('isError') is True,'fixture button unexpectedly accepted set_value'
  assert native_error.get('structuredContent',{}).get('code') is None,'oracle did not exercise the missing-code tool error'
  assert json.loads((work/'fixture-state.json').read_text())=={'display':'0','buttons':[]},'error oracle changed receiver state'
  hidden=protocol(sock,{'method':'call','name':'set_agent_cursor_enabled','args':{'session':'icu-test-observer','enabled':False},'session_id':'icu-test-observer','client_kind':'cli'})
  assert not hidden.get('isError'),hidden
  report['native_error_without_code']=True
 press(wait_for(lambda:find_name('Send'),5,'Send unavailable'))
 assert first_request.wait(12),'task did not reach the configured model endpoint'
 guard=int(subprocess.check_output(['xdotool','search','--name','^ICU Focus Guard$'],env=env,text=True,timeout=3).splitlines()[0])
 subprocess.run(['xdotool','windowactivate','--sync',str(guard)],env=env,check=True,timeout=3)
 wait_for(lambda:active_window()==guard,3,'focus guard not active')
 pointer=root.query_pointer();baseline_pointer=[pointer.root_x,pointer.root_y]
 report['guard_window']=guard;report['baseline_pointer']=baseline_pointer
 focus=connection.get_input_focus().focus;baseline_focus=getattr(focus,'id',focus);report['baseline_focus']=baseline_focus
 def sample_ownership():
  monitor_display=display.Display(env['DISPLAY']);monitor_root=monitor_display.screen().root
  try:
   while not monitor_stop.is_set():
    prop=monitor_root.get_full_property(active_atom,X.AnyPropertyType)
    pointer=monitor_root.query_pointer();focus=monitor_display.get_input_focus().focus
    sample={'time':time.monotonic(),'active':int(prop.value[0]) if prop is not None else 0,'focus':getattr(focus,'id',focus),'pointer':[pointer.root_x,pointer.root_y]}
    if check_no_overlay:
     overlays=[]
     for child in monitor_root.query_tree().children:
      try:
       name=child.get_wm_name()
       if isinstance(name,str) and name.startswith('Cua.AgentCursorOverlay.'):
        attrs=child.get_attributes()
        if attrs.map_state==X.IsViewable:
         # The renderer keeps an empty-shaped window mapped while hidden.
         # Mapping alone does not establish that any pixels can be visible.
         rects=child.shape_get_rectangles(shape.SK.Bounding).rectangles
         area=sum(rect.width*rect.height for rect in rects)
         overlays.append({'id':child.id,'name':name,'override_redirect':bool(attrs.override_redirect),'bounding_area':area})
      except BadWindow:pass # A destroyed window cannot remain rendered.
     sample['mapped_native_overlays']=overlays
    samples.append(sample)
    monitor_stop.wait(.02)
  except Exception as error:monitor_errors.append(repr(error))
  finally:monitor_display.close()
 monitor=threading.Thread(target=sample_ownership,daemon=True);monitor.start()
 if mode=='stop_model':
  press(wait_for(lambda:find_name('Stop'),5,'Stop unavailable while the model is pending'))
  report['stopping_before_response']=bool(wait_for(lambda:find_name('Stopping…'),5,'GUI did not acknowledge cancellation'))
  assert not release_model.is_set(),'model response was released before cancellation'
 release_model.set()
 if mode=='tool_error':
  wait_for(lambda:find_name('Send'),10,'failed native task did not reach a terminal GUI state')
  state=json.loads((work/'fixture-state.json').read_text());report['fixture_state']=state
  report['model_requests']=len(model_requests)
  assert state=={'display':'0','buttons':[]},'native tool error was followed by another action'
  assert len(model_requests)==1,'native tool error reached another model decision'
  report['ui_error_summary']=bool(wait_for(lambda:find_text('Action failed. Stopped without replaying'),5,'native tool failure did not reach the GUI'))
 elif mode=='stop_model':
  report['ui_stopped_summary']=bool(wait_for(lambda:find_text('stopped: stopped by request'),8,'cancellation did not reach the GUI'))
  state=json.loads((work/'fixture-state.json').read_text())
  assert state=={'display':'0','buttons':[]},'cancelled model response still caused native input'
  assert len(model_requests)==1,'cancelled run contacted the model again'
 elif mode=='key_chord':
  wait_for(lambda:find_name('Send'),15,'keyboard task did not return to a terminal state')
  state=json.loads((work/'fixture-state.json').read_text())
  report['draft_state']=json.loads((work/'draft-state.json').read_text())
  report['model_outcome']=model_outcome;report['model_requests']=len(model_requests)
  if check_key_refusal:
   assert len(model_requests)==1,'refused keyboard task reached another model decision'
   assert report['draft_state']['text']=='seed' and report['draft_state']['selection']==[],'refused shortcut changed the native draft'
   report['required_keyboard_refusal']=bool(wait_for(lambda:find_text('requested target has no focus-free input backend'),5,'expected native keyboard capability refusal was not surfaced'))
   report['ui_error_summary']=bool(wait_for(lambda:find_text('Action failed. Stopped without replaying'),5,'keyboard refusal did not terminate the task'))
  else:
   assert model_outcome.get('selected_all') is True,'keyboard chord did not select the native draft'
   assert model_outcome.get('fresh_replacement') is True and report['draft_state']['text']=='replacement','keyboard task did not replace and re-observe the native draft'
   assert len(model_requests)==3,'keyboard task did not separate selection, typing and verification'
   report['ui_done_summary']=bool(wait_for(lambda:find_text('done. Observed replacement'),8,'keyboard completion did not reach the GUI'))
 elif mode=='draft_value':
  wait_for(lambda:find_name('Send'),15,'editable-value task did not return to a terminal state')
  state=json.loads((work/'fixture-state.json').read_text())
  report['draft_state']=json.loads((work/'draft-state.json').read_text())
  report['model_outcome']=model_outcome;report['model_requests']=len(model_requests)
  final_observation=protocol(sock,{'method':'call','name':'get_window_state','args':{'pid':fx.pid,'window_id':fixture_window,'include_screenshot':False,'cumulative_diff':True},'session_id':'icu-test-observer','client_kind':'cli'})
  (work/'final-driver-state.json').write_text(json.dumps(final_observation,indent=2))
  report['private_value_excluded']='fixture-private-value' not in json.dumps(final_observation) and 'fixture-private-value' not in json.dumps(model_requests)
  assert report['private_value_excluded'],'protected entry value leaked in native/model observation'
  final_state=final_observation.get('structuredContent',final_observation)
  draft_node=next(e for e in final_state['elements'] if e.get('label')=='Draft')
  initial_node=next(e for e in before_state['elements'] if e.get('label')=='Draft')
  report['native_value_observed']=draft_node.get('value')==expected_draft and 'value' in draft_node
  report['draft_identity_preserved']=draft_node.get('stable_id')==initial_node.get('stable_id') and draft_node.get('stable_id') is not None
  report['native_value_diff']=final_state['diff']
  assert all('value' not in e for e in final_state['elements'] if e.get('role')=='password text'),'protected field fabricated or exposed a value'
  verification=protocol(sock,{'method':'call','name':'verify_state','args':{'pid':fx.pid,'window_id':fixture_window,'expect':[{'element':{'selector':{'role':'text','label_contains':'Draft'},'exists':True,'value_equals':expected_draft}}],'timeout_ms':2000,'stable_samples':2,'include_screenshot':False},'session_id':'icu-test-observer','client_kind':'cli'},6)
  (work/'native-value-verification.json').write_text(json.dumps(verification,indent=2))
  verified=verification.get('structuredContent',verification)
  report['native_value_predicate']=verified.get('status')=='satisfied' and verified.get('stable') is True
  negative=protocol(sock,{'method':'call','name':'verify_state','args':{'pid':fx.pid,'window_id':fixture_window,'expect':[{'element':{'selector':{'role':'text','label_contains':'Draft'},'exists':True,'value_equals':'not-the-draft'}}],'timeout_ms':2000,'stable_samples':2,'include_screenshot':False},'session_id':'icu-test-observer','client_kind':'cli'},6)
  (work/'negative-value-verification.json').write_text(json.dumps(negative,indent=2))
  report['wrong_value_refused']=negative.get('structuredContent',negative).get('status')=='unsatisfied'
  assert report['draft_state']['text']==expected_draft,'native set_value did not update the draft'
  assert len(model_requests)==2,'value task did not verify its first native effect'
  if model_outcome.get('fresh_value'):
   report['ui_done_summary']=bool(wait_for(lambda:find_text('done. '+draft_summary),8,'value completion did not reach the GUI'))
  else:
   report['ui_observation_failure']=bool(wait_for(lambda:find_text('draft value absent from fresh observation'),8,'failed observation did not reach the GUI'))
 else:
  end=time.monotonic()+30;finished=False
  while time.monotonic()<end:
   state=json.loads((work/'fixture-state.json').read_text())
   if state['display']=='42' and model_outcome['terminal']:
    finished=True;break
   time.sleep(.03)
 report['fixture_state']=state;report['model_requests']=len(model_requests);report['model_outcome']=model_outcome
 # Capture the driver's complete observation after the model has finished.
 # Doing this earlier could replace tokens belonging to the app's active run.
 if mode=='complete' and model_outcome['terminal']:
  observation=protocol(sock,{'method':'call','name':'get_window_state','args':{'pid':fx.pid,'window_id':fixture_window,'include_screenshot':False,'cumulative_diff':True},'session_id':'icu-test-observer','client_kind':'cli'})
  (work/'final-driver-state.json').write_text(json.dumps(observation,indent=2))
  observed=observation.get('structuredContent',observation)
  passive=observed.get('passive_elements',[])
  assert any(n.get('label')=='42' or n.get('value')=='42' for n in passive),'fresh passive result missing from native structured state'
  assert all('element_index' not in n and 'element_token' not in n for n in passive),'passive result received native input handles'
  assert all('Focus Guard' not in str(n) and 'Keep focus here' not in str(n) for n in passive),'fresh passive text leaked from the sibling window'
  assert '42' in observed['diff']['text'],'native diff omitted the passive result change'
  report['passive_native_diff_verified']=True
  verification=protocol(sock,{'method':'call','name':'verify_state','args':{'pid':fx.pid,'window_id':fixture_window,'expect':[{'element':{'selector':{'role':'label','label_contains':'42'},'exists':True}}],'timeout_ms':4000,'stable_samples':2,'include_screenshot':False},'session_id':'icu-test-observer','client_kind':'cli'},8)
  (work/'native-verification.json').write_text(json.dumps(verification,indent=2))
  verified=verification.get('structuredContent',verification)
  assert verified.get('status')=='satisfied' and verified.get('stable') is True,'native verifier could not confirm the passive result'
  report['passive_native_predicate_verified']=True
 if mode=='complete':
  assert finished and model_outcome['fresh42'],'packaged task did not reach and re-observe 42'
  assert state['buttons']==['6','×','7','='],'input multiplicity/order mismatch'
  report['ui_done_summary']=bool(wait_for(lambda:find_text('done. Observed 42'),8,'completion summary did not reach the GUI'))
 report['ui_returned_to_send']=bool(wait_for(lambda:find_name('Send'),8,'terminal event did not reset the GUI'))
 if check_repaint:
  import gi
  gi.require_version('Gdk','3.0')
  from gi.repository import Gdk
  terminal_observed=time.monotonic();repaints=[];report['repaint_samples']=repaints
  for delay in [0,.25,.5,1,2,4]:
   time.sleep(max(0,terminal_observed+delay-time.monotonic()))
   capture=Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(),0,0,1600,1000)
   name=f'terminal-{round(delay*1000):04d}.png'
   capture.savev(str(work/name),'png',[],[])
   repaints.append({'elapsed':time.monotonic()-terminal_observed,'file':name,'send_accessible':find_name('Send') is not None,'error_accessible':find_text('Action failed. Stopped without replaying') is not None})
 time.sleep(.4);monitor_stop.set();monitor.join(2);report['samples']=samples
 assert samples and all(s['active']==guard for s in samples),'automation changed foreground ownership'
 assert all(s['focus']==baseline_focus for s in samples),'automation changed keyboard focus'
 assert all(s['pointer']==baseline_pointer for s in samples),'automation moved the physical pointer'
 assert not monitor_errors,'ownership/overlay monitor failed: '+str(monitor_errors)
 if check_no_overlay:
  report['overlay_samples']=len(samples)
  report['mapped_overlay_samples']=sum(bool(s['mapped_native_overlays']) for s in samples)
  report['visible_overlay_samples']=sum(any(o['bounding_area']>0 for o in s['mapped_native_overlays']) for s in samples)
  assert len(samples)>=10,'overlay visibility observation was too short'
  assert report['visible_overlay_samples']==0,'app-owned native cursor overlays became visible during input'
 events_path=work/'receiver-events.jsonl'
 events=[json.loads(line) for line in events_path.read_text().splitlines()] if events_path.exists() else []
 assert [e['button'] for e in events]==(['6','×','7','='] if mode=='complete' else []),'receiver callbacks disagree with the expected effects'
 if mode=='stop_model':
  assert json.loads((work/'fixture-state.json').read_text())=={'display':'0','buttons':[]},'late input after cancellation'
 assert all(not e['active'] for e in events),'receiver became active during a callback'
 if mode in ('key_chord','draft_value'):
  draft_events=[json.loads(line) for line in (work/'draft-events.jsonl').read_text().splitlines()]
  draft_events=[e for e in draft_events if e['time']>=samples[0]['time']]
  report['draft_events']=draft_events
  assert all(not e['active'] for e in draft_events),'keyboard receiver became active during a callback'
  if check_key_refusal:assert all(e['text']=='seed' and e['selection']==[] for e in draft_events),'refused keyboard task mutated the draft'
  else:assert draft_events,'keyboard receiver had no callbacks'
 import gi
 gi.require_version('Gdk','3.0')
 from gi.repository import Gdk
 image=Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(),0,0,1600,1000)
 image.savev(str(work/'completed.png'),'png',[],[])
 if mode=='draft_value':
  assert model_outcome.get('fresh_value') is True,'native value is absent from the fresh model observation'
  assert report['initial_value_observed'] and report['native_value_observed'],'native value projection is incomplete'
  assert report['draft_identity_preserved'] and report['native_value_diff']['kind']=='diff','value change lost identity or accessibility diff'
  assert report['native_value_predicate'] and report['wrong_value_refused'],'native predicate did not distinguish the actual value from the negative control'
 report['ok']=True
except Exception as error:
 report['ok']=False;report['error']=str(error);report['error_type']=type(error).__name__;report['traceback']=traceback.format_exc()
 try:
  report['ui_nodes']=[{'name':n.name,'role':n.getRoleName(),'text':node_text(n)} for n in nodes()][:500]
 except Exception:pass
 try:
  import gi
  gi.require_version('Gdk','3.0')
  from gi.repository import Gdk
  shot=Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(),0,0,1600,1000)
  shot.savev(str(work/'failure.png'),'png',[],[])
 except Exception:pass
finally:
 signal.alarm(0)
 monitor_stop.set()
 if monitor:monitor.join(2)
 report["samples"]=samples
 report['monitor_errors']=monitor_errors
 release_model.set()
 if server:server.shutdown();server.server_close()
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
 if driver_identity:
  driver_pid,driver_started=driver_identity
  # A parent that exits early can leave its daemon outside the direct-child
  # wait list. Check the exact process instance and clean up only that instance.
  if process_identity(driver_pid)==driver_started:
   try:os.kill(driver_pid,signal.SIGTERM)
   except ProcessLookupError:pass
   end=time.monotonic()+2
   while process_identity(driver_pid)==driver_started and time.monotonic()<end:time.sleep(.02)
  if process_identity(driver_pid)==driver_started:
   try:os.kill(driver_pid,signal.SIGKILL)
   except ProcessLookupError:pass
   end=time.monotonic()+2
   while process_identity(driver_pid)==driver_started and time.monotonic()<end:time.sleep(.02)
  report['owned_daemon_exited']=process_identity(driver_pid)!=driver_started
  if not report['owned_daemon_exited']:
   report['ok']=False;report.setdefault('error','owned daemon did not exit')
 for f in logs:f.close()
 report['owned_processes_reaped']=all(p.poll() is not None for p in processes)
 (work/'result.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps(report,indent=2))
 sys.exit(0 if report.get('ok') else 1)
