import copy
import json
import pytest
from .product_verdict import verify


def fixture(tmp_path, change=None):
    reference=tmp_path/'reference';candidate=tmp_path/'candidate'
    reference.mkdir();candidate.mkdir()
    a={'engine':'openai-native','mode':'model-driven-product','passed':True,'cleanup_verified':True,'fixture_sha256':'f'*64,'cases':[]}
    b={**a,'engine':'icu-packaged-app','model':'test-model','cases':[]}
    for key in ['calculator','text_replacement']:
        expected={'display':'42' if key=='calculator' else '0','draft':'seed' if key=='calculator' else 'paired text','Alpha':'alpha'}
        oracle={**expected,'buttons':['6','×','7','='] if key=='calculator' else []}
        case={'id':key,'goal':key,'expected':expected,'outcome':expected}
        a['cases'].append(case)
        b['cases'].append({**copy.deepcopy(case),'model_confirmed':True,'fixture_state':oracle,'owned_daemon_exited':True,'owned_processes_reaped':True})
        tool={'type':'mcpToolCall','server':'node_repl','tool':'js','status':'completed','arguments':{'code':'await sky.get_window_state({window}); await sky.click({window,element_index:3});'}}
        items=[tool]
        if change=='recovered':items.insert(0,{**copy.deepcopy(tool),'status':'failed','arguments':{'code':'await sky.get_window_state({stale});'}})
        if change=='file-route':tool['arguments']['code']+='await fs.writeFile("state.json","42");'
        if change=='missing-actions':tool['arguments']['code']='await sky.get_window_state({window});'
        if change=='only-failures':tool['status']='failed'
        if change=='wrong-model':b['model']='different-model'
        if change=='false-outcome':oracle['display']='wrong'
        folder=reference/key;folder.mkdir()
        (folder/'state.json').write_text(json.dumps(oracle))
        (folder/'thread.json').write_text(json.dumps({'thread':{'model':'test-model','turns':[{'status':'completed','items':items}]}}))
    (reference/'result.json').write_text(json.dumps(a))
    (candidate/'result.json').write_text(json.dumps(b),encoding='utf-8-sig')
    return reference,candidate


def test_windows_bom_and_a_recovered_reference_tool_error(tmp_path):
    result=verify(*fixture(tmp_path,'recovered'))
    assert result['passed']
    assert all(c['reference_failed_tool_calls']==1 for c in result['cases'])


@pytest.mark.parametrize('change',['file-route','missing-actions','only-failures','wrong-model','false-outcome'])
def test_product_false_passes_are_rejected(tmp_path,change):
    assert not verify(*fixture(tmp_path,change))['passed']
