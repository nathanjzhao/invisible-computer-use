"""Build an invisible scripting fixture that receives only its own AppleEvents."""
import argparse,pathlib,plistlib,subprocess,hashlib,json
parser=argparse.ArgumentParser();parser.add_argument('--output',type=pathlib.Path,required=True);args=parser.parse_args();root=pathlib.Path(__file__).resolve().parents[1]
bundle=args.output.resolve()/'Messages Transport Fixture.app';mac=bundle/'Contents/MacOS';mac.mkdir(parents=True,exist_ok=True);resources=bundle/'Contents/Resources';resources.mkdir(exist_ok=True)
(resources/'Fixture.sdef').write_bytes((root/'tests/native/Fixture.sdef').read_bytes())
info={'CFBundleIdentifier':'com.nathanzhao.fixture.Messages','CFBundleExecutable':'fixture','CFBundleName':'Messages Transport Fixture','CFBundlePackageType':'APPL','CFBundleVersion':'1','LSUIElement':True,'NSAppleScriptEnabled':True,'OSAScriptingDefinition':'Fixture.sdef','NSPrincipalClass':'FixtureApplication','NSAppleEventsUsageDescription':'Process synthetic fixture AppleEvents only.'}
(bundle/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
subprocess.run(['clang','-O1','-fobjc-arc','-Wno-deprecated-declarations',str(root/'tests/native/Fixture.m'),'-framework','Foundation','-framework','AppKit','-framework','ScriptingBridge','-framework','Carbon','-o',str(mac/'fixture')],check=True)
names=['native/MessagesBridge.m','native/MessagesScripting.h','tests/native/Fixture.m','tests/native/Fixture.sdef'];manifest={'sources':{name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in names},'target':'com.nathanzhao.fixture.Messages','tcc_tested':False,'clang':subprocess.check_output(['clang','--version'],text=True)}
(resources/'build-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
subprocess.run(['codesign','--force','--sign','-',str(bundle)],check=True);print(bundle)
