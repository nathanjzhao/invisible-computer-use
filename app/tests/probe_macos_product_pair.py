#!/usr/bin/env python3
"""Our packaged UI on the exact reference fixture/goals, with the same model.

Account sign-in must already exist on the disposable host. The app uses its own
Codex-login adapter; this probe never reads or copies its authentication file.
"""
import hashlib,json,os,signal,sys,time
from pathlib import Path
from macos_probe import Session,wait_for,center
if sys.platform!='darwin' or os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('disposable Mac required')
root=Path(sys.argv[1]);bundle=Path(sys.argv[2]);fixture=Path(sys.argv[3]);ui=Path(sys.argv[4]);model=sys.argv[5]
root.mkdir(parents=True,exist_ok=False)
TASKS={
 'calculator':('Compute six times seven by using the calculator buttons. Read the result before finishing.',{'display':'42','draft':'seed','Alpha':'alpha'}),
 'text_replacement':('Replace the Draft field text with paired text. Keep the Alpha document unchanged.',{'display':'0','draft':'paired text','Alpha':'alpha'}),
}
report={'engine':'icu-packaged-app','mode':'model-driven-product','model':model,'passed':False,'cases':[],
        'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),
        'harness_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
signal.signal(signal.SIGALRM,lambda *_:(_ for _ in ()).throw(TimeoutError('product comparison timeout')));signal.alarm(600)
try:
 for case,(goal,expected) in TASKS.items():
  work=root/case;work.mkdir();r={'id':case,'goal':goal,'expected':expected,'passed':False};report['cases'].append(r);s=Session(work,bundle,fixture,ui,r)
  try:
   data=work/'fixture';data.mkdir();fx=s.launch([str(fixture),str(data)],'fixture')
   def state():
    try:return json.loads((data/'state.json').read_text())
    except (OSError,ValueError):return {}
   wait_for(state,10,'fixture not ready')
   s.start_app(['ICU_APP_MEMORY_ROOT='+str(work/'memory'),'ICU_OPENAI_MODEL='+model])
   s.ui('press',s.app_pid,json.dumps({'role':'AXPopUpButton','name':'Which model drives'}));time.sleep(.2)
   s.ui('press',s.app_pid,json.dumps({'role':'AXMenuItem','name':'OpenAI'}))
   wait_for(lambda:s.find(s.named('Which model drives','AXPopUpButton')).get('value')=='OpenAI',5,'provider selection failed')
   for label in ['Give this task the stored memories for its app','Let memory learn from this task']:
    n=s.find(s.named(label,'AXCheckBox'))
    if n and n.get('value')=='1':s.ui('press',s.app_pid,json.dumps({'role':'AXCheckBox','name':label}))
   s.pick_window('Paired task fixture')
   s.enter_goal(goal);s.press('Send');started=time.monotonic()
   wait_for(lambda:s.find(s.named('Stop','AXButton')),15,'task did not start')
   wait_for(lambda:s.find(s.named('Stop','AXButton')) is None and s.find(s.named('Send','AXButton')),200,'task never reached terminal event')
   r['wall_seconds']=round(time.monotonic()-started,2);raw=state();r['outcome']={k:raw[k] for k in expected};r['fixture_state']=raw
   r['chat']=[n['name'] for n in s.tree() if n.get('role')=='AXStaticText' and n.get('name')]
   r['model_confirmed']=any('codex-oauth:'+model in line for line in r['chat'])
   r['passed']=r['outcome']==expected and r['model_confirmed'] and (case!='calculator' or raw['buttons'][-4:]==['6','×','7','='])
   s.screenshot('after.png')
  except Exception as e:r['error']=str(e);r['ui_nodes']=s.tree() if s.app_pid else [];s.screenshot('failure.png')
  finally:s.close();r['passed']=r['passed'] and r.get('owned_daemon_exited') is True and r.get('owned_processes_reaped') is True;(work/'result.json').write_text(json.dumps(r,indent=2)+'\n')
 report['passed']=len(report['cases'])==len(TASKS) and all(r['passed'] for r in report['cases'])
finally:
 signal.alarm(0);(root/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({**report,'cases':[{k:v for k,v in r.items() if k not in ['chat','ui_nodes']} for r in report['cases']]}))
raise SystemExit(0 if report['passed'] else 1)
