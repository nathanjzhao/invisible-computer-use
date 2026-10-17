"""Cross-turn element identity in real macOS driver output.

Observes the owned AppKit calculator through Copilot's bundled driver (the
product's own native lineage) while the fixture changes itself between turns,
and checks each claim narrowly:

- repeat observation: every stable_id is unchanged while element tokens change;
- value change: the display keeps its id and the diff marks it changed;
- relabel with a platform identifier (AXIdentifier): the id follows the key;
- removal/addition: the removed key's id is retired, never handed to the new key;
- duplicate labels without identifiers: after one "Same" button is removed the
  survivor must not inherit either old id (no identity from position);
- unlabelled controls without identifiers (the title-bar buttons) that did not
  change in content or window-relative frame keep their ids across a value
  change elsewhere in the window;


  probe_macos_identity.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
"""
import hashlib, json, os, pathlib, signal, sys, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'turns': {}, 'checks': {},
          'scope': "installed Copilot.app's bundled driver, get_window_state diff lineage, owned AppKit fixture",
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)


def jsonl(name):
    p = work / name
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def command(cmd, **extra):
    before = len(jsonl('command-results.jsonl'))
    (work / 'command-calc.json').write_text(json.dumps({'cmd': cmd, **extra}))
    wait_for(lambda: len(jsonl('command-results.jsonl')) > before, 8, 'fixture did not run ' + cmd)
    time.sleep(.3)


def observe(tag, pid, wid):
    raw = s.call('get_window_state', {'pid': pid, 'window_id': wid, 'include_screenshot': False})
    (work / f'turn-{tag}.json').write_text(json.dumps(raw, indent=2, ensure_ascii=False))
    sc = raw.get('structuredContent', raw)
    els = [e for e in sc['elements'] if e.get('role') in ('AXButton', 'AXTextField', 'AXStaticText') and e.get('label')]
    turn = {'snapshot_id': sc.get('snapshot_id'), 'diff': (sc.get('diff') or {}).get('text', ''),
            'ids': [(e['role'], e['label'], e.get('stable_id'), e.get('element_token')) for e in els],
            'unlabelled': [(e['role'], e.get('stable_id')) for e in sc['elements'] if not e.get('label') and not e.get('identifier')]}
    report['turns'][tag] = turn
    return turn


def ids(turn, label, role='AXButton'):
    return [i for r, l, i, _ in turn['ids'] if l == label and r == role]


def check(name, ok, detail=None):
    report['checks'][name] = {'ok': bool(ok), 'detail': detail}


signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('identity probe deadline'))); signal.alarm(180)
try:
    fx, _ = s.fixture_proc('calc')
    s.start_app()
    wins = s.windows()
    W = next(w['window_id'] for w in wins if w.get('pid') == fx.pid and w.get('title') == 'ICU Calculator')
    S = next(w['window_id'] for w in wins if w.get('pid') == fx.pid and w.get('title') == 'ICU Focus Guard')
    t0 = observe('w0', fx.pid, W); t1 = observe('w1', fx.pid, W)
    by_label = lambda t: {(r, l): i for r, l, i, _ in t['ids']}
    check('repeat_ids_unchanged', by_label(t0) == by_label(t1) and all(i is not None for _, _, i, _ in t0['ids']), {f'{r} {l}': i for (r, l), i in by_label(t1).items()})
    check('tokens_change_but_ids_do_not', {tok for *_, tok in t0['ids']}.isdisjoint({tok for *_, tok in t1['ids']}), [t0['snapshot_id'], t1['snapshot_id']])
    result_id = ids(t0, 'Result', 'AXTextField')
    command('set_display', value='42'); t2 = observe('w2', fx.pid, W)
    check('value_change_keeps_id', ids(t2, 'Result', 'AXTextField') == result_id and f'~ [{result_id[0]}]' in t2['diff'] and '"42"' in t2['diff'], t2['diff'])
    check('unchanged_unlabelled_controls_keep_ids', t1['unlabelled'] and t2['unlabelled'] == t1['unlabelled']
          and not any(line.lstrip().startswith(('+', '-')) for line in t2['diff'].splitlines()[2:]),
          {'w1': t1['unlabelled'], 'w2': t2['unlabelled'], 'diff': t2['diff']})
    nine = ids(t2, '9')
    command('relabel_nine'); t3 = observe('w3', fx.pid, W)
    check('identifier_backed_relabel_keeps_id', ids(t3, 'Nine') == nine and not ids(t3, '9') and f'~ [{nine[0]}]' in t3['diff'], t3['diff'])
    eight = ids(t3, '8'); known = {i for _, _, i, _ in t0['ids'] + t3['ids']}
    command('remove_eight'); command('add_new'); t4 = observe('w4', fx.pid, W)
    new = ids(t4, 'New')
    check('removed_id_retired_not_reused', not ids(t4, '8') and len(new) == 1 and new[0] not in known and new[0] != eight[0], {'eight': eight, 'new': new, 'diff': t4['diff']})
    s0 = observe('s0', fx.pid, S); s1 = observe('s1', fx.pid, S)
    same = ids(s0, 'Same')
    check('duplicate_labels_distinct_ids', len(same) == 2 and len(set(same)) == 2 and ids(s1, 'Same') == same, same)
    command('remove_first_same'); s2 = observe('s2', fx.pid, S)
    survivor = ids(s2, 'Same')
    check('ambiguous_survivor_gets_no_old_id', len(survivor) == 1 and survivor[0] not in same, {'old': same, 'survivor': survivor, 'diff': s2['diff']})
    report['ok'] = all(c['ok'] for c in report['checks'].values())
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
finally:
    signal.alarm(0)
    s.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    for k, c in report['checks'].items(): print(('PASS ' if c['ok'] else 'FAIL ') + k, json.dumps(c['detail'], ensure_ascii=False)[:400])
    print(json.dumps({'ok': report.get('ok'), 'error': report.get('error')}))
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
