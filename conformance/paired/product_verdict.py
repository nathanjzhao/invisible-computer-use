"""Verify retained, same-model task outcomes without confusing native and model tests."""
import hashlib
import json
import re
from pathlib import Path


def verify(reference:Path,candidate:Path):
    a=json.loads((reference/'result.json').read_text(encoding='utf-8-sig'));b=json.loads((candidate/'result.json').read_text(encoding='utf-8-sig'))
    errors=[];cases=[]
    if a.get('mode')!='model-driven-product' or b.get('mode')!='model-driven-product':errors.append('wrong comparison mode')
    if a.get('engine')!='openai-native' or b.get('engine')!='icu-packaged-app':errors.append('wrong engines')
    if not a.get('passed') or not b.get('passed') or not a.get('cleanup_verified'):errors.append('an engine did not pass or clean up')
    if a.get('fixture_sha256')!=b.get('fixture_sha256'):errors.append('fixtures differ')
    ar={r['id']:r for r in a.get('cases',[])};br={r['id']:r for r in b.get('cases',[])}
    expected={'calculator','text_replacement'}
    if set(ar)!=expected or set(br)!=expected or len(a.get('cases',[]))!=2 or len(b.get('cases',[]))!=2:errors.append('missing or duplicated cases')
    for key in sorted(expected):
        if key not in ar or key not in br:continue
        x,y=ar[key],br[key];fail=[]
        thread=json.loads((reference/key/'thread.json').read_text())['thread']
        items=[i for turn in thread.get('turns',[]) for i in turn.get('items',[])]
        tool_calls=[i for i in items if i.get('type')=='mcpToolCall']
        if not tool_calls or any(i.get('server')!='node_repl' or i.get('tool')!='js' or i.get('status') not in ['completed','failed'] for i in tool_calls):fail.append('reference computer-use tool evidence missing or unterminated')
        code='\n'.join(i.get('arguments',{}).get('code','') for i in tool_calls)
        completed_code='\n'.join(i.get('arguments',{}).get('code','') for i in tool_calls if i.get('status')=='completed')
        if not re.search(r'\.get_(?:app|window)_state\s*\(',completed_code) or not re.search(r'\.(?:click|set_value|type_text|press_key)\s*\(',completed_code):fail.append('reference observation and native action calls missing')
        if re.search(r'\b(?:writeFile|appendFile|createWriteStream|spawn|execFile|execSync)\b|node:child_process',code):fail.append('reference attempted a file mutation or process route')
        if any(t.get('status')!='completed' for t in thread.get('turns',[])):fail.append('reference turn not completed')
        if any(i.get('type') in ['commandExecution','fileChange','dynamicToolCall'] for i in items):fail.append('reference used a non-computer-use route')
        if thread.get('model')!=b.get('model') or not y.get('model_confirmed'):fail.append('model identity differs or unproven')
        if x['goal']!=y['goal'] or x['expected']!=y['expected']:fail.append('goals or success criteria differ')
        if x.get('outcome')!=y.get('outcome') or x.get('outcome')!=x['expected']:fail.append('outcomes do not meet the common task')
        if not y.get('owned_daemon_exited') or not y.get('owned_processes_reaped'):fail.append('candidate cleanup unproven')
        oracle=json.loads((reference/key/'state.json').read_text())
        if any(oracle[k]!=v for k,v in x['expected'].items()):fail.append('reference outcome contradicts fixture state')
        if any(y['fixture_state'][k]!=v for k,v in y['expected'].items()):fail.append('candidate outcome contradicts fixture state')
        if key=='calculator' and (oracle['buttons'][-4:]!=['6','×','7','='] or y['fixture_state']['buttons'][-4:]!=['6','×','7','=']):fail.append('calculator buttons not exercised')
        cases.append({'id':key,'matched':not fail,'model':thread.get('model'),'reference_tool_calls':len(tool_calls),
                      'reference_failed_tool_calls':sum(i.get('status')=='failed' for i in tool_calls),
                      'outcome':x.get('outcome'),'failures':fail})
    return {'passed':not errors and len(cases)==2 and all(c['matched'] for c in cases),'problems':errors,'cases':cases,
            'scope':'Same-model outcomes on the named fixture. Host prompts and action interfaces differ; this is not a claim of identical model decisions.'}


if __name__=='__main__':
    import argparse
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('reference',type=Path);p.add_argument('candidate',type=Path);a=p.parse_args()
    result=verify(a.reference,a.candidate);print(json.dumps(result,indent=2));raise SystemExit(0 if result['passed'] else 1)
