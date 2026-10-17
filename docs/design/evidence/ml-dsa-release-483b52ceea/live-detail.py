import asyncio,json
from pathlib import Path
import bittensor as bt
async def main():
 for name,port in [('mixed',21944)]:
  async with bt.Client(f'ws://127.0.0.1:{port}',fallback_endpoints=[],archive_endpoints=[]) as c:
   raw=c._substrate.raw;rpc=raw._session.request
   h=await rpc('chain_getFinalizedHead',[]);header=await rpc('chain_getHeader',[h]);height=int(header['number'],16);codec=await raw._runtimes.codec_at(h)
   def calls(x):
    if isinstance(x,dict):
     if 'call_module' in x:yield x['call_module']+'.'+x['call_function']
     for v in x.values():yield from calls(v)
    elif isinstance(x,list):
     for v in x:yield from calls(v)
   rows=[]
   for n in range(max(1,height-5),height+1):
    bh=await rpc('chain_getBlockHash',[n]);b=await rpc('chain_getBlock',[bh])
    for xt in b['block']['extrinsics']:
     ns=list(calls(codec.decode_extrinsic(xt)))
     if ns and ns[0] not in ['Timestamp.set','SubtensorModule.drain_emission','MevShield.announce_next_key','Drand.write_pulse']:rows.append({'block':n,'calls':ns})
   print(json.dumps({'chain':name,'finalized':height,'recent_calls':rows}))
asyncio.run(main())
