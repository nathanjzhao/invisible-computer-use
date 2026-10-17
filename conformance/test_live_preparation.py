"""Prove live acceptance refuses stale binaries before touching any desktop surface."""
import importlib.util, json
from pathlib import Path
import pytest

ROOT=Path(__file__).resolve().parents[1]

@pytest.mark.parametrize('change', ['missing','source','binary'])
def test_stale_live_preparation_never_reaches_desktop(tmp_path,monkeypatch,change):
    spec=importlib.util.spec_from_file_location('audit_live',ROOT/'impl/native/macos-drive/audit-live.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    (tmp_path/'study/audit/fixes').mkdir(parents=True)
    build=tmp_path/'prepared';build.mkdir()
    binaries={name:tmp_path/name for name in ('DRIVER','ICU','EXE','PROBE')}
    for key,path in binaries.items():path.write_text('fixture');monkeypatch.setattr(module,key,path)
    monkeypatch.setattr(module,'ROOT',tmp_path);monkeypatch.setattr(module,'BUILD',build)
    monkeypatch.setenv('ICU_BOUNDED_TEST','1')
    expected={'source_sha256':{'source.rs':'old'},'binaries':{'fixture':'old'},'vendor_source_sha256':{'native.rs':'same'},'vendor_revision':'base','vendor_diff_sha256':'patch'}
    if change!='missing':(build/'build-manifest.json').write_text(json.dumps(expected))
    current=json.loads(json.dumps(expected))
    if change=='source':current['source_sha256']['source.rs']='new'
    else:current['binaries']['fixture']='new'
    monkeypatch.setattr(module,'current_build_manifest',lambda:current,raising=False)
    class Result:stdout='{}';stderr='';returncode=0
    monkeypatch.setattr(module,'run',lambda *a,**kw:Result())
    touched=[]
    def forbidden():
        touched.append(True)
        raise AssertionError('desktop probe reached with stale/unprepared executables')
    monkeypatch.setattr(module,'snapshot',forbidden)
    with pytest.raises(RuntimeError,match='prepar|stale'):
        module.live()
    assert not touched


def load_live():
    spec=importlib.util.spec_from_file_location('audit_live',ROOT/'impl/native/macos-drive/audit-live.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    return module


def test_owned_startup_restores_original_baseline_without_moving_pointer():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1.75,'y':2.25},'windowserver_front_psn':[1]}
    driver={'front_pid':20,'pointer':before['pointer'],'windowserver_front_psn':[2]}
    reads=iter([driver,before]); restores=[]
    report=live.restore_owned_startup(before,{20},lambda:next(reads),restores.append)
    assert restores==[10] and report['restored']==before and report['observed']==driver


def test_startup_refuses_unowned_focus_or_pointer_changes():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1.75,'y':2.25},'windowserver_front_psn':[1]}
    for observed in [dict(before,front_pid=30),dict(before,pointer={'x':8,'y':9})]:
        with pytest.raises(RuntimeError,match='refusing further input'):
            live.restore_owned_startup(before,{20},lambda:observed,lambda _:pytest.fail('must not fight user'))


def test_startup_keeps_exact_ownership_assertions_after_restore():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1.75,'y':2.25},'windowserver_front_psn':[1]}
    driver=dict(before,front_pid=20,windowserver_front_psn=[2])
    with pytest.raises(RuntimeError,match='original input ownership'):
        live.restore_owned_startup(before,{20},lambda:driver,lambda _:None)


def test_user_intervention_during_focus_restore_is_preserved():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1.75,'y':2.25},'windowserver_front_psn':[1]}
    driver=dict(before,front_pid=20,windowserver_front_psn=[2])
    for after in [dict(before,front_pid=30),dict(before,pointer={'x':8,'y':9})]:
        reads=iter([driver,after])
        with pytest.raises(live.UserIntervened):
            live.restore_owned_startup(before,{20},lambda:next(reads),lambda _:None)


def test_user_intervention_during_ax_readiness_prevents_dispatch():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1.75,'y':2.25},'windowserver_front_psn':[1]}
    after=dict(before,pointer={'x':8,'y':9})
    reads=iter([before,after]); observations=[]
    def observe(tag):
        observations.append(tag)
        return {'elements':[{'identifier':'audit-play-control','element_token':'s1:0'}]}
    with pytest.raises(live.UserIntervened):
        live.wait_for_actionable_fixture(before,{20},observe,lambda:next(reads),lambda _:None)
    assert len(observations)==1


def test_observation_failure_still_classifies_setup_intervention():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1,'y':2},'windowserver_front_psn':[1]}
    changed=dict(before,pointer={'x':8,'y':9});reads=iter([before,changed])
    def fail(_): raise ValueError('observation failed')
    with pytest.raises(live.UserIntervened):
        live.wait_for_actionable_fixture(before,{20},fail,lambda:next(reads),lambda _:None)


def test_no_unchecked_final_readiness_pause():
    live=load_live()
    before={'front_pid':10,'pointer':{'x':1,'y':2},'windowserver_front_psn':[1]}
    pauses=[]
    with pytest.raises(RuntimeError,match='bounded readiness wait'):
        live.wait_for_actionable_fixture(before,{20},lambda _: {'elements':[]},lambda:before,pauses.append)
    assert len(pauses)==7


def test_state_probe_does_not_launch_the_fixture_bundle(monkeypatch):
    live=load_live()
    calls=[]
    class Result: returncode=0;stdout='{}';stderr=''
    monkeypatch.setattr(live,'run',lambda argv,**kw:(calls.append(argv) or Result()))
    live.snapshot()
    assert Path(calls[0][0]) != live.EXE, 'short-lived probe impersonates the target app bundle'
    assert '.app/Contents/MacOS/' not in str(calls[0][0])


@pytest.mark.parametrize('kinds', [
    ['middle_button_down','middle_button_down','middle_button_up','middle_button_up'],
    ['middle_button_down'], ['middle_button_up','middle_button_down']])
def test_middle_guard_rejects_duplicate_incomplete_or_reversed_pair(kinds):
    with pytest.raises(AssertionError):
        load_live().require_middle_pair([{'kind':k} for k in kinds])


def test_middle_guard_accepts_exact_ordered_pair():
    load_live().require_middle_pair([{'kind':'middle_button_down'},{'kind':'middle_button_up'}])
