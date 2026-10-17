import importlib.util
from pathlib import Path
import pytest
spec=importlib.util.spec_from_file_location('analysis',Path(__file__).with_name('analyze_memory.py'));module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
def rows():
 return [{'family':'test','instance':0,'arm':arm,'executed':True,'oracle':{'completed':success,'reference':'value'},'value':'value','decisions':steps,'seconds':seconds} for arm,success,steps,seconds in [('without-memory',True,5,12),('with-imported-memory',False,1,2)]]
def test_failed_early_exit_is_never_scored_as_faster():
 r=module.analyze({'cases':rows()},{'pairs':[{'family':'test','instance':0}],'seed':1,'model':'actual'})
 assert r['with_successes']==0
 assert r['deltas_with_minus_without']['decision_delta']['mean'] is None
 assert r['deltas_with_minus_without']['success_delta']['mean']==-1
@pytest.mark.parametrize('mutation',['missing','unexecuted','wrong_value','duplicate'])
def test_incomplete_or_invalid_evidence_cannot_be_a_success(mutation):
 rs=rows();registration={'pairs':[{'family':'test','instance':0}],'seed':1,'model':'actual'}
 if mutation=='wrong_value':
  rs[0]['oracle']['reference']='wrong';r=module.analyze({'cases':rs},registration);assert r['without_successes']==0;return
 if mutation=='missing':rs.pop()
 if mutation=='unexecuted':rs[0]['executed']=False
 if mutation=='duplicate':rs.append(rs[0])
 with pytest.raises(ValueError):module.analyze({'cases':rs},registration)
