# Ghidra headless postScript: decompile functions whose name contains any given substring.
# Usage: -postScript DumpDecomp.py <substr1> <substr2> ...
import ghidra.app.decompiler as decomp
args = list(getScriptArgs())
targets = [a.lower() for a in args]
print("=====TARGET SUBSTRINGS: %s" % targets)
ifc = decomp.DecompInterface()
ifc.setOptions(decomp.DecompileOptions())
ifc.openProgram(currentProgram)
fm = currentProgram.getFunctionManager()
matched = 0
seen = set()
for f in fm.getFunctions(True):
    name = f.getName()
    ln = name.lower()
    hit = [t for t in targets if t in ln]
    if not hit:
        continue
    ep = str(f.getEntryPoint())
    if ep in seen:
        continue
    seen.add(ep)
    matched += 1
    print("\n=====FUNC===== [%s] %s @ %s" % (",".join(hit), name, ep))
    try:
        r = ifc.decompileFunction(f, 120, monitor)
        if r and r.decompileCompleted():
            print(r.getDecompiledFunction().getC())
        else:
            print("  [decompile failed: %s]" % (r.getErrorMessage() if r else "no result"))
    except Exception as e:
        print("  [exception: %s]" % e)
print("\n=====TOTAL MATCHED %d=====" % matched)
