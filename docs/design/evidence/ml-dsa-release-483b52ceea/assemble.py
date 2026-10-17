"""Package only validation evidence, excluding wallet secrets and node databases."""
from pathlib import Path
import gzip,hashlib,json,subprocess,xml.etree.ElementTree as ET
R=Path('/home/johnreed/Desktop/work/subtensor');P=Path(__file__).parent
OUT=R/'docs/design/evidence/ml-dsa-release-483b52ceea'
SUITES={x:x+'/results.xml' for x in ['release','authority','validators','lifecycle','parity','extended','proxy-boundaries','sponsorship','nested','validator-proxy','mixed','shield']}
SUITES['operations']='operations/rerun/results.xml'
SUITES.update(activation='rpc/activation.xml',additional='rpc/additional.xml')
def junit(path):
 cases=[]
 for c in ET.parse(path).iter('testcase'):
  cases.append({'class':c.get('classname'),'name':c.get('name'),'seconds':float(c.get('time',0)),'failed':c.find('failure') is not None or c.find('error') is not None,'skipped':c.find('skipped') is not None})
 return {'passed':sum(not c['failed'] and not c['skipped'] for c in cases),'failed':sum(c['failed'] for c in cases),'skipped':sum(c['skipped'] for c in cases),'cases':cases}
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 runs={name:{'source':src,**junit(P/src)} for name,src in SUITES.items()}
 assert sum(x['passed'] for x in runs.values())==97
 assert not any(x['failed'] or x['skipped'] for x in runs.values())
 assert (P/'preflight-final.exit').read_text().strip()=='0'
 assert (P/'sdk-final.exit').read_text().strip()=='0'
 assert (P/'mainnet-verified.exit').read_text().strip()=='0'
 assert (P/'shutdown-final.json').exists()
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==(P/'head.txt').read_text().strip()
 assert json.loads((P/'shutdown-final.json').read_text())['all_closed']
 assert json.loads((P/'mixed/cli-state-verification.json').read_text())['all_verified']
 baseline=json.loads((P/'source-manifest.json').read_text())
 changed=[s for s,h in baseline.items() if sha(R/s)!=h]
 assert changed==['sdk/python/bittensor/_transport/interface.py'],changed
 summary={'head':(P/'head.txt').read_text().strip(),'runtime_source_unchanged':True,'sdk_fix':'Bounded retry of identical signed bytes only on explicit pre-admission overload; no retries after watch acceptance or ambiguous send.','live_runs':runs,'live_passed':97,'normal_block_seconds':12,'fresh_chains':14,'validators_per_chain':3,'reference_weights':json.loads((P/'weight-selection-verification.json').read_text()),'sdk':junit(P/'sdk-final.xml'),'sdk_optional':junit(P/'sdk-legacy-final.xml'),'crypto':json.loads((P/'crypto/crosscheck-results.json').read_text()),'mainnet_current':json.loads((P/'mainnet-current.json').read_text()),'mainnet_rehearsal':json.loads((P/'mainnet-verified/ml-dsa-mainnet-upgrade-results.json').read_text()),'shutdown':json.loads((P/'shutdown-final.json').read_text()),'receipt_audit':json.loads((P/'receipt-verification/combined-summary.json').read_text()),'local_changes':subprocess.check_output(['git','status','--short'],cwd=R,text=True),'superseded_attempts':{'preflight':'Committed evidence CSV line endings, then final SDK formatting, failed cheap checks; both corrected and full gate rerun successfully. The original final-recheck wrapper retains its earlier formatting failure; final-recheck-results.json identifies accepted logs.','operations':'Original SDK surfaced transient admission rejection; all eight cases rerun after bounded submission retry fix.','rpc':'Historical fixture referenced removed subordinate activation storage; updated to single flag and rerun.','mainnet':'Incorrect executable/snapshot paths in initial wrapper; pinned official executable and exact snapshot hash verified before accepted run.'}}
 s=(P/'preflight-final.log').read_text();import re
 rows=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',s)
 summary['rust']={'passed':sum(int(x[0]) for x in rows),'failed':sum(int(x[1]) for x in rows),'ignored':sum(int(x[2]) for x in rows),'targets':len(rows)}
 summary['sdk_validation_boundary']='The runtime was unchanged throughout. Live suites began on HEAD; the eight operation-family cases were rerun with the initial local SDK admission fix. A subsequent refinement captured the payload/hash before awaiting to prevent caller mutation. The complete offline SDK suite, 20 focused regression cases, and full preflight passed after the final refinement. Already-running live processes were not restarted.'
 summary['final_source_sha256']={s:sha(R/s) for s in baseline}
 summary['final_source_sha256']['sdk/python/tests/unit/test_submission_admission.py']=sha(R/'sdk/python/tests/unit/test_submission_admission.py')
 summary['mixed_cli_finalized_state']=json.loads((P/'mixed/cli-state-verification.json').read_text())
 summary['decoder_compatibility']={s:json.loads((P/'codecs'/s).read_text()) for s in ['javascript.json','native-current.json','native-legacy.json']}
 summary['rpc']={s:json.loads((P/'rpc'/s).read_text()) for s in ['fixed-live-results.json','ethereum-pending-results.json','pending-contract-results.json']}
 assert all(v['status']=='passed' for v in summary['rpc'].values())
 assert summary['rust']=={'passed':3583,'failed':0,'ignored':47,'targets':89}
 OUT.mkdir(parents=True,exist_ok=True)
 selected=[]
 for pattern in ['*.json','*.exit','*.log','*.py','*.sh','*.sha256','*.xml','*.txt','crypto/*.json','crypto/*.py','crypto/*.rs','crypto/*.log','codecs/*.json','codecs/*.cjs','codecs/*.log','rpc/*.json','rpc/*.xml','rpc/*.log','rpc/*.py','mixed/cli-state-verification.json','mixed/verify-cli-state.py','mixed/verification*.log','mainnet-verified/*.json','mainnet-verified/*.log','receipt-verification/*.json','receipt-verification/*.py','receipt-verification/*.log','*/results.xml','*/pytest.log','*/activation.json','*/activation.log','*/receipts/*.json','*/rerun/results.xml','*/rerun/pytest.log','*/rerun/receipts/*.json','validator-proxy/verify*','*/localnet.sh','*/genesis.py']:
  selected.extend(P.glob(pattern))
 manifest={}
 for src in sorted(set(selected)):
  if not src.is_file():continue
  rel=src.relative_to(P);dst=OUT/rel
  if src.suffix=='.log':dst=dst.with_suffix('.log.gz');data=gzip.compress(src.read_bytes(),mtime=0)
  else:data=src.read_bytes()
  dst.parent.mkdir(parents=True,exist_ok=True);dst.write_bytes(data);manifest[str(dst.relative_to(OUT))]=sha(dst)
 summary['portable_files']=manifest
 (OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
 print(OUT)
if __name__=='__main__':main()
