"""Build the embedded Qt5 compatibility library for the native Linux target."""
from pathlib import Path
import os,shlex,struct,subprocess,sys
root=Path(__file__).resolve().parent;target=os.environ['TARGET'];host=os.environ['HOST']
if target!=host and not os.environ.get('CXX'):raise SystemExit('Set a target CXX and target pkg-config environment for cross compilation')
flags=shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','Qt5DBus','Qt5Gui'],text=True))
out=Path(sys.argv[1]);compiler=shlex.split(os.environ.get('CXX','c++'))
# QtGui must also be global when the target is an interpreter that loads Qt
# extensions locally. Otherwise its private activation symbol bypasses the shim.
subprocess.run(compiler+['-std=c++17','-O2','-shared','-fPIC','-Wl,-z,relro,-z,now','-Wl,--version-script='+str(root/'exports.map'),str(root/'bridge.cpp'),'-o',str(out),'-Wl,--no-as-needed',*flags,'-Wl,--as-needed','-ldl'],check=True)
data=out.read_bytes();assert data[:4]==b'\x7fELF'
expected={'x86_64':62,'aarch64':183}.get(target.split('-')[0]);assert expected is not None and struct.unpack('<H' if data[5]==1 else '>H',data[18:20])[0]==expected,'Compatibility library architecture differs from the app'
