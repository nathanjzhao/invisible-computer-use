#!/usr/bin/env python3
"""Scripted native acceptance for explicit foreground typing, keys and dragging."""
import argparse,json,os,subprocess,sys,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
if os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('owned disposable host required')
sys.path.insert(0,str(a.root/'tests'));from macos_probe import Session,wait_for,HARNESS_CAPTURE_PROMPT
out=a.out;out.mkdir(parents=True,exist_ok=False);report={'mode':'scripted-native','passed':False,'actions':[]};s=Session(out,a.root/'Copilot.app',a.root/'MemoryLab.app/Contents/MacOS/MemoryLab',a.root/'macos_ui',report);editor=None

def call(name,args):
 raw=s.call(name,args,timeout=20);value=raw.get('structuredContent',raw)
 if raw.get('isError') or value.get('code'):raise RuntimeError(name+': '+str(value))
 return value

def front_app():
 # This is explicit user setup on the disposable desktop, before sampling.
 subprocess.run(['open','-a',str(s.bundle)],check=True)
 wait_for(lambda:s.ui('state')['front_pid']==s.app_pid,5,'setup did not focus controller')

def act(name,pid,wid,**args):
 result=call(name,{'pid':pid,'window_id':wid,'delivery_mode':'foreground',**args});time.sleep(.15)
 report['actions'].append({'name':name,'result':result,'front_after':s.ui('state')['front_pid']})
 if s.ui('state')['front_pid']!=s.app_pid:raise RuntimeError('foreground action did not restore controller')
try:
 s.start_app(['ICU_APP_MEMORY_ROOT='+str(out/'memory')]);subprocess.run(['/usr/sbin/screencapture','-x',str(out/'setup.png')],check=True);s.approve_capture_prompt(prompt=HARNESS_CAPTURE_PROMPT)
 document=out/'Foreground.txt';document.write_text('seed text\n');subprocess.run(['open','-a','TextEdit',str(document)],check=True)
 editor=wait_for(lambda:(s.ui('pid-of','com.apple.TextEdit')['pids'] or [None])[0],10,'editor missing')
 win=wait_for(lambda:next((w for w in s.windows() if w['pid']==editor and w['title']=='Foreground.txt'),None),10,'document missing');wid=win['window_id'];front_app()
 state=call('get_window_state',{'pid':editor,'window_id':wid,'include_screenshot':False,'include_accessibility_tree':True})
 field=next(e for e in state['elements'] if e.get('role')=='AXTextArea')
 call('click',{'pid':editor,'window_id':wid,'element_token':field['element_token']})
 act('hotkey',editor,wid,keys=['cmd','a']);act('press_key',editor,wid,key='backspace')
 act('type_text',editor,wid,text='Native foreground entry');act('press_key',editor,wid,key='return');act('hotkey',editor,wid,keys=['cmd','s'])
 report['saved_text']=document.read_text();assert report['saved_text']=='Native foreground entry\n','saved text differs from requested input'
 pad=out/'pointer.json';fx=s.launch(['/Users/runner/golden/permission/PointerFixture',str(pad)],'pointer-fixture');wait_for(pad.exists,10,'pointer fixture missing')
 win=wait_for(lambda:next((w for w in s.windows() if w['pid']==fx.pid and w['title']=='Pointer capture fixture'),None),10,'pointer window missing');wid=win['window_id'];front_app()
 shot=out/'pad.png';state=call('get_window_state',{'pid':fx.pid,'window_id':wid,'include_screenshot':True,'include_accessibility_tree':True,'max_dimension':640,'screenshot_out_file':str(shot)})
 width,height=state['screenshot_width'],state['screenshot_height'];b=win['bounds'];n=next(n for n in s.ui('tree',fx.pid)['nodes'] if n.get('name')=='Pointer pad');f=n['frame'];sx=width/b['width'];sy=height/b['height']
 x=(f['x']+100-b['x'])*sx;y=(f['y']+100-b['y'])*sy
 act('drag',fx.pid,wid,from_x=x,from_y=y,to_x=x+100*sx,to_y=y+40*sy,screenshot_id=state['screenshot_id'])
 report['pointer']=json.loads(pad.read_text());kinds=[row['kind'] for row in report['pointer']]
 assert kinds.count('down')==1 and kinds.count('up')==1 and 'drag' in kinds,'native drag gesture incomplete'
 report['passed']=True
except Exception as e:report['error']=str(e)
finally:
 s.close()
 if editor:
  s.ui('quit',editor,check=False)
 report['passed'] &= report.get('owned_daemon_exited') is True and report.get('owned_processes_reaped') is True
 (out/'result.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
raise SystemExit(0 if report['passed'] else 1)
