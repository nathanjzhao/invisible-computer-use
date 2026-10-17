"""Packaging must bind the new pointer resource without weakening protocol pins."""
import hashlib,importlib.util,json,shutil,tempfile,unittest
from pathlib import Path
HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('helper_check',HERE/'check-linux-helpers.py')
check=importlib.util.module_from_spec(spec);spec.loader.exec_module(check)

class Tests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)/'helpers'
        shutil.copytree(HERE.parent/'src-tauri/linux-helpers',self.root)
    def tearDown(self):self.temp.cleanup()
    def manifest(self):return json.loads((self.root/'provenance.json').read_text())
    def save(self,value):(self.root/'provenance.json').write_text(json.dumps(value))
    def test_current_resources_pass(self):check.check(self.root)
    def test_omitted_pointer_resource_is_not_allowed(self):
        data=self.manifest();del data['files']['gnome/winrects@cua/history-pointer.js'];self.save(data)
        with self.assertRaisesRegex(ValueError,'not bound'):check.check(self.root)
    def test_changed_pointer_protocol_is_rejected_even_with_a_matching_hash(self):
        name='gnome/winrects@cua/extension.js';p=self.root/name
        p.write_text(p.read_text().replace('GetHistoryPointerVersion() { return 2; }','GetHistoryPointerVersion() { return 3; }'))
        data=self.manifest();data['files'][name]=hashlib.sha256(p.read_bytes()).hexdigest();self.save(data)
        with self.assertRaisesRegex(ValueError,'pointer protocol'):check.check(self.root)
    def test_old_metadata_is_not_accepted_with_new_code(self):
        name='gnome/winrects@cua/metadata.json';p=self.root/name;value=json.loads(p.read_text());value['version']=9;p.write_text(json.dumps(value))
        data=self.manifest();data['files'][name]=hashlib.sha256(p.read_bytes()).hexdigest();self.save(data)
        with self.assertRaisesRegex(ValueError,'version must be 11'):check.check(self.root)
    def test_changed_kwin_input_protocol_is_rejected_even_with_a_matching_hash(self):
        name='kwin/kwin_target_helper.cpp';p=self.root/name
        p.write_text(p.read_text().replace('GetHistoryInputVersion() { return 1; }','GetHistoryInputVersion() { return 2; }'))
        data=self.manifest();data['files'][name]=hashlib.sha256(p.read_bytes()).hexdigest();self.save(data)
        with self.assertRaisesRegex(ValueError,'KWin History input protocol'):check.check(self.root)
    def test_changed_kwin_setup_protocol_is_rejected_even_with_a_matching_hash(self):
        name='kwin/kwin_target_helper.cpp';p=self.root/name
        p.write_text(p.read_text().replace('GetHistorySetupVersion() { return 1; }','GetHistorySetupVersion() { return 2; }'))
        data=self.manifest();data['files'][name]=hashlib.sha256(p.read_bytes()).hexdigest();self.save(data)
        with self.assertRaisesRegex(ValueError,'KWin History setup protocol'):check.check(self.root)

if __name__=='__main__':unittest.main()
