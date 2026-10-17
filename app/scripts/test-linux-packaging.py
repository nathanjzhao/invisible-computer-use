#!/usr/bin/env python3
"""Release contract for the Linux audio executable and its ELF dependencies."""
import json
import unittest
from pathlib import Path

CONFIG = Path(__file__).resolve().parents[1] / 'src-tauri/tauri.linux.conf.json'


class LinuxAudioPackaging(unittest.TestCase):
    def test_every_attested_helper_resource_has_both_native_package_mappings(self):
        bundle=json.loads(CONFIG.read_text())['bundle']
        provenance=json.loads((CONFIG.parent/'linux-helpers/provenance.json').read_text())
        for format in ['deb','rpm']:
            for relative in ['provenance.json',*provenance['files']]:
                self.assertEqual(bundle['linux'][format]['files'].get('/usr/share/copilot/linux-helpers/'+relative),'linux-helpers/'+relative)

    def test_observer_and_runtime_dependencies(self):
        bundle = json.loads(CONFIG.read_text())['bundle']
        self.assertIn('binaries/taiku-observer', bundle.get('externalBin', []))
        self.assertIn('binaries/cua-driver', bundle['externalBin'])
        for package in ['libpulse0', 'libudev1', 'libx11-6', 'libxtst6', 'libstdc++6', 'libgcc-s1']:
            self.assertIn(package, bundle['linux']['deb']['depends'])
        for library in ['libpulse.so.0', 'libudev.so.1', 'libX11.so.6', 'libXtst.so.6', 'libstdc++.so.6', 'libgcc_s.so.1']:
            self.assertIn(library + '()(64bit)', bundle['linux']['rpm']['depends'])


if __name__ == '__main__':
    unittest.main()
