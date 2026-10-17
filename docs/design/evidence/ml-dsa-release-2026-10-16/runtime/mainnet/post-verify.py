#!/usr/bin/env python3
"""Supplemental read-only evidence verification; no RPC, runtime execution, or Cargo."""
import datetime
import hashlib
import importlib.util
import json
import re
import sys
from pathlib import Path

sys.dont_write_bytecode = True
root = Path('/home/johnreed/Desktop/work/subtensor')
base = Path('/tmp/ms-release-validation/mainnet')
checker_path = root / 'docs/design/evidence/ml-dsa-mainnet-log-check.py'
spec = importlib.util.spec_from_file_location('unchanged_log_checker', checker_path)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)
require = checker.require

def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()

result_path = base / 'ml-dsa-mainnet-upgrade-results.json'
raw = json.loads(result_path.read_text())
source_path = base / 'source.json'
source = json.loads(source_path.read_text())
require(raw['successful'] is True, 'Raw runner did not pass')
require(raw['source'] == source, 'Source record differs')
require(json.loads((base / 'read-source.log').read_text()) == source, 'RPC source log differs')
require(source['genesis_hash'] == '0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03', 'Unexpected chain genesis')
paths = {
    'runner': (root / 'docs/design/evidence/ml-dsa-mainnet-upgrade.py', 'script_sha256', None),
    'source_record': (source_path, 'source_json_sha256', None),
    'snapshot': (base / 'mainnet.snap', 'snapshot_sha256', 'snapshot_size_bytes'),
    'runtime_wasm': (base / 'runtime.wasm', 'runtime_wasm_sha256', 'runtime_wasm_size_bytes'),
    'try_runtime_cli': (Path('/tmp/hashed-full-ci-_as7hfjj/bin/try-runtime.real'), 'try_runtime_sha256', None),
}
artifacts = {}
for name, (path, hash_field, size_field) in paths.items():
    actual = sha256(path)
    require(actual == raw[hash_field], f'{name} digest differs')
    size = path.stat().st_size
    if size_field:
        require(size == raw[size_field], f'{name} size differs')
    artifacts[name] = {'path': str(path), 'sha256': actual, 'size_bytes': size, 'digest_matches_raw_record': True}
require([run['name'] for run in raw['runs']] == ['on-runtime-upgrade', 'fast-forward'], 'Unexpected run sequence')
logs = {}
runs = []
for run in raw['runs']:
    path = base / run['log_file']
    require(run['exit_code'] == 0, f"Nonzero exit: {run['name']}")
    require(sha256(path) == run['log_sha256'], 'Execution log digest differs')
    text = path.read_text()
    loaded_root = checker.one_match(checker.ROOT_PATTERN, text)
    require(loaded_root == source['state_root'], 'Loaded root differs from RPC-pinned root')
    versions = re.findall(r'runtime\s+\[Name: "([^"]+)"\] \[Version: ([0-9]+)\]', text)
    require(len(versions) == 2, 'Missing source/candidate runtime versions')
    require(all(name == source['source_spec_name'] for name, _ in versions), 'Spec name differs')
    require([int(v) for _, v in versions] == [479, 480], 'Unexpected upgrade versions')
    logs[run['name']] = text
    runs.append({'name': run['name'], 'exit_code': 0, 'log_sha256': sha256(path), 'log_digest_matches_record': True, 'loaded_snapshot_state_root': loaded_root, 'loaded_root_matches_rpc_pinned_header': True, 'runtime_versions': [479, 480]})
for marker in ('Running TryRuntime_on_runtime_upgrade with checks: All', 'Running TryRuntime_on_runtime_upgrade again to check idempotency: All', 'Migrations are idempotent'):
    require(marker in logs['on-runtime-upgrade'], f'Missing upgrade marker: {marker}')
require(raw['runs'][1]['requested_blocks'] == 10, 'Unexpected requested count')
require(int(checker.one_match(r'Fast forwarding (\d+) blocks', logs['fast-forward'])) == 10, 'CLI block count differs')
blocks = checker.verify_blocks(logs['fast-forward'], source['finalized_block_number'] + 1, 10)
snapshot_log = (base / 'snapshot.log').read_text()
require(not re.findall(checker.ROOT_PATTERN, snapshot_log), 'Unexpected acquisition root; reassess evidence scope')
require(not re.findall(r'Completed scraping (\d+) child tries', snapshot_log), 'Unexpected child-trie count; reassess scope')
keys = int(checker.one_match(r'Found (\d+) keys in', snapshot_log))
require(keys == 3304807, 'Unexpected reported acquisition key count')
try:
    checker.one_match(checker.ROOT_PATTERN, snapshot_log)
except ValueError as error:
    full_checker_limitation = str(error)
else:
    raise AssertionError('Original checker acquisition-root requirement unexpectedly satisfied')

output = {
    'kind': 'supplementary-independent-local-evidence-verification',
    'verified_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'raw_runner_result_preserved': True,
    'raw_runner_result_sha256': sha256(result_path),
    'existing_checker_preserved': True,
    'existing_checker_sha256': sha256(checker_path),
    'supplementary_script_sha256': sha256(Path(__file__)),
    'network_calls': False,
    'runtime_or_cargo_execution': False,
    'source': source,
    'artifacts': artifacts,
    'runs': runs,
    'upgrade_checks_all': True,
    'migration_idempotency_verified': True,
    'exact_contiguous_block_count': len(blocks),
    'blocks': blocks,
    'snapshot_acquisition_evidence': {
        'log_sha256': sha256(base / 'snapshot.log'),
        'reported_keys': keys,
        'key_count_source': 'Acquisition progress: Found 3304807 keys; not an independent unique-key enumeration',
        'configured_child_trie_inclusion': True,
        'configuration_basis': 'Coordinator-recorded acquisition used no pallet/prefix filters; independently inspected pinned CLI help defaults whole-chain snapshots to child-tree inclusion',
        'cli_help_path': str(base / 'create-snapshot-help.txt'),
        'cli_help_sha256': sha256(base / 'create-snapshot-help.txt'),
        'child_tries': None,
        'child_trie_count_status': 'unknown: acquisition log has no child-trie count',
        'acquisition_logged_storage_root': None,
        'acquisition_root_status': 'not logged; RUST_LOG was not enabled during acquisition',
        'snapshot_file_digest_and_size_match_record': True,
        'subsequent_loaded_root_matches_rpc_pinned_header_in_both_runs': True,
    },
    'original_full_checker_status': 'not passed: required acquisition-log evidence is absent',
    'original_full_checker_limitation': full_checker_limitation,
    'supplementary_execution_and_artifact_checks_status': 'PASS',
    'release_ready': False,
    'remaining_release_blocker': 'Pre-registration hotkey association can prevent descriptor registration and trap bare-funded balances; unchanged by this upgrade rehearsal.',
    'weight_measurements': 'excluded; none performed',
}
(base / 'post-verification.json').write_text(json.dumps(output, indent=2) + '\n')
print(json.dumps({'supplementary_checks': 'PASS', 'blocks': [b['number'] for b in blocks], 'all_artifact_hashes_match': True, 'both_loaded_roots_match': True, 'reported_keys': keys, 'child_tries': None, 'original_full_checker_passed': False}, indent=2))
