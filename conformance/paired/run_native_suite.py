#!/usr/bin/env python3
"""Run and grade both native engines on a prepared, disposable desktop.

Provisioning and OS permission grants are separate. This runner never installs a
service or copies credentials. Input requires ICU_DISPOSABLE_HOST=1. --check-only
verifies preparation and cannot report a live pass.
"""
import argparse,json,os,shutil,signal,subprocess,sys
from pathlib import Path

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
from verdict import compare,digest


def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--platform',choices=['macos','windows'],required=True)
 p.add_argument('--work',type=Path,required=True)
 p.add_argument('--fixture',type=Path,required=True)
 p.add_argument('--candidate-driver',type=Path,required=True)
 p.add_argument('--reference-executable',type=Path,required=True)
 p.add_argument('--reference-cli',type=Path,required=True)
 p.add_argument('--guard',type=Path,required=True)
 p.add_argument('--ui',type=Path,help='macOS ownership sampler')
 p.add_argument('--host-harness',type=Path,help='macOS signed app-server RPC helper directory')
 p.add_argument('--reference-client',type=Path,help='Windows directory containing helper_transport.js, codex_turn_metadata.js and tslib.es6.mjs')
 p.add_argument('--suite',choices=['basic','expanded'],default='basic');p.add_argument('--node',default='node');p.add_argument('--check-only',action='store_true');args=p.parse_args()
 args.work.mkdir(parents=True,exist_ok=False)
 report={'schema':1,'executed':False,'passed':False,'platform':args.platform,'status':'preparing','artifacts':{}}
 try:
  inputs={'fixture':args.fixture,'candidate_driver':args.candidate_driver,'reference_executable':args.reference_executable,'reference_cli':args.reference_cli,'guard':args.guard}
  if args.platform=='macos':
   if not args.ui or not args.host_harness:p.error('macOS requires --ui and --host-harness')
   inputs['ui']=args.ui;inputs['rpc']=args.host_harness/'rpc.py'
  else:
   if not args.reference_client:p.error('Windows requires --reference-client')
   for name in ['helper_transport.js','codex_turn_metadata.js','tslib.es6.mjs']:inputs[name]=args.reference_client/name
  for key,path in inputs.items():
   if not path.is_file():raise ValueError(f'{key} file is missing')
   report['artifacts'][key]={'path':str(path.resolve()),'sha256':digest(path)}
  if args.check_only:
   report['status']='prepared_not_executed';return 0
  expected='darwin' if args.platform=='macos' else 'win32'
  if sys.platform!=expected or os.environ.get('ICU_DISPOSABLE_HOST')!='1':raise ValueError('live runs require the selected OS and an explicitly disposable host')
  stages=args.work/'inputs';stages.mkdir()
  if args.platform=='windows':
   for source,name in [(args.fixture,'PairFixture.exe'),(args.guard,'PairGuard.exe'),(args.candidate_driver,'cua-driver.exe'),(args.reference_executable,'codex-computer-use.exe')]:shutil.copy2(source,stages/name)
   shutil.copytree(HERE/'expanded',stages/'expanded');shutil.copytree(args.reference_client,stages/'reference');shutil.copy2(HERE/'windows.mjs',stages/'windows.mjs');(stages/'package.json').write_text('{"type":"module"}\n')
  report['executed']=True;report['runs']=[]
  for engine in ['openai-native','icu-native']:
   output=args.work/engine
   if args.platform=='macos':
    command=[sys.executable,str(HERE/'macos.py'),'--engine',engine,'--suite',args.suite,'--fixture',str(args.fixture.resolve()),'--driver',str(args.candidate_driver.resolve()),'--codex',str(args.reference_cli.resolve()),'--service',str(args.reference_executable.resolve()),'--host-harness',str(args.host_harness.resolve()),'--guard',str(args.guard.resolve()),'--ui',str(args.ui.resolve()),'--work',str(output.resolve())]
   else:command=[args.node,str(stages/'windows.mjs'),engine,str(stages.resolve()),str(output.resolve()),'with-ownership',args.suite]
   env=dict(os.environ);env['PATH']=str(args.reference_cli.resolve().parent)+os.pathsep+env.get('PATH','')
   with (args.work/(engine+'.log')).open('w') as log:
    process=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,env=env,start_new_session=True)
    try:code=process.wait(timeout=900)
    except (subprocess.TimeoutExpired,KeyboardInterrupt):
     if os.name=='nt':subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
     else:os.killpg(process.pid,signal.SIGKILL)
     process.wait();raise
   report['runs'].append({'engine':engine,'exit_code':code})
  a,b=[json.loads((args.work/engine/'result.json').read_text()) for engine in ['openai-native','icu-native']]
  contract=json.loads((args.work/'openai-native/contract.json').read_text());result=compare(a,b,contract)
  result['passed']=result['passed'] and all(r['exit_code']==0 for r in report['runs'])
  (args.work/'comparison.json').write_text(json.dumps(result,indent=2)+'\n')
  report.update(status='passed' if result['passed'] else 'failed',passed=result['passed'])
  return 0 if report['passed'] else 1
 except (OSError,ValueError,subprocess.SubprocessError) as error:
  report.update(status='failed',error=str(error));return 1
 finally:
  (args.work/'suite.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))

if __name__=='__main__':raise SystemExit(main())
