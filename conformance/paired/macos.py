#!/usr/bin/env python3
"""Run the same scripted native cases against OpenAI and ICU on an isolated Mac.

Uses the signed Codex app-server's supported MCP call path. No native IPC
impersonation, engine patching, account files, or local desktop interaction.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import time

APP = 'dev.icu.paired.fixture'
CASES = {'calculator': {'expected': {'display':'42','draft':'seed','Alpha':'alpha'}},
         'text_replacement': {'expected': {'display':'0','draft':'paired text','Alpha':'alpha'}},
         'invalid_element': {'expected': {'display':'0','draft':'seed','Alpha':'alpha','refused':True}}}

def case_catalog(suite):
    cases=json.loads(json.dumps(CASES))
    if suite=='expanded':
        extra=json.loads((Path(__file__).parent/'expanded/cases.json').read_text())
        for spec in extra.values():
            spec['expected']={**{'display':'0','draft':'seed','Alpha':'alpha'},**spec['expected']}
        cases.update(extra)
    return cases

def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def wait(fn, seconds=15):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        value=fn()
        if value:return value
        time.sleep(.1)
    raise TimeoutError('condition did not become ready')

def state(path):
    try:return json.loads(path.read_text())
    except (OSError,ValueError):return {}

def rpc_socket(path,name,args):
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(30);sock.connect(str(path));sock.sendall((json.dumps({'method':'call','name':name,'args':args,'session_id':'paired','client_kind':'cli'})+'\n').encode())
        data=b''
        while b'\n' not in data:
            part=sock.recv(65536)
            if not part:raise RuntimeError('driver disconnected')
            data+=part
            if len(data)>16*1024*1024:raise RuntimeError('driver reply exceeds cap')
        reply=json.loads(data.split(b'\n')[0]);
        if not reply.get('ok'):raise RuntimeError('driver transport refused')
        return reply['result']

class Candidate:
    def __init__(self,args,work):
        self.sock=work/'driver.sock';self.log=(work/'driver.log').open('w')
        self.proc=subprocess.Popen([str(args.driver),'serve','--no-overlay','--socket',str(self.sock)],stdout=self.log,stderr=self.log,env=dict(os.environ,CUA_DRIVER_EMBEDDED='1'))
        wait(self.sock.exists);self.base={}
    def bind(self,st):self.base={'pid':st['pid'],'window_id':st['window']}
    def observe(self):
        reply=rpc_socket(self.sock,'get_window_state',{**self.base,'include_text':True,'include_screenshot':False,'disable_diff':True})
        if reply.get('isError'):raise RuntimeError(str(reply))
        self.state=reply.get('structuredContent',reply);return self.state
    def target(self,label):
        matches=[e for e in self.state.get('elements',[]) if label in [e.get('label'),e.get('title'),e.get('description')]]
        if len(matches)!=1:raise RuntimeError(f'target {label!r} not unique')
        return matches[0]['element_index']
    def action(self,kind,label=None,value=None,index=None,**extra):
        args={**self.base,'element_index':self.target(label) if index is None else index}
        args['snapshot_id']=self.state['snapshot_id']
        if kind=='set_value':args['value']=value
        if kind=='scroll':args.update(direction=extra['direction'],amount=extra['amount'],by='page')
        return rpc_socket(self.sock,kind,args)
    def close(self):
        self.proc.terminate()
        try:self.proc.wait(8)
        except subprocess.TimeoutExpired:self.proc.kill();self.proc.wait()
        self.log.close();return self.proc.poll() is not None

class Reference:
    def __init__(self,args,work):
        sys.path.insert(0,str(args.host_harness));from rpc import Rpc
        self.rpc=Rpc([str(args.codex),'app-server'],work/'appserver.jsonl',cwd='/Users/runner/golden/work',jsonrpc_field=False,on_request=self.approve)
        self.rpc.request('initialize',{'clientInfo':{'name':'icu-paired-native','version':'1'},'capabilities':{'experimentalApi':True}},timeout=60);self.rpc.notify('initialized')
        self.thread=self.rpc.request('thread/start',{'cwd':'/Users/runner/golden/work','approvalPolicy':'on-request','sandbox':'workspace-write'},timeout=120)['thread']['id']
        self.js('globalThis.sky=(await import("@oai/sky")).sky; nodeRepl.write("ready")')
    def approve(self,msg):
        params=msg.get('params',{});meta=params.get('_meta',{});target=meta.get('tool_params',{}).get('app')
        allowed=msg.get('method')=='mcpServer/elicitation/request' and target==APP
        return {'action':'accept' if allowed else 'decline','content':{},'_meta':{'persist':'session'}}
    def js(self,code):
        reply=self.rpc.request('mcpServer/tool/call',{'server':'node_repl','threadId':self.thread,'tool':'js','arguments':{'code':code}},timeout=120)
        if reply.get('isError'):raise RuntimeError(str(reply))
        return '\n'.join(x.get('text','') for x in reply.get('content',[]) if x.get('type')=='text')
    def bind(self,st):pass
    def observe(self):
        result=self.js('nodeRepl.write((await sky.get_app_state('+json.dumps({'app':APP,'disableDiff':True})+')).text)')
        self.state=result;return {'text':result}
    def target(self,label):
        rows=[]
        for line in self.state.splitlines():
            m=re.match(r'^\s*(\d+) (.*)',line)
            if m and (re.fullmatch(r'(?:\(selected\) )?'+re.escape(label),m[2]) or re.search(r'^(?:button|checkbox|radio button|menu item) '+re.escape(label)+r'(?:,|$)',m[2]) or re.search(r'(?:^| )Description: '+re.escape(label)+r'(?:,|$)',m[2])):rows.append(int(m[1]))
        if len(rows)!=1:raise RuntimeError(f'target {label!r} not unique: {rows}')
        return rows[0]
    def action(self,kind,label=None,value=None,index=None,**extra):
        args={'app':APP,'element_index':self.target(label) if index is None else index}
        if kind=='set_value':args['value']=value
        if kind=='scroll':args.update(direction=extra['direction'],pages=extra['amount'])
        result=self.js('try {await sky.'+kind+'('+json.dumps(args)+'); nodeRepl.write(JSON.stringify({isError:false}))} catch(e) {nodeRepl.write(JSON.stringify({isError:true,error:String(e)}))}')
        return json.loads(result)
    def close(self):
        self.rpc.close();return self.rpc.proc.poll() is not None

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--engine',choices=['openai-native','icu-native'],required=True)
    p.add_argument('--fixture',type=Path,required=True);p.add_argument('--driver',type=Path,required=True)
    p.add_argument('--codex',type=Path,default=Path('/Applications/ChatGPT.app/Contents/Resources/codex'))
    p.add_argument('--service',type=Path,default=Path('/Users/runner/.codex/computer-use/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService'))
    p.add_argument('--host-harness',type=Path,default=Path('/Users/runner/golden/remote'))
    p.add_argument('--guard',type=Path,help='Owned foreground sentinel; requires --ui')
    p.add_argument('--ui',type=Path,help='Native ownership sampler; requires --guard')
    p.add_argument('--work',type=Path,required=True);p.add_argument('--suite',choices=['basic','expanded'],default='basic');args=p.parse_args();cases=case_catalog(args.suite)
    if bool(args.guard) != bool(args.ui):p.error('--guard and --ui must be supplied together')
    if sys.platform!='darwin' or os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise SystemExit('explicit disposable macOS host required')
    args.work.mkdir(parents=True,exist_ok=False)
    report={'schema':1,'engine':args.engine,'mode':'scripted-native','executed':False,'cleanup_verified':False,'cases':[],
            'os':subprocess.check_output(['sw_vers','-productVersion'],text=True).strip(),'display':None,
            'fixture_sha256':sha(args.fixture),'harness_sha256':sha(__file__),'engine_sha256':sha(args.service if args.engine=='openai-native' else args.driver)}
    adapter=None;fixtures=[]
    try:
        adapter=Reference(args,args.work) if args.engine=='openai-native' else Candidate(args,args.work)
        report['executed']=True
        for name,spec in cases.items():
            work=args.work/name;work.mkdir();case={'id':name,'executed':False,'observations':[],'actions':[]};report['cases'].append(case)
            log=(work/'fixture.log').open('w');fx=subprocess.Popen([str(args.fixture),str(work)],stdout=log,stderr=log);fixtures.append(fx)
            guard=None;sampler=None
            try:
                baseline=wait(lambda:state(work/'state.json'));adapter.bind(baseline);case['executed']=True
                report['display']=baseline['screen']
                if args.guard:
                    guard=subprocess.Popen([str(args.guard),str(work)],stdout=log,stderr=log);fixtures.append(guard)
                    wait(lambda:state(work/'guard.json'))
                    def ownership():return json.loads(subprocess.check_output([str(args.ui),'state'],text=True))
                    before=wait(lambda:(lambda v:v if v['front_pid']==guard.pid and v.get('focused_element',{}).get('pid')==guard.pid else None)(ownership()))
                    case['ownership_baseline']=before
                    sampler=subprocess.Popen([str(args.ui),'sample',str(work/'ownership.jsonl')],stdout=log,stderr=log)
                    time.sleep(.3)
                case['observations'].append(adapter.observe());refused=False
                actions=spec.get('actions') or ([{'kind':'click','label':label} for label in ['6','×','7','=']] if name=='calculator' else [{'kind':'set_value','label':'Draft','value':'paired text'}] if name=='text_replacement' else [{'kind':'click','index':99999}])
                for action in actions:
                    result=adapter.action(**action);case['actions'].append({**action,'result':result})
                    if action.get('index') is not None:refused=result.get('isError') is True
                    elif result.get('isError'):raise RuntimeError(str(result))
                    time.sleep(.2);case['observations'].append(adapter.observe())
                raw=wait(lambda:state(work/'state.json'));case['outcome']={k:raw[k] for k in spec['expected'] if k!='refused'}
                if name=='invalid_element':case['outcome']['refused']=refused
            except Exception as e:case['error']=str(e)
            finally:
                if sampler:
                    time.sleep(.4);sampler.send_signal(signal.SIGTERM);sampler.wait(5)
                    samples=[json.loads(line) for line in (work/'ownership.jsonl').read_text().splitlines()]
                    baseline=case['ownership_baseline'];first=samples[0] if samples else baseline
                    violations=[x for x in samples if x['front_pid']!=guard.pid or x.get('focused_element')!=baseline.get('focused_element') or x.get('menubar_pid')!=baseline.get('menubar_pid') or x['pointer']!=baseline['pointer']]
                    hid=[x for x in samples if x['hid_idle']+.25<first['hid_idle']+x['time']-first['time']]
                    case['ownership']={'sample_count':len(samples),'violations':len(violations),'hid_resets':len(hid),'guard_text':state(work/'guard.json').get('text'),'first_violation':violations[:1],'first_hid_reset':hid[:1]}
                    if len(samples)<20:case['error']='insufficient ownership samples'
                if guard:guard.terminate();guard.wait(5)
                fx.terminate()
                try:fx.wait(5)
                except subprocess.TimeoutExpired:fx.kill();fx.wait()
                log.close()
    except Exception as e:report['error']=str(e)
    finally:
        report['cleanup_verified']=(adapter.close() if adapter else True) and all(p.poll() is not None for p in fixtures)
        (args.work/'result.json').write_text(json.dumps(report,indent=2)+'\n')
        (args.work/'contract.json').write_text(json.dumps({'mode':'scripted-native','fixture_sha256':report['fixture_sha256'],'cases':cases,'ownership_required':bool(args.guard)},indent=2)+'\n')
        print(json.dumps({k:v for k,v in report.items() if k!='cases'}))
    return 0 if report['executed'] and len(report['cases'])==len(cases) and all(not c.get('error') for c in report['cases']) else 1
if __name__=='__main__':raise SystemExit(main())
