"""Build the native Contacts helper without launching it or requesting local access."""
import argparse,pathlib,subprocess,hashlib,json
parser=argparse.ArgumentParser();parser.add_argument('--output',type=pathlib.Path,required=True);args=parser.parse_args()
root=pathlib.Path(__file__).resolve().parents[1];bundle=args.output.resolve()/'Copilot Contacts.app';macos=bundle/'Contents/MacOS';macos.mkdir(parents=True,exist_ok=True)
(bundle/'Contents/Info.plist').write_bytes((root/'native/Info.plist').read_bytes())
subprocess.run(['swiftc','-O',str(root/'native/ContactsBridge.swift'),'-o',str(macos/'icu-messages-contacts'),'-framework','Contacts','-framework','Foundation'],check=True)
resources=bundle/'Contents/Resources';resources.mkdir(exist_ok=True)
manifest={'sources':{name:hashlib.sha256((root/'native'/name).read_bytes()).hexdigest() for name in ['ContactsBridge.swift','Info.plist','Contacts.entitlements.plist']},'swift':subprocess.check_output(['swiftc','--version'],text=True)}
(resources/'build-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
subprocess.run(['codesign','--force','--entitlements',str(root/'native/Contacts.entitlements.plist'),'--sign','-',str(bundle)],check=True)
print(bundle)
