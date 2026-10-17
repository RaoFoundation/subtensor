"""Read-only local admission isolation reproduction, restricted to validator 20245."""
import concurrent.futures,datetime,hashlib,json,time,urllib.request
from pathlib import Path
URL='http://127.0.0.1:20245'
MARKER='Local runtime execution capacity reached; retry later'
ALLOWED={'author_pendingExtrinsics','chain_getBlockHash','chain_getHeader','eth_getBalance','state_call','state_getStorage'}
def rpc(method,params):
    assert method in ALLOWED
    req=urllib.request.Request(URL,data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(req,timeout=15) as response:return json.load(response)
def limited(response):return MARKER in json.dumps(response)
started=time.monotonic(); drain=[]
target='0x'+'00'*20
while True:
    pool=rpc('author_pendingExtrinsics',[])
    assert 'result' in pool,pool
    count=len(pool['result']); drain.append({'elapsed_seconds':time.monotonic()-started,'count':count})
    if count<8:
        time.sleep(2.25)
        baseline_latest=rpc('eth_getBalance',[target,'latest'])
        baseline_pending=rpc('eth_getBalance',[target,'pending'])
        if 'result' in baseline_latest and 'result' in baseline_pending:break
    assert time.monotonic()-started<180,'Pending baseline did not become healthy with fewer than 8 pool entries; no flood sent'
    time.sleep(3)
pool_before=len(rpc('author_pendingExtrinsics',[])['result'])
assert pool_before<8,'Pool increased; aborting before flood'
time.sleep(2.25)
flood_started=time.monotonic()
with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
    responses=list(executor.map(lambda _:rpc('state_call',['MldsaAdmissionProbe_unexported','0x']),range(64)))
flood_elapsed=time.monotonic()-flood_started
assert all('error' in r for r in responses) and any(limited(r) for r in responses)
# This direct probe names the first initialization API explicitly. A capacity
# response proves rejection occurs before malformed arguments reach decoding.
initialize_probe=rpc('state_call',['EthereumRuntimeRPCApi_initialize_pending_block','0x'])
initialize_probe_elapsed=time.monotonic()-flood_started
pending=rpc('eth_getBalance',[target,'pending'])
pending_elapsed=time.monotonic()-flood_started
latest=rpc('eth_getBalance',[target,'latest'])
pool_after=len(rpc('author_pendingExtrinsics',[])['result'])
assert 'result' in latest,latest
assert limited(initialize_probe),initialize_probe
assert limited(pending),pending
# Recovery after refill distinguishes admission throttling from a persistent
# runtime failure; no transaction or state mutation is submitted.
time.sleep(2.25)
recovered_pending=rpc('eth_getBalance',[target,'pending'])
assert 'result' in recovered_pending,recovered_pending
code=rpc('state_getStorage',['0x3a636f6465'])['result']
out={
 'test':'unknown-export flood still blocks EVM pending reconstruction',
 'performed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'endpoint':URL,'genesis':rpc('chain_getBlockHash',[0])['result'],
 'runtime_wasm_sha256':hashlib.sha256(bytes.fromhex(code[2:])).hexdigest(),
 'test_script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
 'read_only_rpc_methods':sorted(ALLOWED),'submitted_transactions':0,
 'pool_drain_observations':drain,'pool_count_before_flood':pool_before,'pool_count_after_flood':pool_after,
 'baseline_latest':baseline_latest,'baseline_pending':baseline_pending,
 'unknown_export_requests':64,'unknown_export_capacity_rejections':sum(limited(r) for r in responses),
 'unknown_response_samples':{'first':responses[0],'last':responses[-1]},
 'flood_elapsed_seconds':flood_elapsed,
 'named_initialize_pending_block_probe':initialize_probe,'initialize_probe_elapsed_seconds':initialize_probe_elapsed,
 'pending_after_flood':pending,'pending_elapsed_seconds':pending_elapsed,
 'latest_after_flood':latest,'pending_after_refill':recovered_pending,
 'stage_classification':{
  'initialize_pending_block_export_admission_denied':True,
  'pending_rpc_internal_failure_stage':'RPC wrapper does not report exact failed export; cannot distinguish initial API, fallback Core_initialize_block, or an inherent apply from this response alone',
  'ready_pool_transaction_apply_not_needed_for_reproduction':pool_after==0,
  'relation_to_36_entry_test':'This previously healthy, small-pool flood reproduces independent-service coupling. The separate 36-entry test exercises pending reconstruction under a populated pool; its generic API error also does not identify the exact failing call.'
 },
 'verdict':'CONFIRMED MEDIUM pending-service availability isolation gap; fail-closed balance behavior retained',
 'limits':['Only validator20245 was probed','No chain writes or signed transactions','No internal per-export tracing was enabled; exact nested failure stage remains unobserved'],
}
Path('/tmp/ms-release-validation/live-pending-isolation-results.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps(out,indent=2))
