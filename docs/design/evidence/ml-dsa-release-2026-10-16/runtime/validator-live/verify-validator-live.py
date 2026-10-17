from pathlib import Path
import asyncio
import datetime
import hashlib
import json

import bittensor as bt

BASE = Path('/tmp/ms-release-validation')


def errors(value):
    if isinstance(value, dict):
        if 'Err' in value:
            yield value['Err']
        for item in value.values():
            yield from errors(item)
    elif isinstance(value, (list, tuple)):
        for item in value:
            yield from errors(item)


def chain_bytes(value):
    if isinstance(value, str):
        return bytes.fromhex(value[2:]) if value.startswith('0x') else value.encode()
    return bytes(value)


async def main():
    result = {'verified_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'runs': {}}
    async with bt.Client('ws://127.0.0.1:20244', fallback_endpoints=[], archive_endpoints=[]) as client:
        rpc = client._substrate.raw.rpc_request
        final_hash = await rpc('chain_getFinalizedHead', [])
        final_number = int((await rpc('chain_getHeader', [final_hash]))['number'], 16)
        result.update(finalized_block=final_hash, finalized_number=final_number)
        for folder in ('validator-live', 'validator-live-corrected'):
            runs = []
            for path in sorted((BASE / folder).glob('*.json')):
                report = json.loads(path.read_text())
                if folder.endswith('corrected'):
                    assert report['successful'], path
                    assert len(report['receipts']) == 14, report['receipts']
                receipts = []
                for receipt in report['receipts']:
                    assert receipt['success'], receipt
                    number, index = map(int, receipt['extrinsic_id'].split('-'))
                    assert number <= final_number
                    block_hash = await rpc('chain_getBlockHash', [number])
                    assert block_hash == receipt['block_hash'], receipt
                    body = await rpc('chain_getBlock', [block_hash])
                    encoded = bytes.fromhex(body['block']['extrinsics'][index][2:])
                    events = await client._substrate.query('System', 'Events', [], block_hash=block_hash)
                    matching = [event for event in events if event.get('extrinsic_idx') == index]
                    assert any(event['module_id'] == 'System' and event['event_id'] == 'ExtrinsicSuccess' for event in matching), matching
                    assert not any(event['module_id'] == 'System' and event['event_id'] == 'ExtrinsicFailed' for event in matching), matching
                    assert not list(errors(matching)), matching
                    receipts.append({'operation': receipt['operation'], 'block_hash': block_hash, 'extrinsic_id': receipt['extrinsic_id'], 'extrinsic_blake2_256': '0x' + hashlib.blake2b(encoded, digest_size=32).hexdigest(), 'system_extrinsic_success': True, 'nested_results_all_success': True, 'events': [event['module_id'] + '.' + event['event_id'] for event in matching]})
                params = [report['netuid'], report['accounts']['validator']]
                revealed = await client._substrate.query('Commitments', 'RevealedCommitments', params, block_hash=final_hash)
                plaintext = f"MS timelock commitment {report['crypto_type']}".encode()
                assert any(chain_bytes(value) == plaintext and int(at) > 0 for value, at in revealed or []), revealed
                final_weights = await client._substrate.query('SubtensorModule', 'Weights', [report['netuid'], report['validator_uid']], block_hash=final_hash)
                commits = await client._substrate.query('SubtensorModule', 'WeightCommits', params, block_hash=final_hash)
                if folder.endswith('corrected'):
                    assert final_weights == [(report['miner_uid'], 65535)], final_weights
                    assert not commits, commits
                runs.append({'crypto_type': report['crypto_type'], 'netuid': report['netuid'], 'report_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'receipts': receipts, 'revealed_plaintext': plaintext.decode(), 'revealed_metadata': revealed, 'final_weights': final_weights, 'remaining_legacy_commits': commits})
            assert len(runs) == 2
            result['runs'][folder] = runs
    result['all_verified'] = True
    (BASE / 'validator-live-post-verification.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'all_verified': True, 'receipts': sum(len(run['receipts']) for runs in result['runs'].values() for run in runs), 'finalized_number': final_number}))


asyncio.run(main())
