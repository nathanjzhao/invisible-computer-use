import ghidra.app.script.GhidraScript;
public class SetOpts extends GhidraScript {
    public void run() throws Exception {
        String[] off = {
            "Decompiler Switch Analysis",
            "Decompiler Parameter ID",
            "Call Convention ID",
            "Call Convention Identification",
            "Aggressive Instruction Finder"
        };
        for (String o : off) {
            try { setAnalysisOption(currentProgram, o, "false"); } catch (Exception e) {}
        }
        println("=====PRESCRIPT: disabled slow decompiler analyzers");
    }
}
