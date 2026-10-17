"""Bind a separately logged snapshot acquisition to untouched replay evidence."""
import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

BASE=Path('/tmp/ms-release-validation/mainnet')
REPO=Path('/home/johnreed/Desktop/work/subtensor')
CLI=Path('/tmp/hashed-full-ci-_as7hfjj/bin/try-runtime.real')
RUNNER=REPO/'docs/design/evidence/ml-dsa-mainnet-upgrade.py'
CHECKER=REPO/'docs/design/evidence/ml-dsa-mainnet-log-check.py'
def sha256(path):
    digest=hashlib.sha256()
    with path.open('rb') as handle:
        for block in iter(lambda:handle.read(1024*1024),b''):digest.update(block)
    return digest.hexdigest()
def one(pattern,text):
    matches=re.findall(pattern,text)
    assert len(matches)==1,(pattern,matches)
    return matches[0]
def run(command,log):
    print(json.dumps({'command':command,'log':str(log)}),flush=True)
    with log.open('w') as handle:
        subprocess.run(command,stdout=handle,stderr=subprocess.STDOUT,check=True,cwd=REPO)

manifest=json.loads((BASE/'logged-acquisition-manifest.json').read_text())
original=json.loads((BASE/'ml-dsa-mainnet-upgrade-results.json').read_text())
source=json.loads((BASE/'source.json').read_text())
assert sha256(CLI)==original['try_runtime_sha256']==manifest['cli_sha256']
assert sha256(RUNNER)==original['script_sha256']
assert sha256(BASE/'runtime.wasm')==original['runtime_wasm_sha256']
assert sha256(BASE/'source.json')==original['source_json_sha256']
original_hash=sha256(BASE/'mainnet.snap')
assert original_hash==original['snapshot_sha256']
new_hash=sha256(BASE/'mainnet-logged.snap')
text=(BASE/'snapshot-logged.log').read_text()
root=one(r'initialized state externalities with storage root (0x[0-9a-f]{64})',text)
assert root==source['state_root']
size=int(one(r'writing snapshot of (\d+) bytes',text))
assert size==(BASE/'mainnet-logged.snap').stat().st_size
keys=int(one(r'Parallel key fetching complete: (\d+) unique keys',text))
children=int(one(r'Completed scraping (\d+) child tries',text))
manifest.update(completion_status='passed',completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),snapshot_sha256=new_hash,snapshot_size_bytes=size,snapshot_acquisition_root=root,unique_top_level_keys=keys,observed_child_tries=children,acquisition_log_sha256=sha256(BASE/'snapshot-logged.log'),snapshot_bytes_identical_to_original=new_hash==original_hash)
(BASE/'logged-acquisition-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
if new_hash==original_hash:
    result=BASE/'ml-dsa-mainnet-upgrade-results.json'
    log_dir=BASE
else:
    log_dir=BASE/'logged-verification'
    assert not log_dir.exists(),'Preserve prior artifacts; choose an unused directory before rerunning'
    run([sys.executable,str(RUNNER),'--runtime',str(BASE/'runtime.wasm'),'--snapshot',str(BASE/'mainnet-logged.snap'),'--source',str(BASE/'source.json'),'--output-dir',str(log_dir),'--try-runtime',str(CLI)],BASE/'logged-replay-runner.log')
    result=log_dir/'ml-dsa-mainnet-upgrade-results.json'
output=BASE/'logged-full-verification.json'
assert not output.exists(),'Preserve prior full verification result'
run([sys.executable,str(CHECKER),'--result',str(result),'--runner',str(RUNNER),'--source',str(BASE/'source.json'),'--log-dir',str(log_dir),'--snapshot-log',str(BASE/'snapshot-logged.log'),'--output',str(output)],BASE/'logged-full-verification.log')
verified=json.loads(output.read_text())
assert verified['snapshot_sha256']==new_hash
assert verified['snapshot_acquisition_proof']['state_root_matches_finalized_header']
assert verified['runs'][1]['observed_successful_blocks']==10
manifest.update(full_checker='passed',full_checker_source_sha256=sha256(CHECKER),verified_result=str(output),verified_result_sha256=sha256(output),raw_runner_result=str(result),strict_replay_repeated=new_hash!=original_hash,release_ready=False,remaining_release_blockers=['Hostile pre-registration hotkey ownership can prevent descriptor registration and trap bare-funded balances','Unknown runtime exports can still exhaust Other admission needed by pending EVM reconstruction'])
(BASE/'logged-acquisition-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(manifest,indent=2),flush=True)
