"""Offline guards for the real Cocoa monitor and strict live-result checks."""
import importlib.util, json, subprocess
from pathlib import Path
import pytest
ROOT=Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('audit_live',ROOT/'impl/native/macos-drive/audit-live.py')
live=importlib.util.module_from_spec(spec);spec.loader.exec_module(live)


def test_actual_monitor_handles_scroll_without_mouse_only_accessors():
    assert live.PROBE.is_file(), 'Required freshly prepared Cocoa observer is missing'
    p=subprocess.run([str(live.PROBE),'inspect-wheel'],capture_output=True,text=True,timeout=5)
    assert p.returncode==0,p.stderr
    d=json.loads(p.stdout)
    assert d['type']==22 and d['delta_y']==-1
    assert 'click_count' not in d and 'button' not in d


@pytest.mark.parametrize('damage', ['missing','wrong_window','duplicate_delta','no_effect'])
def test_scroll_guard_requires_target_delta_and_effect(damage):
    before={'window_id':1,'scroll_y':0};after={'window_id':1,'scroll_y':20}
    events=[{'kind':'input_event','type':22,'window_id':1,'delta_y':-1} for _ in range(2)]
    if damage=='missing':events=[]
    elif damage=='wrong_window':events[0]['window_id']=2
    elif damage=='duplicate_delta':
        for e in events:e['delta_y']=-2
    else:after['scroll_y']=0
    with pytest.raises(AssertionError):live.require_scroll_effect(before,after,events,1,2)


def test_scroll_guard_accepts_exact_receipt_and_movement():
    live.require_scroll_effect({'window_id':1,'scroll_y':0},{'window_id':1,'scroll_y':20},
        [{'kind':'input_event','type':22,'window_id':1,'delta_y':-1} for _ in range(2)],1,2)


@pytest.mark.parametrize('states', [[1,1],[1,2,2]])
def test_click_receiver_rejects_replay(states):
    events=[e for state in states for e in [{'kind':'left_down','click_count':state},{'kind':'text_mouse_down_returned'}]]
    with pytest.raises(AssertionError): live.require_click_sequence(events,max(states))


def test_menu_receiver_rejects_reopening():
    events=[{'kind':k} for k in ['right_down','menu_closed','right_down','menu_closed']]
    with pytest.raises(AssertionError): live.require_menu_sequence(events)


@pytest.mark.parametrize('count',[1,2])
def test_click_receiver_accepts_exact_completed_pairs(count):
    events=[e for state in range(1,count+1) for e in [{'kind':'left_down','click_count':state},{'kind':'text_mouse_down_returned'}]]
    live.require_click_sequence(events,count)
    with pytest.raises(AssertionError):live.require_click_sequence(events[:-1],count)


def test_menu_receiver_accepts_one_completed_menu():
    live.require_menu_sequence([{'kind':'right_down'},{'kind':'menu_closed'}])
    with pytest.raises(AssertionError):live.require_menu_sequence([{'kind':'right_down'}])

@pytest.mark.parametrize('current_counts',[[1],None])
def test_keyboard_or_unreadable_input_preserves_owner_even_without_pointer_change(current_counts):
    base={'front_pid':1,'pointer':{'x':2,'y':3},'windowserver_front_psn':[1],'hid_input_counts':[0]}
    now=dict(base,hid_input_counts=current_counts)
    report=live.ownership_report(base,{2},now)
    assert report and report['classification']=='ownership_uncertain'
