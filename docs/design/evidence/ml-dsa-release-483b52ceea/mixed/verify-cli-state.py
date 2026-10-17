"""Read public mixed-CLI fixture metadata and verify finalized chain state.

Usage: PYTHONPATH=sdk/python sdk/python/.venv/bin/python SCRIPT MODE
Reads coldkeypub.txt and multisig cache only; never opens a secret key file.
"""

import asyncio
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

import bittensor as bt

RUN = Path('/tmp/ms-release-483b52ceea/mixed')
FIXTURES = Path('/tmp/pytest-of-johnreed/pytest-328/popen-gw1')
MODE = int(sys.argv[1])
assert MODE in (0, 1)


async def main():
    fixture = FIXTURES / f'test_cli_mixed_two_of_three_al{MODE}'
    public = {}
    source_hashes = {}
    for name in ('rotating', 'sr', 'ed'):
        path = fixture / 'wallets' / name / 'coldkeypub.txt'
        data = json.loads(path.read_text())
        public[name] = {k: data[k] for k in ('ss58Address', 'cryptoType', 'publicKey')}
        source_hashes[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    cache_path = fixture / 'multisig_cache.json'
    cache = next(iter(json.loads(cache_path.read_text()).values()))
    source_hashes[str(cache_path)] = hashlib.sha256(cache_path.read_bytes()).hexdigest()
    group = cache['multisig_address']
    recipient = next(
        a['value'] for call in cache['params']['calls']
        if call['call_module'] == 'Balances'
        for a in call['call_args'] if a['name'] == 'dest'
    )
    expected_codes = (6, 1, 0) if MODE == 0 else (0, 4, 6)
    assert tuple(p['cryptoType'] for p in public.values()) == expected_codes
    async with bt.Client('ws://127.0.0.1:21944', fallback_endpoints=[], archive_endpoints=[]) as client:
        raw = client._substrate.raw
        rpc = raw._session.request
        final_hash = await rpc('chain_getFinalizedHead', [])
        height = int((await rpc('chain_getHeader', [final_hash]))['number'], 16)
        assert await rpc('chain_getBlockHash', [height]) == final_hash
        codec = await raw._runtimes.codec_at(final_hash)

        async def read(address):
            return {
                'address': address,
                'account': await client._substrate.query('System', 'Account', [address], block_hash=final_hash),
                'hashed': await client._substrate.query('HashedAccounts', 'Accounts', [address], block_hash=final_hash),
                'proxies': await client._substrate.query('Proxy', 'Proxies', [address], block_hash=final_hash),
            }

        states = {name: await read(p['ss58Address']) for name, p in public.items()}
        group_state, recipient_state = await read(group), await read(recipient)
        pending = await client._substrate.query_map('Multisig', 'Multisigs', [group], block_hash=final_hash)
        assert not pending
        assert group_state['account']['data']['free'] == 12 * 10**9
        assert group_state['account']['data']['reserved'] == 0
        assert group_state['account']['nonce'] == 0
        assert recipient_state['account']['data']['free'] == (8 + MODE) * 10**9
        assert recipient_state['account']['data']['reserved'] == 0
        assert recipient_state['hashed']['generation'] == 0
        expected_nonces = (11, 7, 4) if MODE == 0 else (11, 9, 5)
        expected_generations = (10, None, None) if MODE == 0 else (None, 8, 4)
        proxy_deposit = int(await client.constant(bt.constants.Proxy.ProxyDepositBase)) + int(await client.constant(bt.constants.Proxy.ProxyDepositFactor))
        for index, (name, state) in enumerate(states.items()):
            assert state['account']['nonce'] == expected_nonces[index], (name, state)
            assert (state['hashed']['generation'] if state['hashed'] else None) == expected_generations[index]
            expected_reserve = proxy_deposit if MODE == 1 and name == 'rotating' else 0
            assert state['account']['data']['reserved'] == expected_reserve
            assert state['proxies'][1] == expected_reserve
            if expected_reserve:
                assert state['proxies'][0] == [{'delegate': group, 'proxy_type': 'Any', 'delay': 0}]
            else:
                assert state['proxies'][0] == []

        # Discover the evidence from this run; never reuse old block numbers.
        proof = []
        identifiers = [p['publicKey'].removeprefix('0x') for p in public.values()]
        for start in range(1, height + 1, 12):
            ns = list(range(start, min(height + 1, start + 12)))
            hashes = await raw._session.request_batch([('chain_getBlockHash', [n]) for n in ns])
            blocks = await raw._session.request_batch([('chain_getBlock', [h]) for h in hashes])
            for number, block_hash, block in zip(ns, hashes, blocks):
                for index, wire in enumerate(block['block']['extrinsics']):
                    if not any(identity in wire for identity in identifiers):
                        continue
                    decoded = codec.decode_extrinsic(wire)
                    call = decoded['call']
                    if call['call_module'] not in ('Proxy', 'Multisig', 'Utility', 'HashedAccounts'):
                        continue
                    tx_hash = '0x' + hashlib.blake2b(bytes.fromhex(wire[2:]), digest_size=32).hexdigest()
                    report = await raw.resolve_extrinsic(tx_hash, block_hash, finalized=True)
                    events = [e['event'] for e in report.triggered_events if e['event']['module_id'] in ('Proxy', 'Multisig', 'System', 'Balances', 'HashedAccounts', 'Utility')]
                    # Keep only this fixture's multisig events or explicit proxy grant.
                    relevant = any(e['module_id'] == 'Multisig' and group in json.dumps(e) for e in events)
                    grant = call['call_module'] == 'Proxy' and call['call_function'] == 'add_proxy' and group in json.dumps(call)
                    if not (relevant or grant):
                        continue
                    proof.append({'block_number': number, 'block_hash': block_hash, 'extrinsic_index': index, 'extrinsic_hash': tx_hash, 'call': f"{call['call_module']}.{call['call_function']}", 'nonce': decoded.get('nonce'), 'success': report.is_success, 'error': report.error_message, 'events': events})
        failures = [r for r in proof if any(e['module_id'] == 'Multisig' and e['event_id'] == 'MultisigExecuted' and 'Err' in e['attributes']['result'] for e in r['events'])]
        assert len(failures) == 1, failures
        assert any(r['block_number'] > failures[0]['block_number'] and r['success'] and any(e['module_id'] == 'Multisig' and e['event_id'] == 'MultisigExecuted' and 'Ok' in e['attributes']['result'] for e in r['events']) for r in proof)
        grants = [r for r in proof if r['call'] == 'Proxy.add_proxy']
        assert len(grants) == 1, grants
        if MODE == 0:
            assert not grants[0]['success']
            assert grants[0]['error']['name'] == 'AccountPolicyViolation'
        else:
            assert grants[0]['success']
            assert any(e['module_id'] == 'Proxy' and e['event_id'] == 'ProxyExecuted' and 'Ok' in e['attributes']['result'] for r in proof for e in r['events'])

        result = {
            'mode': MODE, 'verified': True, 'captured_at_utc': datetime.now(timezone.utc).isoformat(),
            'anchor': {'block_number': height, 'block_hash': final_hash},
            'genesis': await rpc('chain_getBlockHash', [0]),
            'runtime': await rpc('state_getRuntimeVersion', [final_hash]),
            'public_fixture_sha256': source_hashes, 'public_members': public,
            'account_states': {'members': states, 'multisig': group_state, 'recipient': recipient_state},
            'pending_multisigs': pending, 'event_proof': proof,
            'proof_blocks': [row['block_number'] for row in proof],
            'assertions': {'final_balances_match_test': True, 'all_temporary_multisig_deposits_released': True, 'member_nonces_and_hashed_generations_exact': True, 'only_mode1_permanent_proxy_deposit_remains': True},
        }
        output_path = RUN / 'cli-state-verification.json'
        output = json.loads(output_path.read_text()) if output_path.exists() else {'cases': {}}
        cases = {str(c['mode']): c for c in output['cases']} if isinstance(output['cases'], list) else output['cases']
        cases[str(MODE)] = result
        output['cases'] = [cases[key] for key in sorted(cases)]
        output['candidate'] = '483b52ceea5cf08a0fb492ca2915b7efa6a8690e'
        output['all_verified'] = set(cases) == {'0', '1'} and all(c['verified'] for c in cases.values())
        output.pop('complete', None)
        output['verifier_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        output['scope'] = 'Read-only public finalized state and events from a normal-slot local chain with committed production reference weights; no secret-key reads or signing.'
        output_path.write_text(json.dumps(output, indent=2) + '\n')
        print(json.dumps({'mode': MODE, 'verified': True, 'finalized_height': height, 'evidence_sha256': hashlib.sha256(output_path.read_bytes()).hexdigest(), 'proof_blocks': [row['block_number'] for row in proof]}))


asyncio.run(asyncio.wait_for(main(), 300))
