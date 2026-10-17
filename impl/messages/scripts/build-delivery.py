"""Compile the native AppleEvents helper. Does not launch it or send a message."""
import argparse,hashlib,json,pathlib,plistlib,subprocess
parser=argparse.ArgumentParser();parser.add_argument('--output',type=pathlib.Path,required=True);parser.add_argument('--fixture-target',help='Compile only for a dedicated synthetic scripting app');args=parser.parse_args()
root=pathlib.Path(__file__).resolve().parents[1];bundle=args.output.resolve()/'Copilot Messages.app';mac=bundle/'Contents/MacOS';mac.mkdir(parents=True,exist_ok=True);resources=bundle/'Contents/Resources';resources.mkdir(exist_ok=True)
info=plistlib.loads((root/'native/Messages.Info.plist').read_bytes());flags=[]
if args.fixture_target:
 assert args.fixture_target.startswith('com.nathanzhao.fixture.') and all(c.isalnum() or c in '.-' for c in args.fixture_target)
 flags=['-DCUA_MESSAGES_TARGET="'+args.fixture_target+'"'];info['CFBundleIdentifier']='com.nathanzhao.messages.delivery.fixture'
(bundle/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
subprocess.run(['clang','-O2','-fobjc-arc','-Wno-deprecated-declarations',*flags,str(root/'native/MessagesBridge.m'),'-framework','Foundation','-framework','AppKit','-framework','ScriptingBridge','-framework','Carbon','-o',str(mac/'icu-messages-delivery')],check=True)
names=['MessagesBridge.m','MessagesScripting.h','Messages.Info.plist','Messages.entitlements.plist'];manifest={'sources':{name:hashlib.sha256((root/'native'/name).read_bytes()).hexdigest() for name in names},'target':args.fixture_target or 'com.apple.MobileSMS','clang':subprocess.check_output(['clang','--version'],text=True)}
(resources/'build-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
subprocess.run(['codesign','--force','--entitlements',str(root/'native/Messages.entitlements.plist'),'--sign','-',str(bundle)],check=True)
print(bundle)
