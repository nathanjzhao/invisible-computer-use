#!/usr/bin/env python3
"""Stage the pinned observer patch in a fresh copy; never edit an installed recorder."""
import argparse,hashlib,json,shutil,subprocess
from pathlib import Path

HERE=Path(__file__).resolve().parent

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True,help='directory containing taiku-observer and taiku-observer-schema');p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 m=json.loads((HERE/'manifest.json').read_text())
 helper=json.loads((HERE/'audio-helper-manifest.json').read_text())
 for name,expected in helper['files'].items():
  source=a.source/'taiku-observer-audio-helper'/name
  if not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest()!=expected:raise SystemExit('macOS audio helper source changed: '+name)
 for item in m['files']:
  source=a.source/item['path'];actual=hashlib.sha256(source.read_bytes()).hexdigest() if source.exists() else None
  if actual!=item['base_sha256']:raise SystemExit('observer base changed: '+item['path']+'; review and rebase the patch first')
 if a.out.exists():raise SystemExit('use a fresh staging directory')
 a.out.mkdir(parents=True)
 for name in ['taiku-observer','taiku-observer-schema','taiku-observer-audio-helper']:
  shutil.copytree(a.source/name,a.out/name,ignore=shutil.ignore_patterns('target','.git','.env*'))
 subprocess.run(['git','apply','--check',str(HERE/'taiku-observer-clicks.patch')],cwd=a.out,check=True)
 subprocess.run(['git','apply',str(HERE/'taiku-observer-clicks.patch')],cwd=a.out,check=True)
 for item in m['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['patched_sha256']:raise SystemExit('patched bytes differ from manifest: '+item['path'])
 audio=json.loads((HERE/'audio-manifest.json').read_text())
 for item in audio['files']:
  source=a.out/item['path'];actual=hashlib.sha256(source.read_bytes()).hexdigest() if source.exists() else None
  if actual!=item['base_sha256']:raise SystemExit('audio patch base changed: '+item['path'])
 subprocess.run(['git','apply','--check',str(HERE/'taiku-observer-audio.patch')],cwd=a.out,check=True)
 subprocess.run(['git','apply',str(HERE/'taiku-observer-audio.patch')],cwd=a.out,check=True)
 for item in audio['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['patched_sha256']:raise SystemExit('audio patched bytes differ: '+item['path'])
 timing=json.loads((HERE/'audio-timing-manifest.json').read_text())
 for item in timing['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['base_sha256']:raise SystemExit('audio timing base changed: '+item['path'])
 subprocess.run(['git','apply','--check',str(HERE/'taiku-observer-audio-timing.patch')],cwd=a.out,check=True)
 subprocess.run(['git','apply',str(HERE/'taiku-observer-audio-timing.patch')],cwd=a.out,check=True)
 for item in timing['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['patched_sha256']:raise SystemExit('audio timing bytes differ: '+item['path'])
 limit=json.loads((HERE/'audio-limit-manifest.json').read_text())
 for item in limit['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['base_sha256']:raise SystemExit('audio limit base changed: '+item['path'])
 subprocess.run(['git','apply','--check',str(HERE/'taiku-observer-audio-limit.patch')],cwd=a.out,check=True)
 subprocess.run(['git','apply',str(HERE/'taiku-observer-audio-limit.patch')],cwd=a.out,check=True)
 for item in limit['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['patched_sha256']:raise SystemExit('audio limit bytes differ: '+item['path'])
 ready=json.loads((HERE/'audio-ready-manifest.json').read_text())
 for item in ready['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['base_sha256']:raise SystemExit('audio readiness base changed: '+item['path'])
 subprocess.run(['git','apply','--check',str(HERE/'taiku-observer-audio-ready.patch')],cwd=a.out,check=True)
 subprocess.run(['git','apply',str(HERE/'taiku-observer-audio-ready.patch')],cwd=a.out,check=True)
 for item in ready['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['patched_sha256']:raise SystemExit('audio readiness bytes differ: '+item['path'])
 m['audio']=audio
 m['audio_timing']=timing
 m['audio_limit']=limit
 m['audio_ready']=ready
 m['audio_helper']=helper
 linux=json.loads((HERE/'linux-audio-manifest.json').read_text())
 for item in linux['files']:
  source=a.out/item['path'];actual=hashlib.sha256(source.read_bytes()).hexdigest() if source.exists() else None
  if actual!=item['base_sha256']:raise SystemExit('Linux audio patch base changed: '+item['path'])
 subprocess.run(['git','apply','--check',str(HERE/'taiku-observer-linux-audio.patch')],cwd=a.out,check=True)
 subprocess.run(['git','apply',str(HERE/'taiku-observer-linux-audio.patch')],cwd=a.out,check=True)
 for item in linux['files']:
  if hashlib.sha256((a.out/item['path']).read_bytes()).hexdigest()!=item['patched_sha256']:raise SystemExit('Linux audio patched bytes differ: '+item['path'])
 m['linux_audio']=linux
 (a.out/'integration-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
 print('Prepared observer sources; no recorder was started.')

if __name__=='__main__':main()
