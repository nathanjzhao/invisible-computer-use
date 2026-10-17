#!/usr/bin/env python3
"""Prepare bounded Ghidra targets from current Mach-O exports and function starts.

Reads a local binary and invokes only the Swift name demangler. The analyzed
program is never executed. Add --address for internal callees found in a pass.
"""
import argparse
import bisect
import hashlib
import json
from pathlib import Path
import struct
import subprocess

from inspect_codex_runtime import export_names

DEFAULT_BINARY = Path("/Applications/ChatGPT.app/Contents/Resources/cua_node/lib/node_modules/@oai/sky/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService")
DEFAULT_PREFIXES = (
    "ComputerUse.RefetchableSkyshotAXTree.refetch",
    "ComputerUse.RefetchableUIElement.refetchIfNeeded",
    "ComputerUse.RefetchableUIElement.setValue",
    "AccessibilitySupport.FrequencyTable.isUnique",
    "AccessibilitySupport.UIElementTreeInvalidationMonitor.init(",
)


def native_tables(binary):
    data = binary.read_bytes()
    if data[:4] != b"\xcf\xfa\xed\xfe":
        raise ValueError("Expected a thin little-endian Mach-O 64-bit binary")
    offset, base, exports, starts = 32, None, None, None
    for _ in range(struct.unpack_from("<I", data, 16)[0]):
        command, size = struct.unpack_from("<II", data, offset)
        if size < 8 or offset + size > len(data):
            raise ValueError("Invalid load command")
        if command == 0x19 and data[offset + 8:offset + 24].split(b"\0")[0] == b"__TEXT":
            base = struct.unpack_from("<Q", data, offset + 24)[0]
        if command in (0x80000033, 0x26):
            start, length = struct.unpack_from("<II", data, offset + 8)
            if command == 0x26:
                starts = data[start:start + length]
            else:
                exports = data[start:start + length]
        offset += size
    if base is None or starts is None or exports is None:
        raise ValueError("Required native tables are absent")
    relative = {}
    list(export_names(exports, relative))
    addresses = {name: base + offset for name, offset in relative.items()}
    boundaries, current, offset = [], base, 0
    while offset < len(starts):
        delta, shift = 0, 0
        while True:
            byte = starts[offset]
            offset += 1
            delta |= (byte & 127) << shift
            if byte < 128:
                break
            shift += 7
            if shift >= 64:
                raise ValueError("Invalid function-start delta")
        if delta == 0:
            break
        current += delta
        boundaries.append(current)
    return addresses, boundaries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prefix", action="append", help="Demangled symbol prefix; defaults to the refetch/uniqueness paths")
    parser.add_argument("--address", action="append", default=[], help="Additional internal function start, e.g. 0x100123400")
    args = parser.parse_args()
    addresses, boundaries = native_tables(args.binary)
    names = sorted(addresses)
    result = subprocess.run(["xcrun", "swift-demangle", "--compact"],
                            input="\n".join(names) + "\n", text=True, capture_output=True, timeout=45, check=True)
    prefixes = tuple(args.prefix or DEFAULT_PREFIXES)
    selected = {addresses[name]: {"name": name, "description": description}
                for name, description in zip(names, result.stdout.splitlines(), strict=True)
                if description.startswith(prefixes)}
    for text in args.address:
        address = int(text, 0)
        selected.setdefault(address, {"name": "internal_" + format(address, "x"),
                                      "description": "Internal callee; behavior must be inferred from code"})
    rows = []
    for address, info in sorted(selected.items()):
        index = bisect.bisect_left(boundaries, address)
        if index + 1 >= len(boundaries) or boundaries[index] != address:
            raise ValueError(f"No complete LC_FUNCTION_STARTS extent for {address:#x}: {info}")
        rows.append({"address": hex(address), "end": hex(boundaries[index + 1]), **info})
    if not rows:
        raise ValueError("No matching functions")
    args.output.mkdir(parents=True, exist_ok=True)
    with args.binary.open("rb") as stream:
        sha = hashlib.file_digest(stream, "sha256").hexdigest()
    manifest = {"binary": str(args.binary), "sha256": sha, "boundary_source": "LC_FUNCTION_STARTS",
                "symbol_source": "LC_DYLD_EXPORTS_TRIE + swift-demangle", "functions": rows}
    (args.output / "targets.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (args.output / "targets.tsv").write_text("# sha256\t" + sha + "\n# start\tend-exclusive\tmangled-symbol\tdemangled-symbol\n" +
        "".join("\t".join(row[key] for key in ("address", "end", "name", "description")) + "\n" for row in rows))
    print(json.dumps({"binary_sha256": sha, "functions": len(rows), "output": str(args.output)}))


if __name__ == "__main__":
    main()
