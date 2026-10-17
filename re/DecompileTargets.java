// Headless, bounded native-code inspection. Does not launch the analyzed program.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public class DecompileTargets extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 2) throw new IllegalArgumentException("targets.tsv output-directory");
        Path output = Paths.get(args[1]);
        Files.createDirectories(output);
        List<String[]> targets = new ArrayList<>();
        List<String> rows = Files.readAllLines(Paths.get(args[0]), StandardCharsets.UTF_8);
        String expectedHash = rows.stream().filter(row -> row.startsWith("# sha256\t"))
            .map(row -> row.substring(9)).findFirst().orElseThrow(() -> new IllegalArgumentException("Missing binary hash"));
        if (!expectedHash.equalsIgnoreCase(currentProgram.getExecutableSHA256()))
            throw new IllegalStateException("Targets describe a different executable; import the matching binary first");
        for (String row : rows) {
            if (row.isBlank() || row.startsWith("#")) continue;
            String[] fields = row.split("\t", 4);
            if (fields.length != 4) throw new IllegalArgumentException("Expected start, end-exclusive, name, description");
            targets.add(fields);
            Address start = toAddr(fields[0]);
            Address end = toAddr(fields[1]).subtract(1);
            AddressSet body = new AddressSet(start, end);
            new DisassembleCommand(start, body, true).applyTo(currentProgram, monitor);
            Function function = getFunctionAt(start);
            if (function == null) function = createFunction(start, fields[2]);
            if (function == null) throw new IllegalStateException("Cannot define " + fields[0]);
            // LC_FUNCTION_STARTS supplies the physical extent. Keep all blocks, including
            // indirect Swift async continuations inside it, without whole-program analysis.
            function.setBody(body);
        }
        DecompInterface decompiler = new DecompInterface();
        int failures = 0;
        try {
            decompiler.setOptions(new DecompileOptions());
            if (!decompiler.openProgram(currentProgram)) throw new IllegalStateException(decompiler.getLastMessage());
            for (String[] fields : targets) {
                monitor.checkCancelled();
                Function function = getFunctionAt(toAddr(fields[0]));
                println("TARGET " + fields[0] + " " + fields[3]);
                DecompileResults result = decompiler.decompileFunction(function, 45, monitor);
                if (!result.decompileCompleted()) failures++;
                String header = "/* " + fields[3] + "\n * " + fields[2] + "\n * Address range: " + fields[0] + " .. " + fields[1] + "\n * Ghidra pseudocode; inferred types and calling conventions may be wrong.\n */\n";
                String body = result.decompileCompleted() ? result.getDecompiledFunction().getC()
                    : "/* DECOMPILATION FAILED: " + result.getErrorMessage() + " */\n";
                Files.writeString(output.resolve(fields[0] + ".c"), header + body, StandardCharsets.UTF_8);
                StringBuilder assembly = new StringBuilder(header);
                for (Instruction instruction : currentProgram.getListing().getInstructions(function.getBody(), true))
                    assembly.append(instruction.getAddress()).append(": ").append(instruction).append("\n");
                Files.writeString(output.resolve(fields[0] + ".asm"), assembly.toString(), StandardCharsets.UTF_8);
                println("RESULT " + fields[0] + " " + (result.decompileCompleted() ? "OK" : result.getErrorMessage()));
            }
        } finally {
            decompiler.dispose();
        }
        if (failures != 0) throw new IllegalStateException(failures + " targets failed to decompile");
        println("COMPLETE " + targets.size() + " targets for " + expectedHash);
    }
}
