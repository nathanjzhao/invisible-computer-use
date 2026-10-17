import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.symbol.Symbol;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;

public class DumpWitnessTables extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        StringBuilder out = new StringBuilder("table\toffset\taddress\tsymbols\n");
        for (int i = 1; i < args.length; i++) {
            Address table = toAddr(args[i]);
            for (int offset = 0; offset < 0x100; offset += 8) {
                Address target = toAddr(currentProgram.getMemory().getLong(table.add(offset)));
                StringBuilder names = new StringBuilder();
                for (Symbol symbol : currentProgram.getSymbolTable().getSymbols(target)) {
                    if (names.length() > 0) names.append("|");
                    names.append(symbol.getName());
                }
                out.append(table).append("\t").append(Integer.toHexString(offset)).append("\t")
                    .append(target).append("\t").append(names).append("\n");
            }
        }
        Files.writeString(Paths.get(args[0]), out.toString(), StandardCharsets.UTF_8);
        println("DUMPED " + (args.length - 1) + " tables from " + currentProgram.getExecutableSHA256());
    }
}
