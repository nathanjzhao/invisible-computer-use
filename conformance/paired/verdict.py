"""Compare independently observed outcomes; never equate missing evidence with parity."""
import hashlib
import json
from pathlib import Path


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def compare(reference, candidate, contract):
    problems = []
    differences = []
    required = contract['cases']
    if reference.get('engine') != 'openai-native':
        problems.append('reference must execute the actual OpenAI native runtime')
    if candidate.get('engine') != 'icu-native':
        problems.append('candidate must execute our native implementation')
    for name, report in [('reference', reference), ('candidate', candidate)]:
        display = report.get('display')
        if not report.get('os') or not isinstance(display, list) or len(display) != 3 or any(type(x) not in (int, float) or x <= 0 for x in display):
            problems.append(f'{name}: measured operating system/display missing')
        for key in ['executed', 'cleanup_verified']:
            if report.get(key) is not True:
                problems.append(f'{name}: {key} is not true')
        if report.get('fixture_sha256') != contract.get('fixture_sha256'):
            problems.append(f'{name}: fixture identity differs')
        for key in ['engine_sha256', 'harness_sha256']:
            value = report.get(key)
            if not isinstance(value, str) or len(value) != 64 or any(c not in '0123456789abcdef' for c in value):
                problems.append(f'{name}: {key} missing')
        if report.get('mode') != contract['mode']:
            problems.append(f'{name}: comparison mode differs')
    if reference.get('os') != candidate.get('os'):
        problems.append('operating systems differ')
    if reference.get('display') != candidate.get('display'):
        problems.append('display geometry differs')
    for report in [reference, candidate]:
        ids = [case.get('id') for case in report.get('cases', [])]
        if len(ids) != len(set(ids)) or set(ids) != set(required):
            problems.append('case set is incomplete, duplicated, or unexpected')
    by_id = [{case['id']: case for case in report.get('cases', [])} for report in [reference, candidate]]
    results = []
    for name, spec in required.items():
        a, b = [cases.get(name, {}) for cases in by_id]
        failures = []
        for side, case in [('reference', a), ('candidate', b)]:
            if case.get('executed') is not True or case.get('error'):
                failures.append(f'{side}: case not executed successfully')
            if not case.get('observations') or not case.get('actions'):
                failures.append(f'{side}: live observations/actions missing')
            if contract.get('ownership_required'):
                ownership = case.get('ownership', {})
                if type(ownership.get('sample_count')) is not int or ownership['sample_count'] < 20:
                    failures.append(f'{side}: insufficient ownership evidence')
                if ownership.get('guard_text') != 'guard':
                    failures.append(f'{side}: foreground guard received input or was not checked')
            outcome = case.get('outcome', {})
            for key, value in spec['expected'].items():
                if outcome.get(key) != value:
                    failures.append(f'{side}: expected {key}={value!r}, got {outcome.get(key)!r}')
        equal = a.get('outcome') == b.get('outcome') and bool(a.get('outcome'))
        if not equal:
            differences.append(name)
        results.append({'id': name, 'matched': equal and not failures, 'failures': failures,
                        'reference_outcome': a.get('outcome'), 'candidate_outcome': b.get('outcome'),
                        'reference_ownership': a.get('ownership'), 'candidate_ownership': b.get('ownership')})
    ownership_differences = []
    if contract.get('ownership_required'):
        for row in results:
            a,b=row['reference_ownership'] or {},row['candidate_ownership'] or {}
            if [(a.get(k)==0) for k in ('violations','hid_resets')] != [(b.get(k)==0) for k in ('violations','hid_resets')]:
                ownership_differences.append(row['id'])
    return {'schema': 1, 'passed': not problems and bool(results) and all(r['matched'] for r in results),
            'mode': contract['mode'], 'problems': problems, 'differences': differences, 'cases': results,
            'ownership_differences':ownership_differences,
            'claim': 'Task-outcome equivalence for the listed cases only. Ownership differences are separate measured findings, not an equivalence pass.'}


def main():
    import argparse
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('contract', type=Path); p.add_argument('reference', type=Path); p.add_argument('candidate', type=Path)
    args = p.parse_args()
    result = compare(*(json.loads(p.read_text()) for p in [args.reference, args.candidate, args.contract]))
    print(json.dumps(result, indent=2))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
