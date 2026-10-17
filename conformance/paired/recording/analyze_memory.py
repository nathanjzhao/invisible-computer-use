#!/usr/bin/env python3
"""Summarize completed registered model pairs without dropping task failures."""
import argparse,json,random,statistics
from collections import defaultdict
from pathlib import Path

def analyze(report,registration):
 expected={(p['family'],p['instance']) for p in registration['pairs']};pairs=defaultdict(dict)
 for row in report['cases']:
  key=(row['family'],row['instance']);arm=row['arm']
  if key not in expected or arm not in ('with-imported-memory','without-memory') or arm in pairs[key]:raise ValueError('unregistered or duplicate trial')
  if not row.get('executed') or row.get('error') or not isinstance(row.get('oracle'),dict):raise ValueError('unexecuted or incomplete trial')
  if row.get('decisions',0)<1:raise ValueError('actual model decisions not proven')
  # The independent oracle decides success, not a model's assertion or prior verdict.
  row=dict(row);row['success']=row['oracle'].get('completed') is True and row['oracle'].get('reference')==row['value']
  pairs[key][arm]=row
 if set(pairs)!=expected or any(len(p)!=2 for p in pairs.values()):raise ValueError('registered pairs are incomplete')
 family=defaultdict(lambda:defaultdict(list));rows=[]
 for (name,instance),arms in sorted(pairs.items()):
  plain=arms['without-memory'];learned=arms['with-imported-memory'];both=plain['success'] and learned['success']
  row={'family':name,'instance':instance,'without_success':plain['success'],'with_success':learned['success'],'without_decisions':plain['decisions'],'with_decisions':learned['decisions'],'without_seconds':plain['seconds'],'with_seconds':learned['seconds']}
  family[name]['success_delta'].append(int(learned['success'])-int(plain['success']))
  if both:
   family[name]['decision_delta'].append(learned['decisions']-plain['decisions']);family[name]['seconds_delta'].append(learned['seconds']-plain['seconds'])
  rows.append(row)
 rng=random.Random(registration['seed']);names=sorted(family);metrics={}
 for metric in ['success_delta','decision_delta','seconds_delta']:
  eligible={n:statistics.mean(family[n][metric]) for n in names if family[n][metric]}
  if not eligible:metrics[metric]={'mean':None,'families':0};continue
  values=list(eligible.values());draws=sorted(statistics.mean(rng.choice(values) for _ in values) for _ in range(10000))
  metrics[metric]={'mean':statistics.mean(values),'families':len(values),'family_bootstrap_95_percent':[draws[249],draws[9749]]}
 return {'status':'completed_exploratory_experiment','model':registration['model'],'families':len(family),'paired_instances':len(pairs),'trials':len(report['cases']),'without_successes':sum(r['without_success'] for r in rows),'with_successes':sum(r['with_success'] for r in rows),'deltas_with_minus_without':metrics,'pairs':rows,'limitation':'Eight synthetic workflow families. Timing includes application and UI wait time. This does not establish general real-world benefit.'}
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('report',type=Path);p.add_argument('registration',type=Path);p.add_argument('--out',type=Path,required=True);a=p.parse_args();out=analyze(json.loads(a.report.read_text()),json.loads(a.registration.read_text()));a.out.write_text(json.dumps(out,indent=2)+'\n');print(json.dumps({k:v for k,v in out.items() if k!='pairs'},indent=2))
