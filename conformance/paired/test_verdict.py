import copy
import pytest
from .verdict import compare


def pair():
    report = {'engine': 'openai-native', 'executed': True, 'cleanup_verified': True, 'engine_sha256': 'a'*64,
              'harness_sha256': 'b'*64, 'fixture_sha256': 'c'*64, 'os': 'macos', 'display': [1920,1200,1],
              'mode': 'scripted-native', 'cases': [{'id': 'calc', 'executed': True,
              'observations': ['before','after'], 'actions': ['6','×','7','='], 'outcome': {'display': '42'}}]}
    candidate = copy.deepcopy(report); candidate['engine'] = 'icu-native'
    contract = {'mode':'scripted-native', 'fixture_sha256':'c'*64, 'cases':{'calc':{'expected':{'display':'42'}}}}
    return report, candidate, contract


def test_actual_matching_outcomes():
    assert compare(*pair())['passed']


@pytest.mark.parametrize('change', ['wrong_engine','not_executed','missing_case','duplicate_case','wrong_fixture','wrong_result','both_wrong','no_actions','cleanup','mode'])
def test_false_parity_is_rejected(change):
    a,b,c=pair()
    if change=='wrong_engine': a['engine']='openai-client-on-our-driver'
    if change=='not_executed': a['executed']=False
    if change=='missing_case': a['cases']=[]
    if change=='duplicate_case': a['cases']*=2
    if change=='wrong_fixture': b['fixture_sha256']='d'*64
    if change=='wrong_result': b['cases'][0]['outcome']['display']='0'
    if change=='both_wrong': a['cases'][0]['outcome']['display']=b['cases'][0]['outcome']['display']='0'
    if change=='no_actions': a['cases'][0]['actions']=[]
    if change=='cleanup': b['cleanup_verified']=False
    if change=='mode': a['mode']='model-driven'
    assert not compare(a,b,c)['passed']


def test_ownership_must_be_measured_when_required_and_differences_are_reported():
    a,b,c=pair();c['ownership_required']=True
    assert not compare(a,b,c)['passed']
    a['cases'][0]['ownership']={'sample_count':50,'guard_text':'guard','violations':40,'hid_resets':30}
    b['cases'][0]['ownership']={'sample_count':50,'guard_text':'guard','violations':0,'hid_resets':0}
    result=compare(a,b,c)
    assert result['passed'] and result['ownership_differences']==['calc']
    b['cases'][0]['ownership']['guard_text']='unexpected input'
    assert not compare(a,b,c)['passed']


def test_runner_check_only_never_counts_as_live_acceptance(tmp_path):
    import subprocess,sys,json
    from pathlib import Path
    artifact=tmp_path/'binary';artifact.write_bytes(b'prepared-only')
    harness=tmp_path/'host';harness.mkdir();(harness/'rpc.py').write_text('# prepared only\n')
    command=[sys.executable,str(Path(__file__).with_name('run_native_suite.py')),'--platform','macos','--work',str(tmp_path/'work'),
             '--fixture',str(artifact),'--candidate-driver',str(artifact),'--reference-executable',str(artifact),
             '--reference-cli',str(artifact),'--guard',str(artifact),'--ui',str(artifact),'--host-harness',str(harness),'--check-only']
    result=subprocess.run(command,capture_output=True,text=True)
    report=json.loads(result.stdout)
    assert result.returncode==0 and report['status']=='prepared_not_executed' and not report['executed'] and not report['passed']
