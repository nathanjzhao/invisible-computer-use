"""Build a private developer package with complete vendored Cargo sources."""
import argparse,hashlib,json,os,pathlib,shutil,subprocess,tarfile,tempfile
parser=argparse.ArgumentParser()
parser.add_argument('--output',type=pathlib.Path,required=True)
parser.add_argument('--target-dir',type=pathlib.Path)
parser.add_argument('--toolchain',default='1.97.1')
parser.add_argument('--contacts-bundle',type=pathlib.Path,help='Use an already built/signed native Contacts bundle')
parser.add_argument('--delivery-bundle',type=pathlib.Path,help='Use an already built/signed native Messages delivery bundle')
args=parser.parse_args();root=pathlib.Path(__file__).resolve().parents[1];output=args.output.resolve()
if output.exists():raise SystemExit('Package output must be a new directory')
top_files={'Cargo.toml','Cargo.lock','LICENSE','NOTICE.md','README.md','.gitignore'}
source_dirs={'src','tests','examples','schema','scripts','native'}
paths=sorted(p for p in root.rglob('*') if p.is_file() and (p.relative_to(root).parts[0] in source_dirs or p.name in top_files and p.parent==root) and '__pycache__' not in p.parts)
sources={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
environment=os.environ.copy();environment['RUSTC_WRAPPER']=''
target=(args.target_dir or root/'target').resolve();environment['CARGO_TARGET_DIR']=str(target)
cargo=['rustup','run',args.toolchain,'cargo']
subprocess.run([*cargo,'build','--manifest-path',str(root/'Cargo.toml'),'--release','--locked','--offline'],env=environment,check=True)
assert all(hashlib.sha256((root/name).read_bytes()).hexdigest()==digest for name,digest in sources.items()),'Source changed during build'
output.parent.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='icu-messages-package-',dir=output.parent) as temporary:
 stage=pathlib.Path(temporary);source=stage/'source';source.mkdir()
 for path in paths:
  destination=source/path.relative_to(root);destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,destination)
 assert all(hashlib.sha256((source/name).read_bytes()).hexdigest()==digest for name,digest in sources.items()),'Source changed while copying package contents'
 vendor=subprocess.run([*cargo,'vendor','--manifest-path',str(root/'Cargo.toml'),'--locked','--offline','--versioned-dirs',str(source/'vendor')],env=environment,capture_output=True,text=True,check=True)
 config=vendor.stdout.replace(str(source/'vendor'),'vendor')
 (source/'.cargo').mkdir(exist_ok=True);(source/'.cargo/config.toml').write_text(config)
 with tarfile.open(stage/'source.tar.gz','w:gz') as archive:archive.add(source,arcname='icu-messages-source')
 binary=stage/'icu-messages';shutil.copy2(target/'release/icu-messages',binary)
 helper_hash=None
 delivery_hash=None
 if __import__('sys').platform=='darwin':
  if args.contacts_bundle:
   subprocess.run(['codesign','--verify','--deep','--strict',str(args.contacts_bundle.resolve())],check=True)
   helper_manifest=json.loads((args.contacts_bundle.resolve()/'Contents/Resources/build-manifest.json').read_text())
   for name,digest in helper_manifest['sources'].items():assert hashlib.sha256((root/'native'/name).read_bytes()).hexdigest()==digest,'Contacts helper source mismatch: '+name
   shutil.copytree(args.contacts_bundle.resolve(),stage/'Copilot Contacts.app')
  else:subprocess.run(['python3',str(root/'scripts/build-contacts.py'),'--output',str(stage)],check=True)
  helper_hash=hashlib.sha256((stage/'Copilot Contacts.app/Contents/MacOS/icu-messages-contacts').read_bytes()).hexdigest()
  if args.delivery_bundle:
   bundle=args.delivery_bundle.resolve();subprocess.run(['codesign','--verify','--deep','--strict',str(bundle)],check=True)
   helper_manifest=json.loads((bundle/'Contents/Resources/build-manifest.json').read_text());assert helper_manifest['target']=='com.apple.MobileSMS','Refusing a fixture delivery helper'
   assert set(helper_manifest['sources'])=={'MessagesBridge.m','MessagesScripting.h','Messages.Info.plist','Messages.entitlements.plist'}
   for name,digest in helper_manifest['sources'].items():assert hashlib.sha256((root/'native'/name).read_bytes()).hexdigest()==digest,'Messages helper source mismatch: '+name
   shutil.copytree(bundle,stage/'Copilot Messages.app')
  else:subprocess.run(['python3',str(root/'scripts/build-delivery.py'),'--output',str(stage)],check=True)
  delivery_hash=hashlib.sha256((stage/'Copilot Messages.app/Contents/MacOS/icu-messages-delivery').read_bytes()).hexdigest()
 for name in ['LICENSE','NOTICE.md','README.md']:shutil.copy2(root/name,stage/name)
 (stage/'mcp-server.json').write_text(json.dumps({'command':str(output/'icu-messages'),'args':['messages','mcp']},indent=2)+'\n')
 manifest={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'contacts_helper_sha256':helper_hash,'delivery_helper_sha256':delivery_hash,'source_archive_sha256':hashlib.sha256((stage/'source.tar.gz').read_bytes()).hexdigest(),'source_files':sources,'vendored_packages':len(list((source/'vendor').iterdir())),'toolchain':subprocess.check_output(['rustup','run',args.toolchain,'rustc','-Vv'],text=True),'signed':False,'published':False}
 (stage/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
 shutil.rmtree(source);stage.rename(output)
print(json.dumps({'package':str(output),'binary_sha256':manifest['binary_sha256'],'vendored_packages':manifest['vendored_packages'],'signed':False,'published':False},indent=2))
