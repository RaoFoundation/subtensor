from pathlib import Path
import json,re
p=Path(__file__).parent
rows=[]
for f in sorted(p.glob('*/pytest.log')):
 if (f.parent/'rerun/pytest.log').exists():f=f.parent/'rerun/pytest.log'
 s=f.read_text();passed=re.findall(r'PASSED (tests/\S+)',s)
 if not passed:passed=re.findall(r'(tests/\S+) PASSED',s)
 rows.append({'suite':f.relative_to(p).parts[0],'passed':len(set(passed)),'failed':len(set(re.findall(r'FAILED (tests/\S+)',s)))})
print(json.dumps(rows))
for f in ['preflight-final.exit','sdk-final.exit','mainnet-verified.exit','rpc/followup-results.json','operations/rerun/exit']:
 q=p/f
 if q.exists():print(f,q.read_text().strip())
