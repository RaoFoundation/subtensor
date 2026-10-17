"""Independently check the final stored authority and weights of both new cases."""
import asyncio
import hashlib
import json
from pathlib import Path

import bittensor as bt

ROOT = Path('/tmp/ms-release-483b52ceea/validator-proxy')


async def main():
    result = {'candidate': '483b52ceea5cf08a0fb492ca2915b7efa6a8690e', 'cases': []}
    async with bt.Client('ws://127.0.0.1:22344', fallback_endpoints=[], archive_endpoints=[]) as client:
        raw = client._substrate.raw
        rpc = raw._session.request
        final_hash = await raw.get_chain_finalised_head()
        result['finalized_snapshot'] = {'height': await raw.get_block_number(final_hash), 'hash': final_hash}
        for code in (7, 8):
            path = ROOT / 'receipts' / f'ms-validator-proxy-{code}.json'
            data = path.read_bytes()
            case = json.loads(data)
            assert case['successful'] is True
            terminal = case['receipts'][-1]
            height, index = map(int, terminal['extrinsic_id'].split('-'))
            assert height <= result['finalized_snapshot']['height']
            assert await rpc('chain_getBlockHash', [height]) == terminal['block_hash']
            assert terminal['success'] is False and terminal['error']['name'] == 'NotProxy'
            block = terminal['block_hash']
            weights = await client._substrate.query('SubtensorModule', 'Weights',
                [case['netuid'], case['uids']['validator']], block_hash=block)
            assert weights == [(case['uids']['miner'], 65535)]
            assert case['uids']['miner'] != case['uids']['validator'] != case['uids']['alternate']
            states = {}
            for role, generation, recorded in [('validator', 2, 'principal_after_revoke'),
                                               ('delegate', 3, 'delegate_final')]:
                address = case['accounts'][role]
                funding = next(item for item in case['receipts']
                    if item.get('data', {}).get('hashed_registration') == address)
                baseline_account = await client._substrate.query('System', 'Account',
                    [address], block_hash=funding['block_hash'])
                baseline_authority = await client._substrate.query('HashedAccounts', 'MlDsaAccounts',
                    [address], block_hash=funding['block_hash'])
                assert baseline_authority['generation'] == 0
                state = {
                    'account': await client._substrate.query('System', 'Account', [address], block_hash=block),
                    'authority': await client._substrate.query('HashedAccounts', 'MlDsaAccounts', [address], block_hash=block),
                }
                assert state['account']['nonce'] == baseline_account['nonce'] + generation
                assert state['authority']['generation'] == baseline_authority['generation'] + generation
                assert state['account']['data']['reserved'] == 0
                assert json.loads(json.dumps(state)) == case[recorded]
                states[role] = {'address': address, 'initial_nonce': baseline_account['nonce'],
                    'nonce': state['account']['nonce'], 'generation': generation}
            proxies = await client._substrate.query('Proxy', 'Proxies', [case['accounts']['validator']], block_hash=block)
            assert not proxies[0]
            result['cases'].append({'crypto_type': code, 'evidence_sha256': hashlib.sha256(data).hexdigest(),
                'netuid': case['netuid'], 'terminal_extrinsic_id': terminal['extrinsic_id'],
                'terminal_block_hash': block, 'weights': weights, 'authorities': states,
                'proxy_removed': True, 'verified': True})
    result['all_verified'] = True
    (ROOT / 'state-verification.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(result['cases']), 'all_verified': True}))


asyncio.run(main())
