import asyncio,json,os,time
from pathlib import Path
import bittensor as bt
from bittensor.evm.addresses import h160_to_ss58
from tests.harness.samples import dev_wallet
async def main():
 async with bt.Client('ws://127.0.0.1:20246',fallback_endpoints=[],archive_endpoints=[]) as c:
  raw=c._substrate.raw; rpc=raw._session.request; bob=dev_wallet('//Bob').coldkey
  target='0x'+os.urandom(20).hex(); dest=h160_to_ss58(target)
  deadline=time.monotonic()+180
  while True:
   nonce=int(await rpc('system_accountNextIndex',[bob.ss58_address]))
   finalized=await rpc('chain_getFinalizedHead',[])
   confirmed=await c._substrate.query('System','Account',[bob.ss58_address],block_hash=finalized)
   if nonce==int(confirmed['nonce']):break
   assert time.monotonic()<deadline,'prior dev-sender transactions did not finalize'
   await asyncio.sleep(3)
  before=int(await rpc('eth_getBalance',[target,'latest']),16); assert before==0
  call=await c.compose(bt.calls.Balances.transfer_keep_alive(dest=dest,value=10**9))
  signed=[await raw.create_signed_extrinsic(call,bob,nonce=nonce+i,era='00') for i in range(32)]
  hashes=await asyncio.gather(*[rpc('author_submitExtrinsic',[tx.data_hex]) for tx in signed])
  ready=await rpc('author_pendingExtrinsics',[])
  outcomes=[]; marker='Local runtime execution capacity reached; retry later'
  expected=32*10**18
  for _ in range(8):
   try:
    value=int(await rpc('eth_getBalance',[target,'pending']),16)
    outcomes.append({'value':str(value)})
    assert value==expected,('partial successful pending balance',value,expected)
   except Exception as error:
    if isinstance(error,AssertionError):raise
    assert marker in str(error),(type(error).__name__,str(error))
    outcomes.append({'overload':str(error)})
   if any('overload' in x for x in outcomes):break
   await asyncio.sleep(.05)
  assert any('overload' in x for x in outcomes),'No capacity error observed; cannot claim exact overload path was exercised'
  deadline=time.monotonic()+180
  while True:
   final=await rpc('chain_getFinalizedHead',[])
   value=await c._substrate.query('System','Account',[dest],block_hash=final)
   if int(value['data']['free'])==32*10**9:break
   assert time.monotonic()<deadline,'transfers did not finalize'
   await asyncio.sleep(3)
  report={'test':'real pending pool capacity failure propagation','genesis':await rpc('chain_getBlockHash',[0]),'node_port':20246,'target':target,'destination':dest,'submitted':hashes,'pending_pool_entries_observed':len(ready),'pending_outcomes':outcomes,'finalized_block':final,'finalized_received_rao':int(value['data']['free']),'expected_received_rao':32*10**9,'limitation':'Disposable SR dev sender isolates pending-state behavior; no production funds or throughput claim.'}
  Path('/tmp/ms-release-validation/live-pending-results.json').write_text(json.dumps(report,indent=2)+'\n'); print(json.dumps(report,indent=2))
asyncio.run(main())
