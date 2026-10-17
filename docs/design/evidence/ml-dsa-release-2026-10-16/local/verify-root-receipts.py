import asyncio,json,sys
from pathlib import Path
import bittensor as bt

ROOT=Path('/tmp/ms-release-validation')
def receipts(value):
    if isinstance(value,dict):
        if value.get('block_hash') and value.get('extrinsic_id'):
            yield value
        for child in value.values():
            yield from receipts(child)
    elif isinstance(value,list):
        for child in value:
            yield from receipts(child)

async def check(port, paths):
    rows=[]
    async with bt.Client(f'ws://127.0.0.1:{port}',fallback_endpoints=[],archive_endpoints=[]) as client:
        raw=client._substrate.raw
        final_hash=await raw.rpc_request('chain_getFinalizedHead',[])
        final_header=await raw.rpc_request('chain_getHeader',[final_hash])
        final_height=int(final_header['number'],16)
        cache={}
        seen=set()
        for path in paths:
            for receipt in receipts(json.loads(path.read_text())):
                block=receipt['block_hash']
                height_text,index_text=receipt['extrinsic_id'].split('-')
                height,index=int(height_text),int(index_text)
                identity=(block,index)
                if identity in seen:
                    continue
                seen.add(identity)
                assert height<=final_height,receipt
                if block not in cache:
                    assert await raw.rpc_request('chain_getBlockHash',[height])==block,receipt
                    cache[block]=await raw.get_events(block_hash=block)
                selected=[row for row in cache[block] if row.get('extrinsic_idx')==index]
                outcomes=[event for event in selected if event['module_id']=='System' and event['event_id'] in ('ExtrinsicSuccess','ExtrinsicFailed')]
                assert len(outcomes)==1,(receipt,selected,cache[block])
                if receipt['success']:
                    assert outcomes[0]['event_id']=='ExtrinsicSuccess',(receipt,outcomes)
                nested=[event for event in selected if event['module_id'] in ('Sudo','Utility','Multisig','Proxy')]
                rows.append({'source':str(path.relative_to(ROOT)),'extrinsic_id':receipt['extrinsic_id'],'block_hash':block,'sdk_success':receipt['success'],'system_outcome':outcomes[0],'nested_results':nested})
        return {'rpc_port':port,'finalized_height':final_height,'finalized_hash':final_hash,'verified_receipts':len(rows),'receipts':rows}

async def main():
    release=list((ROOT/'live-receipts').glob('late-registration-*.json'))+list((ROOT/'live-receipts').glob('atomic-failure-recovery-*.json'))+list((ROOT/'live-receipts-final').glob('*.json'))
    operations=list((ROOT/'operations-receipts').glob('*.json'))
    selected={int(port) for port in sys.argv[1:]} or {19944,20244}
    results=await asyncio.gather(*(check(port, paths) for port, paths in ((19944,release),(20244,operations)) if port in selected))
    for result in results:
        (ROOT/f"root-finalized-receipts-{result['rpc_port']}.json").write_text(json.dumps(result,indent=2,default=str)+'\n')
    results=[json.loads(path.read_text()) for path in sorted(ROOT.glob('root-finalized-receipts-*.json'))]
    output={'passed':True,'scope':'Canonical finalized block/index and System outcome verification. Nested dispatch events are retained; per-operation tests separately assert business state.','chains':results}
    (ROOT/'root-finalized-receipt-verification.json').write_text(json.dumps(output,indent=2,default=str)+'\n')
    print(json.dumps({'passed':True,'verified_receipts':sum(x['verified_receipts'] for x in results)}))
asyncio.run(main())
