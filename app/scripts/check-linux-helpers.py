#!/usr/bin/env python3
"""Refuse packaging stale compositor resources or an unbound capture protocol."""
import hashlib
import json
import re
from pathlib import Path


def check(root):
    provenance = json.loads((root / 'provenance.json').read_text())
    for name, expected in provenance['files'].items():
        path = root / name
        if path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError('helper resource differs from provenance: ' + name)
    for platform, name, version in [('gnome', 'gnome/winrects@cua/extension.js', 9),
                                    ('kwin', 'kwin/kwin_target_helper.cpp', 2)]:
        code = (root / name).read_text()
        match = re.search(r'GetCaptureVersion\(\)\s*\{\s*return (\d+);', code)
        if not match or int(match[1]) != version or provenance.get(platform + '_capture_protocol') != version:
            raise ValueError('unsupported capture protocol: ' + platform)
    code = (root / 'gnome/winrects@cua/extension.js').read_text()
    pointer = re.search(r'GetHistoryPointerVersion\(\)\s*\{\s*return (\d+);', code)
    if not pointer or int(pointer[1]) != 2 or provenance.get('gnome_history_pointer_protocol') != 2:
        raise ValueError('unsupported GNOME History pointer protocol')
    if 'gnome/winrects@cua/history-pointer.js' not in provenance['files']:
        raise ValueError('GNOME History pointer resource is not bound')
    if json.loads((root / 'gnome/winrects@cua/metadata.json').read_text())['version'] != 11:
        raise ValueError('GNOME extension version must be 11 for History pointer protocol 2')
    kwin = (root / 'kwin/kwin_target_helper.cpp').read_text()
    history = re.search(r'GetHistoryInputVersion\(\)\s*\{\s*return (\d+);', kwin)
    if not history or int(history[1]) != 1 or provenance.get('kwin_history_input_protocol') != 1:
        raise ValueError('unsupported KWin History input protocol')
    setup = re.search(r'GetHistorySetupVersion\(\)\s*\{\s*return (\d+);', kwin)
    if not setup or int(setup[1]) != 1 or provenance.get('kwin_history_setup_protocol') != 1:
        raise ValueError('unsupported KWin History setup protocol')


if __name__ == '__main__':
    check(Path(__file__).resolve().parents[1] / 'src-tauri/linux-helpers')
    print('Linux compositor resources and capture protocols verified')
