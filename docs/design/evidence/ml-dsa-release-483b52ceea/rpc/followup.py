from pathlib import Path
import subprocess,sys,os,json
D=Path(__file__).parent;R=Path('/home/johnreed/Desktop/work/subtensor');P=sys.executable
results=[]
commands=[
 ('pending-contracts',[P,str(D/'pending_contract_semantics.py')]),
 ('additional-flows',[P,'-m','pytest','tests/e2e/test_emission_weights_cli.py','tests/e2e/test_null_consensus.py','tests/e2e/test_timelock_commitment.py','tests/e2e/test_pow_registration.py','-vv','--tb=short',f'--junitxml={D}/additional.xml']),
]
for name,cmd in commands:
 with (D/f'{name}.log').open('w') as out:
  code=subprocess.call(cmd,cwd=R/'sdk/python',env={**os.environ,'E2E_ADMIN_ENDPOINT':'ws://127.0.0.1:21044','E2E_ENDPOINT':'ws://127.0.0.1:21044'},stdout=out,stderr=subprocess.STDOUT)
 results.append({'name':name,'exit':code});(D/'followup-results.json').write_text(json.dumps(results,indent=2));print(name,code,flush=True)
 if code:sys.exit(code)
