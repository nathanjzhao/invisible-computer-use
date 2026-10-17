#!/usr/bin/env python3
"""Inventory the installed computer-use code without launching apps or reading user state.

Records artifact hashes, package versions, native imports/symbols, and byte drift
from the archived Sky JS. Symbol presence is evidence of structure, not behavior.
Uses only the standard library; no decompiler, login, or native service is needed.
"""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import re
import struct

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def export_names(trie, addresses=None):
    """Defined Swift symbols in this build live in LC_DYLD_EXPORTS_TRIE, not LC_SYMTAB."""
    def uleb(offset):
        value, shift = 0, 0
        while shift < 64:
            byte = trie[offset]
            offset += 1
            value |= (byte & 0x7F) << shift
            if byte < 0x80:
                return value, offset
            shift += 7
        raise ValueError("invalid export-trie integer")

    pending = [(0, "", frozenset())]
    while pending:
        offset, prefix, ancestors = pending.pop()
        if offset in ancestors:
            raise ValueError("cycle in export trie")
        ancestors = ancestors | {offset}
        terminal_size, cursor = uleb(offset)
        if terminal_size:
            flags, payload = uleb(cursor)
            # Regular image-relative exports only; reexports and absolute/thread-local
            # symbols have different address semantics.
            if addresses is not None and flags & 0xB == 0:
                addresses[prefix], _ = uleb(payload)
            yield prefix
        cursor += terminal_size
        children = trie[cursor]
        cursor += 1
        for _ in range(children):
            end = trie.index(0, cursor)
            edge = trie[cursor:end].decode("utf-8")
            child, cursor = uleb(end + 1)
            pending.append((child, prefix + edge, ancestors))


def macho_inventory(path):
    data = path.read_bytes()
    if data[:4] != b"\xcf\xfa\xed\xfe":
        return {"format": "not a thin little-endian 64-bit Mach-O"}
    groups = {key: [] for key in (
        "UIElementRenderTree", "UIElementRenderDifference", "RefetchableSkyshotAXTree",
        "waitForUIToSettle", "SystemFrontmostApplicationTracker", "ComputerUseIPC",
    )}
    imports, symtab, exports, text_vmaddr, offset = [], None, None, None, 32
    for _ in range(struct.unpack_from("<I", data, 16)[0]):
        cmd, size = struct.unpack_from("<II", data, offset)
        if size < 8 or offset + size > len(data):
            raise ValueError("invalid Mach-O load command")
        if cmd == 2:
            symtab = struct.unpack_from("<IIII", data, offset + 8)
        if cmd == 0x19 and data[offset + 8:offset + 24].split(b"\0")[0] == b"__TEXT":
            text_vmaddr = struct.unpack_from("<Q", data, offset + 24)[0]
        if cmd == 0x80000033:
            exports = struct.unpack_from("<II", data, offset + 8)
        if cmd in (0x22, 0x80000022):
            exports = struct.unpack_from("<II", data, offset + 40)
        if cmd in (0xC, 0x80000018, 0x8000001F):
            start = offset + struct.unpack_from("<I", data, offset + 8)[0]
            imports.append(data[start:offset + size].split(b"\0", 1)[0].decode())
        offset += size
    symbol_count, names = 0, set()
    if symtab:
        symoff, symbol_count, stroff, strsize = symtab
        strings = data[stroff:stroff + strsize]
        for index in range(symbol_count):
            start = struct.unpack_from("<I", data, symoff + 16 * index)[0]
            end = strings.find(b"\0", start)
            symbol = strings[start:end].decode("utf-8", errors="replace")
            names.add(symbol)
    exported, addresses = set(), {}
    if exports and exports[1]:
        start, size = exports
        exported = set(export_names(data[start:start + size], addresses))
        names.update(exported)
    for symbol in names:
        for key, matches in groups.items():
            if key in symbol:
                matches.append(symbol)
    return {
        "format": "Mach-O 64-bit", "symtab_entries": symbol_count,
        "exported_symbols": len(exported), "unique_symbols": len(names),
        "selected_export_addresses": {name: hex(text_vmaddr + address) for name, address in sorted(addresses.items())
                                      if text_vmaddr is not None and "NextAvailableElementIDIteratorV4next" in name},
        "imports": sorted(imports),
        "symbol_groups": {key: {"count": len(values), "examples": sorted(values, key=lambda value: (len(value), value))[:4]}
                          for key, values in groups.items()},
        "limit": "Names confirm compiled structure; they do not establish runtime behavior or feature-flag values.",
    }


def inventory(app, reference):
    contents = app / "Contents"
    plist = plistlib.loads((contents / "Info.plist").read_bytes())
    packages = contents / "Resources/cua_node/lib/node_modules/@oai"
    result = {
        "app_version": plist.get("CFBundleShortVersionString"),
        "app_build": plist.get("CFBundleVersion"),
        "packages": {},
        "reference": "reference/openai-sky/sky_js_src (@oai/sky 0.6.26)",
    }
    for name in ("sky", "cua", "browser-desktop"):
        root = packages / name
        manifest = root / "package.json"
        if not manifest.is_file():
            result["packages"][name] = {"available": False}
            continue
        info = {"version": json.loads(manifest.read_text())["version"],
                "package_sha256": digest(manifest)}
        src = root / "dist/project/cua/sky_js/src"
        if src.is_dir():
            files = sorted(p for p in src.rglob("*") if p.is_file())
            paths = {str(p.relative_to(src)) for p in files}
            info["sky_js_files"] = {str(p.relative_to(src)): digest(p) for p in files}
            drift = {"same": [], "changed": [], "added": [], "removed": []}
            for path, sha in info["sky_js_files"].items():
                old = reference / path
                key = "added" if not old.is_file() else "same" if digest(old) == sha else "changed"
                drift[key].append(path)
            drift["removed"] = sorted(str(p.relative_to(reference)) for p in reference.rglob("*")
                                      if p.is_file() and str(p.relative_to(reference)) not in paths)
            info["byte_drift"] = drift
            client = src / "targets/mac/client.js"
            info["mac_api_versions"] = sorted(set(re.findall(r"CodexComputerUseIPC-\d+", client.read_text())))
        if name == "cua":
            facade = root / "dist/lib/js/oai_js_cua/src"
            info["facade_files"] = {str(p.relative_to(facade)): digest(p)
                                    for p in sorted(facade.rglob("*")) if p.is_file()}
        # A declared npm bin does not establish that an executable is in this app.
        info["linux_binaries_present"] = [str(p.relative_to(root)) for p in sorted((root / "bin/linux").glob("*")) if p.is_file()]
        result["packages"][name] = info
    native = packages / "sky/Codex Computer Use.app/Contents/MacOS/SkyComputerUseService"
    result["native_service"] = {"path_within_app": str(native.relative_to(app)),
                                "sha256": digest(native), "bytes": native.stat().st_size,
                                **macho_inventory(native)}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=Path("/Applications/ChatGPT.app"))
    parser.add_argument("--reference", type=Path, default=ROOT / "reference/openai-sky/sky_js_src")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = inventory(args.app, args.reference)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + "\n")
        summary = {"app": result["app_version"], "native_sha256": result["native_service"]["sha256"],
                   "packages": {name: {"version": info.get("version"),
                                       "byte_drift": {key: len(paths) for key, paths in info.get("byte_drift", {}).items()}}
                                for name, info in result["packages"].items()}, "output": str(args.output)}
        print(json.dumps(summary, indent=2))
    else:
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
