import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.util.*;

public class DumpDecomp extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        List<String> targets = new ArrayList<>();
        for (String a : args) targets.add(a.toLowerCase());
        println("=====TARGET SUBSTRINGS: " + targets);
        DecompInterface ifc = new DecompInterface();
        ifc.setOptions(new DecompileOptions());
        ifc.openProgram(currentProgram);
        FunctionManager fm = currentProgram.getFunctionManager();
        int matched = 0;
        Set<String> seen = new HashSet<>();
        for (Function f : fm.getFunctions(true)) {
            String ln = f.getName().toLowerCase();
            List<String> hit = new ArrayList<>();
            for (String t : targets) if (ln.contains(t)) hit.add(t);
            if (hit.isEmpty()) continue;
            String ep = f.getEntryPoint().toString();
            if (!seen.add(ep)) continue;
            matched++;
            println("\n=====FUNC===== " + hit + " :: " + f.getName() + " @ " + ep);
            try {
                DecompileResults r = ifc.decompileFunction(f, 120, monitor);
                if (r != null && r.decompileCompleted())
                    println(r.getDecompiledFunction().getC());
                else
                    println("  [decompile failed: " + (r != null ? r.getErrorMessage() : "no result") + "]");
            } catch (Exception e) {
                println("  [exception: " + e + "]");
            }
        }
        println("\n=====TOTAL MATCHED " + matched + "=====");
    }
}
