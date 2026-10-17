"""Packaging snapshot invariants; no application is launched by these tests."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import subprocess

spec=importlib.util.spec_from_file_location('prepare',Path(__file__).with_name('prepare-e2e-macos.py'))
prepare=importlib.util.module_from_spec(spec);spec.loader.exec_module(prepare)

class SnapshotTests(unittest.TestCase):
    def test_snapshot_is_independent_and_tracks_dependency_changes(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input';source.mkdir();(source/'src').mkdir()
            (source/'src/lib.rs').write_text('pub fn original() {}')
            (source/'Cargo.toml').write_text('[package]\nname="fixture"\nversion="0.1.0"\n')
            (source/'entitlements.mac.plist').write_text('<dict/>')
            prepare.copy_source(source,root/'frozen');before=prepare.source_hashes(root/'frozen')
            (source/'src/lib.rs').write_text('pub fn changed() {}')
            self.assertEqual(before,prepare.source_hashes(root/'frozen'))
            self.assertIn('entitlements.mac.plist',before)
            (root/'frozen/src/lib.rs').write_text('pub fn changed() {}')
            self.assertNotEqual(before,prepare.source_hashes(root/'frozen'))

    def test_build_caches_and_environment_files_do_not_enter_snapshot(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input';source.mkdir();(source/'Cargo.toml').write_text('source')
            for name in ['target','node_modules','dist','gen']:
                (source/name).mkdir();(source/name/'generated').write_text('cache')
            (source/'.env.local').write_text('private fixture placeholder')
            prepare.copy_source(source,root/'frozen')
            self.assertEqual(set(prepare.source_hashes(root/'frozen')),{'Cargo.toml'})

    def test_mutable_source_links_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input';source.mkdir();other=root/'elsewhere';other.write_text('mutable')
            (source/'lib.rs').symlink_to(other)
            with self.assertRaisesRegex(RuntimeError,'symlink'):prepare.copy_source(source,root/'frozen')

    def test_ignored_credentials_stay_out_but_new_source_is_kept(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input';source.mkdir()
            subprocess.run(['git','init','-q',str(source)],check=True)
            (source/'.gitignore').write_text('.adminpw\nprivate-cache/\n*.local\ntracked.rs\nkept/\n')
            (source/'.adminpw').write_text('synthetic credential canary')
            (source/'custom.local').write_text('synthetic ignored data')
            (source/'private-cache').mkdir();(source/'private-cache/secret').write_text('synthetic ignored data')
            (source/'new.rs').write_text('pub fn new_source() {}')
            (source/'tracked.rs').write_text('pub fn tracked_source() {}')
            (source/'kept').mkdir();(source/'kept/code.rs').write_text('pub fn tracked_child() {}')
            (source/'kept/private.local').write_text('synthetic ignored data')
            subprocess.run(['git','-C',str(source),'add','-f','tracked.rs','kept/code.rs'],check=True)
            prepare.copy_source(source,root/'frozen')
            self.assertEqual(set(prepare.source_hashes(root/'frozen')), {'.gitignore','new.rs','tracked.rs','kept/code.rs'})
            self.assertFalse((root/'frozen/.adminpw').exists())
            self.assertFalse((root/'frozen/private-cache').exists())

    def test_private_file_names_are_excluded_without_git_metadata(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input';source.mkdir()
            (source/'source.rs').write_text('source')
            for name in prepare.PRIVATE_NAMES:(source/name).write_text('synthetic credential canary')
            prepare.copy_source(source,root/'frozen')
            self.assertEqual(set(prepare.source_hashes(root/'frozen')), {'source.rs'})

if __name__=='__main__':unittest.main()
