from pathlib import Path
import json,os,subprocess,sys,time,xml.etree.ElementTree as E
P=Path(__file__).parent
while True:
 r=json.loads((P/'live-results.json').read_text())
 if 'mixed' in r:
  assert r['mixed'].get('pytest_exit')==0,r['mixed']
  break
 time.sleep(3)
cases=list(E.parse(P/'mixed/results.xml').iter('testcase'));assert len(cases)==24 and all(not any(c.find(x) is not None for x in ['failure','error','skipped']) for c in cases)
with (P/'mixed/verification-mode1.log').open('w') as f:subprocess.run([sys.executable,str(P/'mixed/verify-cli-state.py'),'1'],stdout=f,stderr=subprocess.STDOUT,check=True)
assert json.loads((P/'mixed/cli-state-verification.json').read_text())['all_verified']
for f in ['preflight-final.exit','sdk-final.exit','mainnet-verified.exit','operations/rerun/exit']:assert (P/f).read_text().strip()=='0',f
(P/'stop').write_text('All live suites and independent finalized receipt/state audits completed.\n')
print('Mixed cases passed and finalized state verified; stopping final localnet.',flush=True)
for _ in range(30):
 if (P/'shutdown.json').exists():break
 time.sleep(2)
else:raise TimeoutError('orchestrator shutdown')
time.sleep(2)
with (P/'shutdown-check.log').open('w') as f:subprocess.run([sys.executable,str(P/'check-shutdown.py')],stdout=f,stderr=subprocess.STDOUT,check=True)
print('All owned localnets shut down and ports verified closed.',flush=True)
