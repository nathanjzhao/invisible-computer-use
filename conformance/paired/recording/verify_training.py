#!/usr/bin/env python3
"""Regrade preserved training evidence. Never replaces the original harness report."""
import argparse,hashlib,json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--training',type=Path,required=True);p.add_argument('--demos',type=Path,required=True);p.add_argument('--registration',type=Path,required=True);a=p.parse_args()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
source=a.training/'result.json';original=json.loads(source.read_text());summaries={p.name:json.loads(p.read_text()) for p in (a.training/'memory/summaries').glob('*.json')};entries=json.loads((a.training/'memory/durable.json').read_text())['entries'];families={p['family'] for p in json.loads(a.registration.read_text())['pairs']}
checks=[]
for family in sorted(families):
 matches=[s for s in summaries.values() if any((r.get('project') or '').casefold()==family for r in s.get('recordings',[]))]
 expected=sha(a.demos/family/'recording-demo/events.jsonl')
 checks.append({'family':family,'passed':len(matches)==1 and all(r['recording_sha256']==expected and r['actor']=='synthetic_fixture' and r['app']=='dev.icu.memorylab' for s in matches for r in s['recordings'])})
ids={s['id'] for s in summaries.values()};valid_entries=bool(entries) and all(e['scope']=='dev.icu.memorylab' and e['untrusted'] and set(e['evidence'])<=ids for e in entries)
report={'passed':all(c['passed'] for c in checks) and valid_entries and len(original['cases'])==8,'original_report_sha256':sha(source),'original_harness_passed':original['passed'],'original_harness_error':original.get('error'),'correction':'Match the family to the final reviewed project label case-insensitively. WebKit capitalized the first character on blur. Source hashes and synthetic actor must still match exactly.','source_checks':checks,'summaries':len(summaries),'entries':len(entries),'valid_untrusted_entries':valid_entries,'summary_hashes':{p:sha(a.training/'memory/summaries'/p) for p in summaries}}
(a.training/'acceptance.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report));raise SystemExit(0 if report['passed'] else 1)
