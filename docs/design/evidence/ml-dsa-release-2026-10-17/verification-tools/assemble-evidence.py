"""Fail-closed evidence assembly. Run only after all tests, proofs and cleanup finish.

This script runs final read-only source checks, then copies a public whitelist of
proof transcripts. It never signals processes, starts nodes, commits or pushes.
Local verification transcripts are not cryptographic finality/storage proofs.
"""
from collections import Counter
from pathlib import Path
import datetime
import errno
import gzip
import hashlib
import json
import os
import re
import socket
import subprocess
import time
import xml.etree.ElementTree as ET

RUN = Path(__file__).resolve().parent
ROOT = Path('/home/johnreed/Desktop/work/subtensor')
OUT = ROOT / 'docs/design/evidence/ml-dsa-release-2026-10-17'
HEAD = '437053f80d5ebebce950602b168901bfe6591483'
CASES = {
    'release': ('release/results.xml', 17),
    'authority': ('authority/results.xml', 8),
    'operation_families': ('operations/results.xml', 8),
    'validator_paths': ('validators/results.xml', 2),
    'wallet_lifecycle': ('lifecycle/rerun-1/results.xml', 6),
    'cli_wallet_parity': ('parity/results.xml', 6),
    'extended_proxy_multisig': ('extended/rerun-1/results.xml', 3),
    'proxy_boundaries': ('proxy-boundaries/results.xml', 4),
    'registration_sponsorship': ('sponsorship/results.xml', 4),
    'nested_authority': ('nested/results.xml', 1),
    'validator_proxy': ('validator-proxy/rerun-1/results.xml', 2),
    'mixed_multisig_vault': ('mixed/metadata-rerun.xml', 24),
    'cross_validator_shield': ('shield/results.xml', 6),
}
# Receipt directory -> (accepted JUnit suite, local chain). Mixed and Shield
# have separate evidence and may finish after this bounded receipt audit.
RECEIPT_SUITES = {
    'release': ('release', 'release'),
    'authority': ('authority', 'authority'),
    'operations': ('operation_families', 'operations'),
    'validators': ('validator_paths', 'validators'),
    'lifecycle': ('wallet_lifecycle', 'lifecycle'),
    'parity': ('cli_wallet_parity', 'parity'),
    'extended': ('extended_proxy_multisig', 'extended'),
    'nested': ('nested_authority', 'extended'),
    'sponsorship': ('registration_sponsorship', 'extended'),
    'proxy-boundaries': ('proxy_boundaries', 'proxy-boundaries'),
    'validator-proxy': ('validator_proxy', 'validators'),
}
ALLOWED_EDITS = {
    'docs/design/evidence/ml-dsa-localnet-blocks.py',
    'sdk/python/tests/e2e/test_mixed_multisig.py',
    'sdk/python/tests/e2e/test_mldsa_localnet.py',
    'sdk/python/tests/e2e/test_mldsa_release.py',
    'sdk/python/tests/e2e/test_wallet_parity.py',
    'sdk/python/tests/e2e/test_wallet_shield.py',
    'sdk/python/tests/e2e/test_wallet_types.py',
    'sdk/python/tests/e2e/test_mldsa_extended.py',
    'sdk/python/tests/e2e/test_mldsa_nested_authority.py',
    'sdk/python/tests/e2e/test_mldsa_proxy_boundaries.py',
    'sdk/python/tests/e2e/test_mldsa_sponsorship_live.py',
    'sdk/python/tests/e2e/test_mldsa_validator_proxy.py',
    'docs/design/ml-dsa-release-validation-2026-10-17.md',
}
SOURCE_RESULTS = {
    'preflight': 'preflight/validation-summary.json',
    'dispatch_matrix': 'preflight/executive-matrix-scope.json',
    'ignored_rust_tests': 'preflight/ignored-rust-classification.json',
    'unselected_live_tests': 'preflight/unexecuted-live-test-classification.json',
    'mainnet_rehearsal': 'preflight/mainnet/verified-results.json',
    'mainnet_head': 'preflight/mainnet/current-mainnet-head.json',
    'mainnet_release_features': 'preflight/mainnet-metadata-hash/verified-results.json',
    'mainnet_release_artifact': 'preflight/mainnet-metadata-hash/artifact.json',
    'validator_proxy_state': 'validator-proxy/state-verification.json',
    'mixed_cli_state': 'mixed/cli-state-verification.json',
    'artifacts': 'artifacts.json',
    'chains': 'chains-verified.json',
    'test_weights': 'test-weight-overlay.json',
    'production_activation': 'production/activation.json',
    'cryptography': 'crypto/crosscheck-results.json',
    'independent_reference': 'crypto/reference.json',
    'crypto_bridge': 'crypto/bridge-artifacts.json',
    'codec_finality': 'codecs/finality-manifest.json',
    'javascript_codecs': 'codecs/javascript-decoders.json',
    'native_current': 'codecs/native-current.json',
    'native_legacy': 'codecs/native-legacy.json',
    'native_legacy_provenance': 'codecs/native-legacy-provenance.json',
    'rpc_availability': 'rpc/fixed-live-results.json',
    'ethereum_pending': 'rpc/ethereum-pending-results.json',
    'pending_contracts': 'rpc/pending-contract-results.json',
    'metadata_runtime_fixture': 'mixed/metadata-runtime-install.json',
    'canonical_receipts': 'receipt-verification/final-summary.json',
    'shutdown_primary': 'shutdown.json',
    'shutdown_extra': 'shutdown-extra.json',
    'shutdown_verified': 'shutdown-verified.json',
}
SEEN = {}
PORTABLE = {}


def require(condition, reason):
    if not condition:
        raise RuntimeError(f'Acceptance gate failed: {reason}')


def utcnow():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def remember(path):
    path = Path(path)
    require(path.is_file() and not path.is_symlink(), f'missing/non-regular evidence: {path}')
    digest = sha(path)
    require(path not in SEEN or SEEN[path] == digest, f'evidence changed during assembly: {path}')
    SEEN[path] = digest
    return digest


def load(relative):
    path = RUN / relative
    remember(path)
    return json.loads(path.read_text())


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def workspace_snapshot():
    require(git('rev-parse', 'HEAD').decode().strip() == HEAD, 'candidate HEAD changed')
    require(not git('ls-files', '-u'), 'unmerged files remain')
    require(not git('diff', '--name-only', '--diff-filter=DRT', 'HEAD'), 'deleted/renamed/type-changed files')
    changed = set(filter(None, git('diff', '--name-only', '-z', 'HEAD').decode().split('\0')))
    changed.update(filter(None, git('ls-files', '--others', '--exclude-standard', '-z').decode().split('\0')))
    out_prefix = str(OUT.relative_to(ROOT)) + '/'
    disallowed = sorted(p for p in changed if p not in ALLOWED_EDITS and not p.startswith(out_prefix))
    require(not disallowed, f'changes outside reviewed tests/report/evidence: {disallowed}')
    snapshot = {}
    for relative in sorted(changed):
        path = ROOT / relative
        require(path.is_file() and not path.is_symlink(), f'non-regular changed path: {relative}')
        if not relative.startswith(out_prefix):
            snapshot[relative] = sha(path)
    # Pin unchanged e2e helpers too; an edited helper can invalidate accepted cases.
    for path in sorted((ROOT / 'sdk/python/tests/e2e').glob('*.py')):
        snapshot[str(path.relative_to(ROOT))] = sha(path)
    return snapshot


def verify_junit():
    accepted = {}
    identities = set()
    for name, (relative, expected) in CASES.items():
        path = RUN / relative
        digest = remember(path)
        root = ET.parse(path).getroot()
        cases = list(root.iter('testcase'))
        require(len(cases) == expected, f'{name}: expected {expected} cases, found {len(cases)}')
        require(not any(list(root.iter(kind)) for kind in ('failure', 'error', 'skipped')), f'{name}: nonpassing XML')
        suites = list(root.iter('testsuite'))
        require(suites, f'{name}: no testsuite counters')
        for suite in suites:
            require(int(suite.attrib.get('tests', -1)) == len(list(suite.iter('testcase'))), f'{name}: test count mismatch')
            for key in ('failures', 'errors', 'skipped'):
                require(int(suite.attrib.get(key, -1)) == 0, f'{name}: {key} counter')
        for case in cases:
            key = (case.attrib.get('classname'), case.attrib.get('name'))
            require(all(key) and key not in identities, f'duplicate/unnamed accepted case: {key}')
            identities.add(key)
        accepted[name] = {
            'passed': expected, 'failed': 0, 'skipped': 0, 'source': relative, 'sha256': digest,
            'cases': [{'name': c.attrib['name'], 'class': c.attrib['classname'],
                       'seconds': float(c.attrib.get('time', 0))} for c in cases],
        }
    require(sum(row['passed'] for row in accepted.values()) == 91, 'live total is not 91')
    return accepted


def verify_lockfiles():
    path = RUN / 'preflight/lockfiles-before.sha256'
    remember(path)
    records = {}
    for line in path.read_text().splitlines():
        digest, name = line.split(None, 1)
        candidate = Path(name.strip().lstrip('*'))
        if not candidate.is_absolute():
            candidate = ROOT / candidate
        require(candidate.is_relative_to(ROOT), f'lockfile outside repository: {candidate}')
        require(remember(candidate) == digest, f'lockfile changed: {candidate}')
        records[str(candidate.relative_to(ROOT))] = digest
    require(set(records) == {'Cargo.lock', 'sdk/python/uv.lock', 'ts-tests/pnpm-lock.yaml',
                            'website/yarn.lock', '.github/docs-preview-vercel/package-lock.json'},
            'lockfile baseline does not cover all five governed files')
    return records


def verify_rehearsal(result, directory):
    require(result['successful'] is True, f'{directory}: unsuccessful rehearsal')
    for flag in ('spec_name_check_enabled', 'spec_version_check_enabled', 'idempotency_check_enabled'):
        require(result[flag] is True, f'{directory}: {flag} disabled')
    require(result['chain_writes'] is False, f'{directory}: mainnet write')
    require(result['snapshot_acquisition_proof']['state_root_matches_finalized_header'] is True,
            f'{directory}: snapshot root not authenticated')
    require(result['snapshot_acquisition_proof']['storage_root'] == result['source']['state_root'],
            f'{directory}: snapshot root mismatch')
    runs = {row['name']: row for row in result['runs']}
    require(len(result['runs']) == 2 and set(runs) == {'on-runtime-upgrade', 'fast-forward'},
            f'{directory}: incomplete/duplicate rehearsal stages')
    for row in runs.values():
        require(row['exit_code'] == 0 and row['snapshot_state_root_matches_finalized_header'] is True,
                f'{directory}: failed stage {row["name"]}')
        require(Path(row['log_file']).name == row['log_file'], 'unexpected rehearsal log path')
        require(remember(RUN / directory / row['log_file']) == row['log_sha256'], 'rehearsal log digest mismatch')
    upgrade, replay = runs['on-runtime-upgrade'], runs['fast-forward']
    require(upgrade['migrations_idempotent'] is True and upgrade['upgrade_checks_all'] is True,
            f'{directory}: upgrade/idempotency checks incomplete')
    require(upgrade['reported_runtime_versions'] == [479, 480], f'{directory}: unexpected runtime versions')
    for field in ('requested_blocks', 'observed_executed_blocks', 'observed_successful_blocks'):
        require(replay[field] == 10, f'{directory}: incomplete replay {field}')
    first = result['source']['finalized_block_number'] + 1
    require([b['number'] for b in replay['blocks']] == list(range(first, first + 10)), 'replay sequence mismatch')
    for block in replay['blocks']:
        require(block['try_state'] == 'All' and all(block[k] is True for k in
                ('state_root_check', 'signature_check', 'entire_runtime_state_decodes', 'successfully_executed')),
                f'{directory}: incomplete checks for block {block["number"]}')
    proof = result['post_run_verification']
    require(proof['all_log_digests_match'] is True and proof['exact_block_sequence_and_per_block_checks_passed'] is True,
            f'{directory}: post-run verification failed')
    artifact = load(f'{directory}/artifact.json')
    require(artifact['runtime_wasm_sha256'] == result['runtime_wasm_sha256'], 'rehearsal artifact mismatch')
    require(remember(RUN / directory / 'production-try-runtime.wasm') == result['runtime_wasm_sha256'],
            'actual rehearsal wasm mismatch')
    require((RUN / directory / 'runner.exit').read_text().strip() == '0', 'rehearsal runner exit not zero')


def verify_receipts(results, accepted):
    full = load('receipt-verification/final.json')
    brief = results['canonical_receipts']
    require(full['candidate'] == HEAD and full['all_recorded_receipts_verified'] is True and not full['errors'],
            'canonical receipt verifier failed')
    require(set(full['suites']) == set(RECEIPT_SUITES), 'canonical receipt suite scope changed')
    require({key for key, _ in RECEIPT_SUITES.values()} ==
            set(CASES) - {'mixed_multisig_vault', 'cross_validator_shield'},
            'receipt-to-JUnit mapping does not cover exactly the eleven receipt-bearing suites')
    require(set(full['chains']) == {chain for _, chain in RECEIPT_SUITES.values()},
            'canonical receipt chain scope changed')
    for suite, (accepted_key, chain) in RECEIPT_SUITES.items():
        require(accepted_key in accepted and full['suites'][suite]['chain'] == chain,
                f'{suite}: incorrect receipt/JUnit/chain mapping')
    require(not full['suites_without_receipt_evidence'], 'receipt suite has no evidence')
    for field, value in brief.items():
        require(full.get(field) == value, f'canonical summary disagrees with full report: {field}')
    require(full['unique_receipts'] == full['verified_receipts'] == len(full['receipts']) > 0, 'receipt totals disagree')
    require(full['raw_receipt_references'] - full['deduplicated_references'] == full['unique_receipts'], 'receipt deduplication mismatch')
    identities = {(r['chain'], r['block_hash'], r['extrinsic_index']) for r in full['receipts']}
    require(len(identities) == len(full['receipts']), 'duplicate canonical receipt')
    require(dict(Counter(r['status'] for r in full['receipts'])) == full['status_counts'], 'receipt status counts mismatch')
    chain_ids = {r['name']: r['genesis'] for r in results['chains']['chains']}
    for name, chain in full['chains'].items():
        rows = [row for row in full['receipts'] if row['chain'] == name]
        require(chain['genesis'] == chain_ids[name] and not chain['errors'], f'{name}: wrong genesis/receipt error')
        require(chain['verified_receipts'] == chain['unique_receipts'] == len(rows), f'{name}: receipt totals mismatch')
        for row in rows:
            require(row['canonical_finalized'] is True and row['height'] <= chain['finalized_snapshot']['height'],
                    f'{name}: receipt is not canonical finalized')
            require(row['extrinsic_index'] >= 0 and row['references'], 'invalid receipt locator')
            if row['status'] == 'system_failure':
                require(row['system_event'] == 'ExtrinsicFailed' and row['recorded_success'] is False, 'incorrect outer failure')
            else:
                require(row['status'] in ('success', 'outer_success_inner_failure') and
                        row['system_event'] == 'ExtrinsicSuccess' and
                        row['recorded_success'] is (row['status'] == 'success'), 'incorrect normalized inner/outer success')
                require(bool(row['inner_failures']) == (row['status'] == 'outer_success_inner_failure'), 'lost inner error')
    recorded = {row['path'] for row in full['files']}
    require(len(recorded) == len(full['files']), 'duplicate receipt source file')
    discovered = {str(path.relative_to(RUN)) for suite in full['suites']
                  for path in (RUN / suite).glob('**/receipts/*.json')}
    require(discovered == recorded, 'receipt verifier missed new files or references missing files')
    for row in full['files']:
        require(remember(RUN / row['path']) == row['sha256'], f'receipts changed after verification: {row["path"]}')
    started = datetime.datetime.fromisoformat(full['started_utc']).timestamp()
    receipt_junits = [accepted[key]['source'] for key, _ in RECEIPT_SUITES.values()]
    require(started >= max((RUN / relative).stat().st_mtime for relative in receipt_junits),
            'canonical final verifier started before all eleven receipt-bearing JUnit runs completed')
    return full


def verify_shutdown(results):
    chains = load('chains.json') + load('chains-extra.json')
    require(len(chains) == 11, 'expected 11 owned chains')
    expected_ports = {row[k] + offset for row in chains for k in ('rpc_port', 'p2p_port') for offset in range(3)}
    require(len(expected_ports) == 66, 'owned port configuration overlaps')
    recorded_ports = set()
    for name in ('shutdown_primary', 'shutdown_extra', 'shutdown_verified'):
        require(results[name]['all_closed'] is True, f'{name}: cleanup incomplete')
        if name != 'shutdown_verified':
            for port in results[name]['ports']:
                require(port['closed'] is True, f'{name}: open port')
                recorded_ports.add(port['port'])
    require(recorded_ports == expected_ports, 'shutdown evidence does not cover all 66 ports')
    host_check = results['shutdown_verified']
    require(host_check['remaining_owned_processes'] == [] and host_check['any_node_running'] is False,
            'independent shutdown check found remaining processes')
    require({row['port'] for row in host_check['ports']} == expected_ports | {9977} and
            all(row['closed'] is True for row in host_check['ports']),
            'independent shutdown evidence must include all validation ports and preflight 9977')
    for port in sorted(expected_ports | {9977}):
        with socket.socket() as sock:
            sock.settimeout(0.15)
            result = sock.connect_ex(('127.0.0.1', port))
            require(result == errno.ECONNREFUSED, f'port {port}: expected connection refused, got {result}; port open or check unavailable')
    live_owned_pids = []
    directories = [row['directory'] for row in chains]
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if proc.stat().st_uid != os.getuid():
                continue
            argv = (proc / 'cmdline').read_bytes().split(b'\0')
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
        args = [arg.decode(errors='replace') for arg in argv if arg]
        if args and Path(args[0]).name == 'node-subtensor' and (
            args[0] == str(RUN / 'node-subtensor') or any(path in arg for path in directories for arg in args[1:])
        ):
            live_owned_pids.append(int(proc.name))
    require(not live_owned_pids, f'owned node processes remain: {live_owned_pids}')
    return {'checked_at_utc': utcnow(), 'all_closed': True, 'ports': sorted(expected_ports | {9977}), 'owned_node_pids': []}


def final_checks():
    sdk = ROOT / 'sdk/python'
    checks = [
        ('ruff', sdk, [str(sdk / '.venv/bin/ruff'), 'check', '.']),
        ('ruff-format', sdk, [str(sdk / '.venv/bin/ruff'), 'format', '--check', '.']),
        ('ty', sdk, [str(sdk / '.venv/bin/ty'), 'check', '--exit-zero-on-warning', 'bittensor']),
        ('collection', sdk, [str(sdk / '.venv/bin/python'), '-m', 'pytest', '--collect-only', '-q']),
        ('diff-check', ROOT, ['git', 'diff', '--check', 'HEAD']),
    ]
    outcomes = []
    for name, cwd, command in checks:
        path = RUN / f'preflight/assembly-final-{name}.log'
        start, started = time.monotonic(), utcnow()
        with path.open('w') as log:
            process = subprocess.run(command, cwd=cwd, stdout=log, stderr=subprocess.STDOUT,
                                     timeout=300, check=False)
        outcomes.append({'name': name, 'command': command, 'cwd': str(cwd), 'started_at_utc': started,
                         'exit_code': process.returncode, 'elapsed_seconds': round(time.monotonic() - start, 3),
                         'log': str(path.relative_to(RUN)), 'log_sha256': remember(path)})
        require(process.returncode == 0, f'final {name} failed; inspect {path}')
        if name == 'collection':
            matches = re.findall(r'(\d+) tests? collected', path.read_text())
            require(matches and int(matches[-1]) == 3658, 'final collection must contain exactly 3658 tests')
            outcomes[-1]['collected'] = int(matches[-1])
    target = RUN / 'preflight/assembly-final-checks.json'
    target.write_text(json.dumps({'head': HEAD, 'checks': outcomes, 'all_passed': True,
                                'type_check_policy': 'CI exits zero on warnings; diagnostics are retained in the log.'}, indent=2) + '\n')
    remember(target)
    return outcomes


def portable(path, relative, compress=False, strip_junit_output=False):
    """Copy only caller-selected public files; never recurse through evidence dirs."""
    original_hash = remember(path)
    data = path.read_bytes()
    transformation = None
    if strip_junit_output:
        tree = ET.fromstring(data)
        removed = False
        for parent in tree.iter():
            for child in list(parent):
                if child.tag in ('system-out', 'system-err', 'properties'):
                    parent.remove(child)
                    removed = True
        if removed:
            data = ET.tostring(tree, encoding='utf-8', xml_declaration=True)
            transformation = 'Removed captured output/properties; test identities, counters and outcomes preserved.'
    if compress:
        data = gzip.compress(data, mtime=0)
        relative += '.gz'
    target = OUT / relative
    require(target.is_relative_to(OUT) and '..' not in Path(relative).parts, 'unsafe portable destination')
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    PORTABLE[relative] = {'source': str(path), 'source_sha256': original_hash, 'stored_sha256': sha(target),
                          'stored_bytes': len(data), 'compression': 'gzip' if compress else None,
                          'transformation': transformation}
    return relative


def main():
    initial = load('initial-manifest.json')
    require(initial['head'] == HEAD and initial['status'] == '', 'run did not start from clean pinned HEAD')
    sources_before = workspace_snapshot()
    accepted = verify_junit()
    lockfiles = verify_lockfiles()
    results = {key: load(relative) for key, relative in SOURCE_RESULTS.items()}
    preflight = results['preflight']
    require(preflight['head'] == HEAD and preflight['preflight']['exit_code'] == 0, 'full preflight failed/wrong HEAD')
    require((RUN / 'preflight/full-preflight.exit').read_text().strip() == '0', 'full preflight process failed')
    require(remember(RUN / 'preflight/full-preflight.log') == preflight['preflight']['log_sha256'], 'full preflight log changed')
    require((preflight['preflight']['rust_tests_passed'], preflight['preflight']['rust_tests_failed'],
             preflight['preflight']['rust_tests_ignored']) == (3582, 0, 47), 'unexpected Rust test totals')
    require(preflight['sdk']['distinct_offline_cases_passed'] == 3561, 'offline SDK population incomplete')
    cooldown = preflight['default_feature_cooldown_test']
    require(cooldown['exit_code'] == 0 and cooldown['passed'] == 1 and
            cooldown['covers_every_wallet_mode'] is True, 'default-feature cooldown boundary test failed')
    unselected = results['unselected_live_tests']
    require(unselected['head'] == HEAD and len(unselected['cases']) == 6 and
            all(case['executed'] is False and case['counted_as_passed'] is False for case in unselected['cases']),
            'unselected live tests must remain explicitly unexecuted')
    matrix = results['dispatch_matrix']
    require(matrix['head'] == HEAD and matrix['executive_pool_and_dispatch_cases'] == 1292 and
            matrix['executive_dispatch_variants'] == 323 and matrix['unresolved_fixtures'] == 0,
            'dispatch matrix incomplete')
    require(matrix['modes'] == ['plain_sr25519', 'plain_ed25519', 'rotating_mldsa65', 'fixed_mldsa65'],
            'dispatch matrix modes changed; revise scope')
    ignored = results['ignored_rust_tests']
    require(ignored['head'] == HEAD and ignored['ignored_count'] == len(ignored['entries']) == 47 and
            ignored['ignored_mldsa_tests'] == 0, 'ignored Rust tests not fully classified')
    for name, key in (('dispatch_matrix', 'executive_matrix_scope'), ('ignored_rust_tests', 'ignored_rust_classification')):
        require(preflight[key]['sha256'] == SEEN[RUN / SOURCE_RESULTS[name]], f'{name}: stale preflight digest')
    require(all(row['exit_code'] == 0 for row in preflight['wasm_checks']) and len(preflight['wasm_checks']) == 4,
            'wasm checks incomplete')
    for name, directory, reference in (
        ('mainnet_rehearsal', 'preflight/mainnet', 'mainnet_rehearsal'),
        ('mainnet_release_features', 'preflight/mainnet-metadata-hash', 'mainnet_metadata_hash_rehearsal'),
    ):
        verify_rehearsal(results[name], directory)
        require(preflight[reference]['successful'] is True and preflight[reference]['verified_result_sha256'] ==
                SEEN[RUN / SOURCE_RESULTS[name]], f'{name}: stale preflight reference')
    feature_artifact = results['mainnet_release_artifact']
    require(feature_artifact['build_features'] == ['try-runtime', 'metadata-hash'] and
            feature_artifact['build_profile'] == 'production' and feature_artifact['build_exit_code'] == 0,
            'production metadata-hash rehearsal build incorrect')
    require(results['mainnet_head']['candidate_spec_version_exceeds_current_mainnet'] is True and
            results['mainnet_head']['chain_writes'] is False, 'mainnet compatibility check failed')
    require(results['cryptography']['status'] == 'PASS' and sum(results['cryptography']['case_counts'].values()) == 8003,
            'crypto cross-check incomplete')
    require(results['independent_reference']['source_clean'] is True and
            results['independent_reference']['source_commit'] == results['cryptography']['liboqs_commit'],
            'independent crypto reference provenance mismatch')
    for name in ('rpc_availability', 'ethereum_pending', 'pending_contracts'):
        require(results[name]['status'] == 'passed', f'{name} failed')
    state = results['validator_proxy_state']
    require(state['candidate'] == HEAD and state['all_verified'] is True and len(state['cases']) == 2 and
            all(case['verified'] is True for case in state['cases']), 'validator proxy state verification failed')
    cli_state = results['mixed_cli_state']
    require(cli_state['candidate'] == HEAD and cli_state['all_verified'] is True and len(cli_state['cases']) == 2 and
            all(case['verified'] is True for case in cli_state['cases']), 'mixed CLI final-state verification failed')
    chains = results['chains']
    require(chains['distinct_chains'] == len(chains['chains']) == 11 and chains['normal_slot_seconds'] == 12 and
            chains['all_runtime_code_hashes_match_expected_artifacts'] is True, 'chain evidence incomplete')
    require(len({row['genesis'] for row in chains['chains']}) == 11 and
            all(row['minimum_period_ms'] == 6000 and row['archive'] is True for row in chains['chains']),
            'chains not distinct normal-slot archive chains')
    overlay = results['test_weights']
    require(overlay['source_commit'] == HEAD and len(overlay['patched_files']) == 5, 'incorrect test overlay provenance')
    for row in overlay['patched_files']:
        require(remember(ROOT / row['path']) == row['source_sha256'], f'production weight source changed: {row["path"]}')
    expected_codes = {row['name']: results['artifacts'][
        'production-runtime.wasm' if row['name'] == 'production' else 'test-runtime.wasm']['sha256']
        for row in chains['chains']}
    expected_codes['mixed'] = results['metadata_runtime_fixture']['wasm_sha256']
    require(results['metadata_runtime_fixture']['receipt']['success'] is True, 'mixed runtime fixture failed')
    require(all(row['runtime_code_sha256'] == expected_codes[row['name']] for row in chains['chains']),
            'chain code digest disagrees with tested artifacts')
    activation = results['production_activation']
    production_chain = next(row for row in chains['chains'] if row['name'] == 'production')
    # Preserve the original recorder's nested JSON manifest instead of rewriting it.
    require(json.loads(activation['head']) == initial and activation['genesis'] == production_chain['genesis'] and
            activation['wasm_sha256'] == expected_codes['production'] and activation['minimum_period'] == 6000,
            'production activation evidence provenance mismatch')
    require(activation['activation']['success'] is True and
            {case['crypto_type'] for case in activation['production_gate_cases']} == {7, 8} and
            len(activation['production_gate_cases']) == 2 and
            all(case['rejection']['success'] is False and
                case['rejection']['error']['name'] == 'ExhaustsResources'
                for case in activation['production_gate_cases']),
            'production weight activation gate not verified for both MS profiles')
    for path, record in results['artifacts'].items():
        require(Path(path).name == path and remember(RUN / path) == record['sha256'], f'artifact hash mismatch: {path}')
    codec = results['codec_finality']
    require(codec['head'] == HEAD and codec['all_six_finalized_same_block_successfully'] is True and
            codec['confirmed_finalized_number'] >= codec['block_number'], 'six-wallet codec block not finalized')
    for relative, digest in codec['evidence_files_sha256'].items():
        require(remember(RUN / 'codecs' / relative) == digest, f'codec proof changed: {relative}')
    for relative, digest in codec['runner_sha256'].items():
        require(remember(ROOT / relative) == digest, f'codec runner changed: {relative}')
    for name in ('native_current', 'native_legacy', 'javascript_codecs'):
        record = results[name]
        require(record['capture_sha256'] == codec['capture_sha256'] and record['six_wallet_block'] == codec['block_hash'],
                f'{name}: wrong block/capture')
        versions = ('14', '15', '16') if name == 'javascript_codecs' else ('14', '15')
        for version in versions:
            row = record['metadata'][version]
            field = 'exact_extrinsic_roundtrips' if name == 'javascript_codecs' else 'extrinsics_decoded'
            require(row[field] == 12 and set(row['wallet_types']) == {0, 1, 4, 6, 7, 8}, f'{name}: missing codec modes')
        if name != 'javascript_codecs':
            require(record['metadata']['16']['supported'] is False, 'native V16 limitation changed; revise report')
    receipts = verify_receipts(results, accepted)
    shutdown = verify_shutdown(results)
    checks = final_checks()
    require(workspace_snapshot() == sources_before, 'source changed during final checks')
    verify_lockfiles()
    for path, digest in SEEN.items():
        require(sha(path) == digest, f'evidence changed before export: {path}')

    # Every acceptance gate above must pass before any repository output is written.
    for relative in SOURCE_RESULTS.values():
        portable(RUN / relative, 'proofs/' + relative)
    for relative in ('initial-manifest.json', 'chains.json', 'chains-extra.json',
                     'preflight/lockfiles-before.sha256', 'preflight/assembly-final-checks.json'):
        portable(RUN / relative, 'proofs/' + relative)
    for relative in ('static-review.txt', 'test-assertion-review.txt'):
        portable(RUN / relative, 'proofs/' + relative)
    portable(RUN / 'receipt-verification/final.json', 'proofs/receipt-verification/final.json', compress=True)
    portable(RUN / 'codecs/finalized-six-wallet-blocks.json', 'proofs/codecs/finalized-six-wallet-blocks.json', compress=True)
    for relative, _ in CASES.values():
        portable(RUN / relative, 'proofs/' + relative, strip_junit_output=True)
    for relative in ('preflight/sdk-pytest.xml', 'preflight/sdk-legacy-fernet.xml'):
        portable(RUN / relative, 'proofs/' + relative, compress=True, strip_junit_output=True)
    portable(RUN / 'preflight/sdk-collect-latest.log', 'proofs/preflight/sdk-collect-latest.log', compress=True)
    logs = ['preflight/full-preflight.log'] + [row['log'] for row in checks]
    for name, directory in (('mainnet_rehearsal', 'preflight/mainnet'),
                            ('mainnet_release_features', 'preflight/mainnet-metadata-hash')):
        logs.extend(f'{directory}/{row["log_file"]}' for row in results[name]['runs'])
    for relative in logs:
        portable(RUN / relative, 'proofs/' + relative, compress=True)
    for relative in ('assemble-evidence.py', 'receipt-verification/verify.py', 'validator-proxy/verify.py',
                     'crypto/crosscheck.py', 'crypto/build_bridge.py', 'crypto/primitive_bridge.rs',
                     'mixed/verify-cli-state.py'):
        portable(RUN / relative, 'verification-tools/' + relative)
    for relative in sorted(sources_before):
        if relative.endswith(('.py', '.cjs')):
            portable(ROOT / relative, 'tested-sources/' + relative, compress=True)
    for relative in codec['runner_sha256']:
        target = 'tested-sources/' + relative + '.gz'
        if target not in PORTABLE:
            portable(ROOT / relative, 'tested-sources/' + relative, compress=True)

    summary = {
        'head': HEAD, 'completed_at_utc': utcnow(),
        'status': 'All 91 accepted local live cases passed; scope and exclusions are recorded below.',
        'local_live_cases_passed': 91, 'new_live_regression_cases': 14,
        'additional_existing_mixed_cases_beyond_previous_53_case_run': 24,
        'counting': '91 = 53 existing baseline + 14 new regressions + 24 existing mixed cases. Rust matrix cases are included in 3582, not added again. Receipt counts are deduplicated observations, not additional test cases.',
        'normal_slot_seconds': 12, 'fresh_isolated_chains': 11, 'validators_per_chain': 3, 'archive_state': True,
        'production_source_changes': False, 'lockfile_changes': False,
        'committed_or_pushed': False, 'accepted_live_runs': accepted, 'results': results,
        'assembly_final_checks': checks, 'assembly_shutdown_recheck': shutdown,
        'reviewed_source_sha256': sources_before, 'lockfile_sha256': lockfiles,
        'raw_evidence_directory': str(RUN),
        'evidence_sha256': {relative: SEEN[RUN / relative] for relative in SOURCE_RESULTS.values()},
        'portable_files': PORTABLE,
        'portable_receipt_scope': {
            'file': 'proofs/receipt-verification/final.json.gz', 'unique_receipts': receipts['unique_receipts'],
            'status_counts': receipts['status_counts'], 'chains': sorted(receipts['chains']),
            'suite_mapping': {suite: {'accepted_live_run': key, 'chain': chain}
                              for suite, (key, chain) in RECEIPT_SUITES.items()},
            'timing': 'Canonical audit began after all eleven receipt-bearing suites completed. Mixed and Shield may complete later; their passing JUnit results remain mandatory for the overall 91-case gate.',
            'scope': 'Canonical finalized RPC verification transcript with unique chain/block/index references, extrinsic hashes, event hashes and expected inner/outer failures. Includes recorded setup/superseded-attempt receipts; not a claim that every transaction in every suite is inventoried. Mixed, Shield and RPC suites have separate result evidence.',
            'proof_limit': 'No GRANDPA justification/storage trie proof is embedded. Without retained local chain state, this transcript and its hashes are not independently replayable consensus proofs.',
        },
        'superseded_fixture_attempts': [
            {'path': 'extended/pytest.log', 'reason': 'Corrected canonical event loading for normalized failed receipts; all three cases rerun.'},
            {'path': 'mixed/pytest.log', 'reason': 'Added metadata-hash to the same-head functional fixture required by Vault; all 24 cases rerun.'},
            {'path': 'lifecycle/pytest.log', 'reason': 'Stopped before late uncapped registrations; added explicit consent and reran all six modes.'},
            {'path': 'validator-proxy/pytest.log', 'reason': 'Corrected SCALE Ok-unit representation assertion; both modes rerun.'},
        ],
        'boundaries': [
            'Functional activation uses the documented five-file test weight overlay; production Weight::MAX gates remain. No reference weights or production capacity result is claimed.',
            'Both mainnet build-feature rehearsals use the authenticated snapshot at block 9290353, approximately 49 hours old at execution, and ten synthetic blocks each. They are not a fresh snapshot or a complete historical/future transaction replay; no Srtool bit-identical release artifact is claimed.',
            'Finalize descriptor-account registration before public disclosure or bare funding, under the accepted operational rule.',
            'The 1292 Executive cases test authentication and dispatch parity for 323 native call variants using plain sr25519, plain ed25519, fixed MS and rotating MS. Hashed sr/ed are not matrix modes. Minimal fixtures do not establish successful business semantics for every operation; two uninhabited calls and the self-contained Ethereum call have separate boundaries.',
            'Forty-seven Rust tests remain ignored and are classified separately; none is claimed passed. The CI type-check policy permits warnings, retained in its fresh log.',
            'Real signing, SCALE decoding and finalized dispatch tests are not external certification, side-channel certification, formal proof, or an absolute no-loss guarantee.',
            'The 7200-block EVM association cooldown was preserved. Live normal-slot lifecycle cases prove early rejection. A separate passing default-feature Executive test covers rejection at 7199 and success at 7200 across all six wallet modes; it is not a live 7200-block wait.',
            'Current and legacy Python native libraries support metadata V14/V15 but not V16; tested JavaScript decoders pass V14/V15/V16. No graphical Polkadot.js application or every historical client was tested.',
            'Raw Ethereum RLP uses secp256k1; MS EVM operations use native EVM call/create routes.',
            'The pre-existing Ethereum receipt effectiveGasPrice discrepancy is separate from verified actual debit and this PR regression assessment.',
            'Vault tests simulate the external phone response. No physical hardware wallet, sustained adversarial load/capacity test, or remote GitHub CI run is established by this local evidence.',
            'Portable files exclude wallet keyfiles, node databases, binaries and the large mainnet snapshot. Exact hashes identify omitted artifacts but cannot substitute for their contents when reproducing the run.',
        ],
    }
    OUT.mkdir(parents=True, exist_ok=True)
    target = OUT / 'validation-summary.json'
    target.write_text(json.dumps(summary, indent=2) + '\n')
    print(target)


if __name__ == '__main__':
    main()
