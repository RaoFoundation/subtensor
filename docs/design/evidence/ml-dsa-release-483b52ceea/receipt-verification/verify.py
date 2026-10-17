"""Read-only canonical receipt audit for the eight disposable validation chains.

Run repeatedly while suites append evidence, and once after their completion.
Only receipt directories are scanned: copied source, activation transactions,
old verifier output, and repeated verification counts are never inputs.
"""

import argparse
import asyncio
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re

import bittensor as bt
from bittensor._transport.extrinsics import resolve_outcome

ROOT = Path('/tmp/ms-release-483b52ceea')
SUITES = {
    'release': 'release', 'authority': 'authority', 'operations': 'operations',
    'validators': 'validators', 'lifecycle': 'lifecycle', 'parity': 'parity',
    'extended': 'extended', 'nested': 'nested', 'sponsorship': 'sponsorship',
    'proxy-boundaries': 'proxy-boundaries',
    'validator-proxy': 'validator-proxy',
}
if selected := os.getenv('VERIFY_CHAINS'):
    SUITES = {suite: chain for suite, chain in SUITES.items() if chain in selected.split(',')}

ID = re.compile(r'^(\d+)-(\d+)$')
HASH = re.compile(r'^0x[0-9a-fA-F]{64}$')


def normalized(value):
    return json.loads(json.dumps(value, default=str))


def fingerprint(value):
    return hashlib.sha256(json.dumps(normalized(value), sort_keys=True,
                                    separators=(',', ':')).encode()).hexdigest()


def walk(value, pointer='$', label=None):
    if isinstance(value, dict):
        label = value.get('operation', value.get('step', label))
        if isinstance(value.get('success'), bool):
            match = ID.fullmatch(str(value.get('extrinsic_id', '')))
            block_hash = value.get('block_hash')
            if block_hash is not None or value.get('extrinsic_id') is not None:
                assert match and isinstance(block_hash, str) and HASH.fullmatch(block_hash), (
                    f'incomplete or malformed inclusion locator at {pointer}'
                )
            if match and isinstance(block_hash, str) and HASH.fullmatch(block_hash):
                yield value, pointer, label, tuple(map(int, match.groups()))
        for key, child in value.items():
            yield from walk(child, pointer + '.' + str(key), label)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk(child, pointer + f'[{index}]', label)


def independent_status(events):
    """Interpret canonical System/wrapper events independently of receipt.success."""
    system, inner = [], []
    wrappers = {
        ('Sudo', 'Sudid'): 'sudo_result', ('Sudo', 'SudoAsDone'): 'sudo_result',
        ('Proxy', 'ProxyExecuted'): 'result',
        ('Multisig', 'MultisigExecuted'): 'result',
    }
    for entry in events:
        event = entry['event']
        key = event['module_id'], event['event_id']
        attrs = event['attributes']
        if key in [('System', 'ExtrinsicSuccess'), ('System', 'ExtrinsicFailed')]:
            system.append(event)
        if key in wrappers:
            result = attrs.get(wrappers[key]) if isinstance(attrs, dict) else attrs
            if isinstance(result, dict) and 'Err' in result:
                inner.append({'event': '.'.join(key), 'error': result['Err']})
        elif key in [('Utility', 'BatchInterrupted'), ('Utility', 'ItemFailed')]:
            inner.append({'event': '.'.join(key), 'error': attrs['error']})
        elif key[0] == 'MevShield' and key[1] in {
            'DecryptedRejected', 'DecryptionFailed', 'ExtrinsicDispatchFailed',
            'ExtrinsicDecodeFailed', 'ExtrinsicExpired',
        }:
            inner.append({'event': '.'.join(key), 'error': attrs})
    assert len(system) == 1, f'expected exactly one System outcome, got {system}'
    outer_success = system[0]['event_id'] == 'ExtrinsicSuccess'
    classification = ('success' if not inner else 'outer_success_inner_failure') if outer_success else 'system_failure'
    return outer_success and not inner, classification, system[0], inner


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--label', default='latest')
    args = parser.parse_args()
    assert re.fullmatch(r'[a-zA-Z0-9_-]+', args.label)
    output_dir = ROOT / 'receipt-verification'
    output_dir.mkdir(exist_ok=True)
    report = {
        'candidate': '483b52ceea5cf08a0fb492ca2915b7efa6a8690e',
        'started_utc': datetime.now(timezone.utc).isoformat(),
        'scope': 'Persisted suite receipt files; activation extrinsics excluded. This verifies recorded outcomes, not completeness of operation coverage or all test assertions.',
        'deduplication_key': '(local chain name, block hash, extrinsic index)',
        'files': [], 'chains': {}, 'receipts': [], 'errors': [], 'suites': {},
    }
    configs = {row['name']: row for row in json.loads((ROOT / 'chains.json').read_text())}
    grouped = defaultdict(dict)
    raw_count = 0
    for suite, chain in SUITES.items():
        files = sorted((ROOT / suite).glob('**/receipts/*.json'))
        report['suites'][suite] = {'chain': chain, 'files': len(files), 'receipt_references': 0}
        for path in files:
            relative = str(path.relative_to(ROOT))
            try:
                # Evidence writers replace full JSON documents after each receipt.
                # A short retry handles reading during that write without ignoring it.
                for attempt in range(3):
                    try:
                        content = path.read_bytes()
                        document = json.loads(content)
                        break
                    except json.JSONDecodeError:
                        if attempt == 2:
                            raise
                        await asyncio.sleep(0.1)
                values = list(walk(document))
                report['files'].append({'path': relative, 'sha256': hashlib.sha256(content).hexdigest(),
                                        'receipt_references': len(values),
                                        'declared_successful': document.get('successful') if isinstance(document, dict) else None})
                for receipt, pointer, label, (height, index) in values:
                    raw_count += 1
                    report['suites'][suite]['receipt_references'] += 1
                    key = receipt['block_hash'].lower(), index
                    row = grouped[chain].setdefault(key, {
                        'height': height, 'index': index, 'block_hash': key[0],
                        'recorded_success': receipt['success'], 'references': [],
                        'recorded_events': [],
                    })
                    assert row['height'] == height and row['recorded_success'] == receipt['success'], 'conflicting duplicate receipt'
                    row['references'].append({'path': relative, 'pointer': pointer, 'operation': label})
                    if 'canonical_events' in receipt:
                        row['recorded_events'].append(receipt['canonical_events'])
            except Exception as exc:
                report['errors'].append({'file': relative, 'error': repr(exc)})

    semaphore = asyncio.Semaphore(4)

    async def verify_chain(name):
        rows = grouped[name]
        endpoint = f"ws://127.0.0.1:{configs[name]['rpc_port']}"
        summary = {'endpoint': endpoint, 'unique_receipts': len(rows), 'verified_receipts': 0,
                   'status_counts': {}, 'errors': []}
        report['chains'][name] = summary
        try:
            async with semaphore, bt.Client(endpoint, fallback_endpoints=[], archive_endpoints=[]) as client:
                raw = client._substrate.raw
                rpc = raw._session.request
                summary['genesis'] = await rpc('chain_getBlockHash', [0])
                final_hash = await raw.get_chain_finalised_head()
                final_height = await raw.get_block_number(final_hash)
                summary['finalized_snapshot'] = {'height': final_height, 'hash': final_hash}
                per_block = defaultdict(list)
                for row in rows.values():
                    per_block[row['block_hash']].append(row)
                counts = Counter()
                for block_hash, block_rows in sorted(per_block.items(), key=lambda p: p[1][0]['height']):
                    try:
                        height = block_rows[0]['height']
                        assert height <= final_height, f'height {height} above finalized snapshot {final_height}'
                        assert await rpc('chain_getBlockHash', [height]) == block_hash, 'not canonical block hash'
                        block = (await rpc('chain_getBlock', [block_hash]))['block']
                        assert int(block['header']['number'], 16) == height, 'header height mismatch'
                        events = await raw.get_events(block_hash)
                        codec = await raw._runtimes.codec_at(block_hash)
                        for row in sorted(block_rows, key=lambda r: r['index']):
                            try:
                                index = row['index']
                                assert row['height'] == height
                                encoded = bytes.fromhex(block['extrinsics'][index].removeprefix('0x'))
                                triggered = [event for event in events if event.get('extrinsic_idx') == index]
                                success, status, system, inner = independent_status(triggered)
                                assert success == row['recorded_success'], 'canonical event status disagrees with receipt'
                                outcome = resolve_outcome(triggered, codec)
                                assert outcome['is_success'] == success, 'SDK outcome disagrees with independent event interpretation'
                                for recorded in row['recorded_events']:
                                    assert normalized(triggered) == recorded, 'recorded canonical events differ'
                                report['receipts'].append({
                                    'chain': name, 'height': height, 'extrinsic_index': index,
                                    'block_hash': block_hash,
                                    'extrinsic_hash': '0x' + hashlib.blake2b(encoded, digest_size=32).hexdigest(),
                                    'canonical_finalized': True, 'recorded_success': row['recorded_success'],
                                    'status': status, 'system_event': system['event_id'],
                                    'inner_failures': normalized(inner),
                                    'resolved_error': normalized(outcome['error_message']),
                                    'event_count': len(triggered), 'events_sha256': fingerprint(triggered),
                                    'references': row['references'],
                                })
                                summary['verified_receipts'] += 1
                                counts[status] += 1
                            except Exception as exc:
                                summary['errors'].append({'block_hash': block_hash, 'index': row['index'],
                                                          'references': row['references'], 'error': repr(exc)})
                    except Exception as exc:
                        summary['errors'].append({'block_hash': block_hash, 'height': block_rows[0]['height'], 'error': repr(exc)})
                summary['status_counts'] = dict(counts)
        except Exception as exc:
            summary['errors'].append({'error': repr(exc)})
        print(json.dumps({'chain': name, 'verified': summary['verified_receipts'], 'unique': len(rows),
                          'status_counts': summary['status_counts'], 'errors': summary['errors']}), flush=True)

    await asyncio.gather(*(verify_chain(name) for name in sorted(set(SUITES.values()))))
    report['receipts'].sort(key=lambda row: (row['chain'], row['height'], row['extrinsic_index']))
    report['finished_utc'] = datetime.now(timezone.utc).isoformat()
    report['raw_receipt_references'] = raw_count
    report['unique_receipts'] = sum(len(rows) for rows in grouped.values())
    report['verified_receipts'] = len(report['receipts'])
    report['deduplicated_references'] = raw_count - report['unique_receipts']
    report['status_counts'] = dict(Counter(row['status'] for row in report['receipts']))
    report['all_recorded_receipts_verified'] = (not report['errors'] and
        all(not chain['errors'] for chain in report['chains'].values()) and
        report['verified_receipts'] == report['unique_receipts'])
    report['suites_without_receipt_evidence'] = [name for name, row in report['suites'].items() if not row['receipt_references']]
    (output_dir / f'{args.label}.json').write_text(json.dumps(report, indent=2) + '\n')
    brief = {key: report[key] for key in ['finished_utc', 'raw_receipt_references', 'unique_receipts',
        'verified_receipts', 'deduplicated_references', 'status_counts',
        'all_recorded_receipts_verified', 'suites_without_receipt_evidence']}
    (output_dir / f'{args.label}-summary.json').write_text(json.dumps(brief, indent=2) + '\n')
    print(json.dumps(brief), flush=True)
    if not report['all_recorded_receipts_verified']:
        raise SystemExit(1)


asyncio.run(main())
