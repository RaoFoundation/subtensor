import asyncio
import json
from pathlib import Path

import bittensor as bt
from bittensor.sp_core import ss58_decode, ss58_encode
from tests.e2e.test_mldsa_localnet import derivative_account_id


async def main():
    output = []
    root = Path('/tmp/ms-release-validation')
    async with bt.Client('ws://127.0.0.1:20444', fallback_endpoints=[], archive_endpoints=[]) as client:
        raw = client._substrate.raw
        final_hash = await raw._session.request('chain_getFinalizedHead', [])
        final_header = await raw._session.request('chain_getHeader', [final_hash])
        final_height = int(final_header['number'], 16)
        for code in (7, 8):
            receipts = json.loads((root / f'mldsa-evidence/mldsa-authority-{code}.json').read_text())
            offline = next(item for item in receipts if 'offline_block' in item)
            codec = await raw._runtimes.codec_at(offline['offline_block'])
            decoded = codec.decode_extrinsic(offline['extrinsic'])
            # Registered signed-v4's Raw carrier starts with the stable AccountId32.
            parent = bytes.fromhex(decoded['address']['Raw'][2:])[:32]
            failed_index = next(index for index, item in enumerate(receipts) if item.get('success') is False)
            failed = receipts[failed_index]
            height = int(failed['extrinsic_id'].split('-')[0])
            assert height <= final_height
            assert await raw.get_block_hash(height) == failed['block_hash']
            success = receipts[failed_index - 1]
            good_child = derivative_account_id(parent, 7)
            successful = await raw.query('HashedAccounts', 'DerivedAuthorities', [good_child], success['block_hash'])
            assert successful is not None
            child = derivative_account_id(parent, 8)
            alias = '0x' + bytes(ss58_decode(child))[:20].hex()
            authority = await raw.query('HashedAccounts', 'DerivedAuthorities', [child], failed['block_hash'])
            evm_alias = await raw.query('HashedAccounts', 'EvmAliases', [alias], failed['block_hash'])
            account = await raw.query('System', 'Account', [child], failed['block_hash'])
            assert authority is None and evm_alias is None
            assert account['nonce'] == 0 and account['data']['free'] == 0 and account['data']['reserved'] == 0
            output.append({
                'code': code,
                'parent': ss58_encode(parent, 42),
                'successful_derivative': good_child,
                'successful_authority_at_inclusion': successful,
                'failed_derivative': child,
                'failed_alias': alias,
                'failure_block': failed['block_hash'],
                'failure_extrinsic': failed['extrinsic_id'],
                'finalized_ancestor_verified': True,
                'failed_authority_at_inclusion': authority,
                'failed_alias_at_inclusion': evm_alias,
                'failed_account_at_inclusion': account,
            })
        result = {'finalized_hash': final_hash, 'finalized_height': final_height, 'cases': output}
        (root / 'mldsa-evidence/derived-rollback-postcheck.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps({'passed_codes': [item['code'] for item in output], 'finalized_height': final_height}))


asyncio.run(main())
