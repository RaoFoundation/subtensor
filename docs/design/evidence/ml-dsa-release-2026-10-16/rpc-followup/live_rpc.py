"""Disposable localhost regression; never accepts an external RPC endpoint."""
import asyncio
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import sys
import threading
import time
import urllib.request

import bittensor as bt
from bittensor.evm.addresses import h160_to_ss58
from tests.harness.samples import dev_wallet

ROOT = Path('/tmp/ms-release-validation/rpc-followup')
MODE = sys.argv[1]
assert MODE in ('baseline', 'fixed')
URL = 'http://127.0.0.1:20546'
MARKER = 'Local runtime execution capacity reached; retry later'

def http(method, params):
    req = urllib.request.Request(URL, data=json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}).encode(), headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=25) as response:
        return json.load(response)

async def result(rpc, method, params):
    started = time.monotonic()
    try:
        value = {'result': await rpc(method, params)}
    except Exception as error:
        value = {'error': str(error)}
    value['seconds'] = time.monotonic() - started
    return value

async def next_block(rpc):
    before = await rpc('chain_getBlockHash', [])
    deadline = time.monotonic() + 45
    while await rpc('chain_getBlockHash', []) == before:
        assert time.monotonic() < deadline
        await asyncio.sleep(.2)

async def finalized_balance(c, account, expected):
    deadline = time.monotonic() + 180
    while True:
        final = await c._substrate.raw._session.request('chain_getFinalizedHead', [])
        record = await c._substrate.query('System', 'Account', [account], block_hash=final)
        if int(record['data']['free']) == expected:
            return {'block_hash': final, 'balance_rao': expected}
        assert time.monotonic() < deadline, record
        await asyncio.sleep(2)

async def main():
    out = {'mode': MODE, 'at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'endpoint': URL, 'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    async with bt.Client('ws://127.0.0.1:20546', fallback_endpoints=[], archive_endpoints=[]) as c:
        raw = c._substrate.raw
        rpc = raw._session.request
        out['genesis'] = await rpc('chain_getBlockHash', [0])
        code = await rpc('state_getStorage', ['0x3a636f6465'])
        out['runtime_wasm_sha256'] = hashlib.sha256(bytes.fromhex(code[2:])).hexdigest()
        bob = dev_wallet('//Bob').coldkey
        ed = int(await c.constant(('Balances', 'ExistentialDeposit')))
        out['existential_deposit_rao'] = ed

        # A new EVM sender exists only in the pending overlay. Simulating its
        # value transfer must see that overlay; latest must remain unfunded.
        target = '0x' + os.urandom(20).hex()
        dest = h160_to_ss58(target)
        call = await c.compose(bt.calls.Balances.transfer_keep_alive(dest=dest, value=10**9))
        nonce = int(await rpc('system_accountNextIndex', [bob.ss58_address]))
        tx = await raw.create_signed_extrinsic(call, bob, nonce=nonce, era='00')
        await next_block(rpc)
        txhash = await rpc('author_submitExtrinsic', [tx.data_hex])
        req = {'from': target, 'to': '0x' + '14' * 20, 'value': hex(2*ed*10**9), 'gas': hex(100000), 'gasPrice': hex(10**10)}
        overlay = {'target': target, 'tx': txhash}
        overlay['latest_balance'] = await result(rpc, 'eth_getBalance', [target, 'latest'])
        overlay['pending_balance'] = await result(rpc, 'eth_getBalance', [target, 'pending'])
        overlay['pending_call'] = await result(rpc, 'eth_call', [req, 'pending'])
        overlay['pending_estimate'] = await result(rpc, 'eth_estimateGas', [req, 'pending'])
        overlay['latest_balance_after'] = await result(rpc, 'eth_getBalance', [target, 'latest'])
        out['pending_overlay'] = overlay
        ROOT.joinpath(f'{MODE}-live-results.json').write_text(json.dumps(out, indent=2)+'\n')
        assert overlay['latest_balance']['result'] == '0x0', overlay
        assert overlay['latest_balance_after']['result'] == '0x0', overlay
        assert int(overlay['pending_balance']['result'], 16) == (10**9-ed)*10**9, overlay
        if MODE == 'fixed':
            assert 'result' in overlay['pending_call'], overlay
            assert int(overlay['pending_estimate']['result'], 16) >= 21000, overlay
        else:
            assert 'error' in overlay['pending_call'] and 'error' in overlay['pending_estimate'], overlay
        overlay['finalized'] = await finalized_balance(c, dest, 10**9)

        below_ed_req = {**req, 'value': hex(10**9)}
        out['existential_deposit_control'] = {
            'below_ed_latest_call': await result(rpc, 'eth_call', [below_ed_req, 'latest']),
            'valid_latest_call': await result(rpc, 'eth_call', [req, 'latest']),
        }
        assert 'OutOfFund' in out['existential_deposit_control']['below_ed_latest_call'].get('error', '')
        assert 'result' in out['existential_deposit_control']['valid_latest_call']

        if MODE == 'fixed':
            # Final simulations must not persist changes to their pending base;
            # getters return valid empty state, and overrides never silently
            # discard that base to execute against an unrelated state.
            before = await rpc('eth_getBalance', [target, 'latest'])
            checks = {}
            for i in range(2):
                checks[f'call_{i}'] = await result(rpc, 'eth_call', [req, 'pending'])
                checks[f'estimate_{i}'] = await result(rpc, 'eth_estimateGas', [req, 'pending'])
                assert 'result' in checks[f'call_{i}'], checks
                assert int(checks[f'estimate_{i}']['result'], 16) >= 21000, checks
            checks['pending_code'] = await result(rpc, 'eth_getCode', [target, 'pending'])
            checks['pending_storage'] = await result(rpc, 'eth_getStorageAt', [target, '0x0', 'pending'])
            assert checks['pending_code']['result'] == '0x', checks
            assert int(checks['pending_storage']['result'], 16) == 0, checks
            override_req = {'from': target, 'to': '0x'+'14'*20, 'gas': hex(100000)}
            overrides = {override_req['to']: {'code': '0x602a60005260206000f3'}}
            checks['latest_override'] = await result(rpc, 'eth_call', [override_req, 'latest', overrides])
            assert int(checks['latest_override']['result'], 16) == 42, checks
            checks['pending_override'] = await result(rpc, 'eth_call', [override_req, 'pending', overrides])
            assert 'state overrides are not supported with pending state' in checks['pending_override'].get('error', ''), checks
            after = await rpc('eth_getBalance', [target, 'latest'])
            assert before == after, (before, after)
            assert await rpc('eth_getTransactionCount', [target, 'latest']) == '0x0'
            checks['latest_balance_before_after'] = [before, after]
            out['read_and_simulation_semantics'] = checks

        # A healthy pending query stays available under an unrelated sustained
        # flood. Direct callers of the pending export do not receive a lease.
        await asyncio.sleep(2.5)
        flood = []
        stop = threading.Event()
        def worker():
            while not stop.is_set():
                flood.append(http('state_call', ['MldsaAdmissionProbe_unexported', '0x']))
                time.sleep(.02)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            jobs = [pool.submit(worker) for _ in range(4)]
            await asyncio.sleep(.35)
            try:
                direct = await result(rpc, 'state_call', ['EthereumRuntimeRPCApi_initialize_pending_block', '0x'])
                pending = await result(rpc, 'eth_getBalance', [target, 'pending'])
                latest = await result(rpc, 'eth_getBalance', [target, 'latest'])
            finally:
                stop.set()
                for job in jobs:
                    job.result()
        out['isolation'] = {'unknown_requests': len(flood), 'capacity_rejections': sum(MARKER in json.dumps(x) for x in flood), 'direct_pending_export': direct, 'pending_balance': pending, 'latest_balance': latest}
        assert out['isolation']['capacity_rejections'] > 0
        assert MARKER in direct.get('error', ''), direct
        assert int(latest['result'], 16) == (10**9-ed)*10**9, latest
        if MODE == 'fixed':
            assert int(pending['result'], 16) == (10**9-ed)*10**9, pending
        else:
            assert MARKER in pending.get('error', ''), pending

        # 32 real ready transactions exceed both the previous burst and the new
        # pending burst. Successful replies must include the complete balance.
        await asyncio.sleep(2.5)
        target = '0x' + os.urandom(20).hex()
        dest = h160_to_ss58(target)
        call = await c.compose(bt.calls.Balances.transfer_keep_alive(dest=dest, value=10**9))
        nonce = int(await rpc('system_accountNextIndex', [bob.ss58_address]))
        txs = [await raw.create_signed_extrinsic(call, bob, nonce=nonce+i, era='00') for i in range(32)]
        await next_block(rpc)
        hashes = await asyncio.gather(*(rpc('author_submitExtrinsic', [tx.data_hex]) for tx in txs))
        ready = await rpc('author_pendingExtrinsics', [])
        assert len(ready) >= 32, len(ready)
        pending = await result(rpc, 'eth_getBalance', [target, 'pending'])
        out['populated_pool'] = {'target': target, 'txs': hashes, 'pool_count': len(ready), 'pending_balance': pending}
        ROOT.joinpath(f'{MODE}-live-results.json').write_text(json.dumps(out, indent=2)+'\n')
        if MODE == 'fixed':
            assert int(pending['result'], 16) == (32*10**9-ed)*10**9, pending
        else:
            assert MARKER in pending.get('error', ''), pending
        out['populated_pool']['finalized'] = await finalized_balance(c, dest, 32*10**9)
        out['status'] = 'passed'
        ROOT.joinpath(f'{MODE}-live-results.json').write_text(json.dumps(out, indent=2)+'\n')
        print(json.dumps(out, indent=2))

asyncio.run(main())
