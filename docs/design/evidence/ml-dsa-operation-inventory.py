"""Inventory release calls and distinguish regression coverage from origin inference.

Run with the candidate SDK/native extension on PYTHONPATH:
  python ml-dsa-operation-inventory.py RELEASE_FIXTURE [OUTPUT] [--polkadot-sdk PATH]

This is a source/metadata audit, not a transaction execution test. It reuses the
SDK's deliberate wrap/raw-only inventory. Origin classification follows explicit
guards and origin-forwarding helpers; unresolved paths remain unresolved. The
curated regression map describes what each test actually exercises and does not
turn a referenced call into complete business-operation or finalized-chain proof.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

from bittensor._transport.codec import RuntimeCodec
from bittensor.intents import REGISTRY
from codegen.check import COVERED_PALLETS, RAW_ONLY


ROOT = Path(__file__).resolve().parents[3]
LOCAL = {
    "SubtensorModule": "pallets/subtensor/src",
    "Utility": "pallets/utility/src",
    "Proxy": "pallets/proxy/src",
    "Commitments": "pallets/commitments/src",
    "AdminUtils": "pallets/admin-utils/src",
    "Crowdloan": "pallets/crowdloan/src",
    "Swap": "pallets/swap/src",
    "MevShield": "pallets/shield/src",
    "LimitOrders": "pallets/limit-orders/src",
    "HashedAccounts": "pallets/hashed-accounts/src",
    "EVM": "vendor/frontier/frame/evm/src",
}

# Each entry is deliberately a focused claim, not an exhaustive per-call verdict.
REGRESSIONS = [
    (
        "runtime/src/mldsa_tests.rs",
        "sponsor_registration_and_funding_are_atomic_under_normal_signed_v4",
        "Executive: classical sponsor registers/funds ML-DSA atomically; new account subsequently signs.",
        "HashedAccounts.register Balances.transfer_keep_alive Utility.batch_all",
    ),
    (
        "runtime/src/mldsa_tests.rs",
        "mixed_and_all_pq_multisig_execute_under_the_unchanged_account_and_call_hash",
        "Executive: mixed/all-PQ 2-of-2 approvals, protected registration and transfer; stable account and call hash. Not every threshold/cancel route.",
        "Multisig.approve_as_multi Multisig.as_multi HashedAccounts.register_derived HashedAccounts.register_derived_sponsored Balances.transfer_keep_alive",
    ),
    (
        "runtime/src/mldsa_tests.rs",
        "pq_delegates_preserve_identity_and_classical_delegates_are_rejected",
        "Executive: ML-DSA delegate transfer and identity/generation checks; classical delegate rejected.",
        "Proxy.add_proxy Proxy.proxy Balances.transfer_keep_alive",
    ),
    (
        "runtime/src/mldsa_tests.rs",
        "protected_derivatives_and_pure_proxies_preserve_their_floor_through_lifecycle",
        "Executive: protected derivative/pure creation, failed child rollback, pure destruction retains protection.",
        "Utility.as_protected_derivative Proxy.create_protected_pure Proxy.proxy Proxy.kill_pure Balances.transfer_keep_alive",
    ),
    (
        "runtime/src/mldsa_tests.rs",
        "restricted_proxies_cannot_reserve_permanent_protected_account_deposits",
        "Runtime dispatch: restricted direct/batch/nested proxy routes cannot reserve protected deposits; Any control succeeds.",
        "Utility.as_protected_derivative Utility.batch_all Proxy.create_protected_pure Proxy.proxy",
    ),
    (
        "runtime/src/mldsa_tests.rs",
        "shield_mldsa_outer_and_inner_consume_exactly_the_two_prepared_generations",
        "Executive: carrier/inner generation lifecycle, malformed proof and replay rejection. Ciphertext framing is synthetic; not proposer decryption or finalized blocks.",
        "MevShield.submit_encrypted",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "queried_nonce_executes_and_advances_independently_for_every_wallet_mode",
        "Executive: native EVM call/nonce for ordinary, hashed and ML-DSA wallets.",
        "EVM.call",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "create_prediction_matches_dispatch_with_and_without_explicit_nonce",
        "Executive: EVM create address prediction with/without explicit nonce across wallet modes including ML-DSA.",
        "EVM.create",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "create2_prediction_and_envelope_fee_match_dispatch",
        "Executive: EVM create2 address and envelope fee across wallet modes including ML-DSA.",
        "EVM.create2",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "authorization_fee_is_not_refunded_when_gas_pays_for_execution",
        "Executive: authentication fee survives native EVM gas/refund accounting.",
        "EVM.call",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "reverted_and_invalid_evm_calls_keep_authorization_fees_and_reject_replays",
        "Executive: EVM revert/invalid cases retain authentication fees and reject replay.",
        "EVM.call",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "every_wallet_mode_executes_mixed_multisig_and_proxy_transfers",
        "Executive: wallet-mode matrix including ML-DSA for multisig and proxy transfers.",
        "Multisig.as_multi Multisig.approve_as_multi Proxy.proxy Proxy.add_proxy Balances.transfer_keep_alive",
    ),
    (
        "runtime/src/hashed_native_evm_tests.rs",
        "staking_fee_quotes_and_charges_preserve_account_wrapper_costs",
        "Fee-extension test only: remove_stake quotation/charge for ML-DSA and forced inner failure; not successful unstaking parity.",
        "SubtensorModule.remove_stake",
    ),
    (
        "runtime/src/mldsa_order_tests.rs",
        "aggregate_capability_failure_reverts_real_assets_fees_reserves_and_order_state",
        "Runtime dispatch: capability/order batch failure rolls back real asset/fee/reserve state.",
        "HashedAccounts.install_capability LimitOrders.execute_orders",
    ),
    (
        "runtime/src/mldsa_order_tests.rs",
        "restricted_proxy_cannot_create_deferred_order_spending_authority",
        "Runtime dispatch: restricted proxy routes cannot approve orders/install spending capability; full authority control succeeds.",
        "HashedAccounts.install_capability LimitOrders.authorize_order Proxy.proxy Utility.batch_all",
    ),
    (
        "pallets/hashed-accounts/src/mldsa_tests.rs",
        "mldsa_flags_are_independent_root_only_and_never_erase_protection",
        "Pallet mock: Root-only independent switches, registration/check behavior and persistent protection.",
        "HashedAccounts.set_mldsa_enabled HashedAccounts.register HashedAccounts.check_registered",
    ),
    (
        "pallets/hashed-accounts/src/mldsa_tests.rs",
        "sponsored_derived_registration_requires_owner_and_payer_consent_and_never_double_charges",
        "Pallet mock: derived owner/payer consent and idempotent charging; not an outer ML-DSA transaction.",
        "HashedAccounts.register_derived_sponsored",
    ),
    (
        "pallets/hashed-accounts/src/mldsa_tests.rs",
        "capabilities_require_protected_owner_and_have_independent_monotonic_epochs",
        "Pallet mock: protected owner and independent install/revoke epochs.",
        "HashedAccounts.install_capability HashedAccounts.revoke_capability",
    ),
    (
        "pallets/limit-orders/src/tests/authorization.rs",
        "original_calls_execute_mixed_classical_approved_and_mldsa_orders_atomically",
        "Pallet mock: original sequential/netted calls execute mixed classical/account-approved/ML-DSA orders atomically.",
        "LimitOrders.execute_orders LimitOrders.execute_batched_orders LimitOrders.authorize_order",
    ),
    (
        "pallets/limit-orders/src/tests/authorization.rs",
        "original_cancel_clears_approved_remainder_without_changing_legacy_cancellation",
        "Pallet mock: original cancellation clears approved remainder and preserves legacy semantics.",
        "LimitOrders.cancel_order LimitOrders.authorize_order",
    ),
    (
        "pallets/limit-orders/src/tests/authorization.rs",
        "unified_calls_reject_mismatched_proof_modes_and_bound_work_before_verification",
        "Pallet mock: mismatched legacy/new proof modes and too many authorized entries fail closed.",
        "LimitOrders.execute_orders LimitOrders.execute_batched_orders",
    ),
    (
        "pallets/limit-orders/src/tests/authorization.rs",
        "mldsa_order_proof_decodes_bounded_bytes_and_requires_exact_signature_length",
        "Pallet mock/codec: bounded proof decoding and exact signature length enforcement.",
        "LimitOrders.execute_orders",
    ),
]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def masked_rust(text):
    # Rust block comments can nest. Keep offsets/newlines, including raw-string
    # bodies, so braces in documentation/examples cannot swallow real functions.
    out = list(text)
    i = 0
    while i < len(text):
        end = i
        if text.startswith("//", i):
            end = text.find("\n", i)
            if end < 0:
                end = len(text)
        elif text.startswith("/*", i):
            end, depth = i + 2, 1
            while depth and end < len(text):
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        elif (
            text[i] == "r"
            and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_"))
            and (raw := re.match(r'r(\#*)"', text[i:]))
        ):
            closing = '"' + raw[1]
            close = text.find(closing, i + len(raw[0]))
            end = len(text) if close < 0 else close + len(closing)
        elif text[i] == '"':
            end = i + 1
            while end < len(text):
                if text[end] == "\\":
                    end += 2
                elif text[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
        elif text[i] == "'" and (char := re.match(r"'(?:\\.|[^'\\\n])'", text[i:])):
            end = i + len(char[0])
        if end > i:
            out[i:end] = ["\n" if c == "\n" else " " for c in text[i:end]]
            i = end
        else:
            i += 1
    return "".join(out)


def functions(path):
    text = path.read_text()
    mask = masked_rust(text)
    result = []
    for match in re.finditer(r"\bfn\s+(\w+)\s*(?:<[^{};]*?>)?\s*\(", mask):
        parameter_end, parens = match.end(), 1
        while parens and parameter_end < len(mask):
            parens += (mask[parameter_end] == "(") - (mask[parameter_end] == ")")
            parameter_end += 1
        brace = mask.find("{", parameter_end)
        semicolon = mask.find(";", parameter_end)
        if brace < 0 or 0 <= semicolon < brace:
            continue
        depth, end = 1, brace + 1
        while depth and end < len(mask):
            depth += (mask[end] == "{") - (mask[end] == "}")
            end += 1
        result.append(
            {
                "name": match[1],
                "header": mask[match.start() : brace],
                "body": mask[brace:end],
                "path": path,
                "line": text.count("\n", 0, match.start()) + 1,
                "body_line": text.count("\n", 0, brace) + 1,
            }
        )
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("output", type=Path, nargs="?")
    parser.add_argument("--polkadot-sdk", type=Path)
    args = parser.parse_args()
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    dep = cargo["workspace"]["dependencies"]["pallet-balances"]
    sdk = args.polkadot_sdk
    if sdk is None:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        candidates = sorted(
            (cargo_home / "git/checkouts").glob(f"polkadot-sdk-*/{dep['rev'][:7]}")
        )
        if len(candidates) != 1:
            raise SystemExit("Pass --polkadot-sdk for the pinned checkout")
        sdk = candidates[0]
    actual = subprocess.check_output(
        ["git", "-C", str(sdk), "rev-parse", "HEAD"], text=True
    ).strip()
    assert actual == dep["rev"], (actual, dep["rev"])
    fixture_bytes = args.fixture.read_bytes()
    fixture = json.loads(fixture_bytes)
    codec = RuntimeCodec(
        bytes.fromhex(fixture["metadata"]["15"].removeprefix("0x")),
        spec_version=fixture["specVersion"],
        transaction_version=fixture["transactionVersion"],
    )
    pallets = codec.metadata_ir().pallets
    registry = {entry["id"]: entry["type"] for entry in codec.registry_types()}
    runtime_call = next(
        t for t in registry.values() if t.get("path", [])[-1:] == ["RuntimeCall"]
    )
    call_indices = {}
    for variant in runtime_call["def"]["variant"]["variants"]:
        inner = registry[variant["fields"][0]["type"]]
        for call in inner["def"]["variant"]["variants"]:
            call_indices[(variant["name"], call["name"])] = call["index"]

    touched = set()

    def source(path):
        touched.add(path)
        if path.is_relative_to(ROOT):
            return str(path.relative_to(ROOT))
        return f"{dep['git'].removesuffix('.git')}/blob/{dep['rev']}/{path.relative_to(sdk)}"

    banks = {}
    for pallet in COVERED_PALLETS:
        directory = (
            ROOT / LOCAL[pallet]
            if pallet in LOCAL
            else sdk / "substrate/frame" / pallet.lower() / "src"
        )
        bank = defaultdict(list)
        for path in sorted(directory.rglob("*.rs")):
            if (
                any(
                    "test" in part or "benchmark" in part or part == "mock.rs"
                    for part in path.relative_to(directory).parts
                )
                or path == directory / "weights.rs"
            ):
                continue
            for item in functions(path):
                bank[item["name"]].append(item)
        banks[pallet] = bank

    def guards(pallet, name, seen=()):
        if name in seen or len(seen) >= 8:
            return []
        result = []
        for function in banks[pallet].get(name, []):
            if not re.search(r"\b_?origin\s*:", function["header"]):
                continue
            body = function["body"]
            compact = re.sub(r"\s+", "", body)
            if compact in {"{Err(Error::<T>::Deprecated.into())}", "{Ok(())}"}:
                result.append(
                    {
                        "guard": "unconditional_deprecated_rejection"
                        if "Deprecated" in compact
                        else "origin_ignored_compatibility_no_op",
                        "source": source(function["path"]),
                        "function": name,
                        "line": function["body_line"],
                    }
                )
            # Follow only helpers explicitly passed the origin as first argument.
            for helper in re.findall(r"Self::(\w+)\s*\(\s*origin\b", body):
                result.extend(guards(pallet, helper, (*seen, name)))
            for helper in re.findall(
                r"pallet_subtensor::Pallet::<T>::(\w+)\s*\(\s*origin\b", body
            ):
                result.extend(guards("SubtensorModule", helper, (*seen, name)))
            for match in re.finditer(
                r"(?:[\w:<>]+::)?(?:ensure_[a-z_]+|try_origin|from_runtime_origin)\s*\([^;?]{0,350}",
                body,
            ):
                snippet = re.sub(r"\s+", " ", match[0]).strip()
                if re.search(r"\borigin\b", snippet):
                    result.append(
                        {
                            "guard": snippet,
                            "source": source(function["path"]),
                            "function": name,
                            "line": function["body_line"]
                            + body.count("\n", 0, match.start()),
                        }
                    )
        return list(
            {json.dumps(entry, sort_keys=True): entry for entry in result}.values()
        )

    regressions = {}
    per_call = defaultdict(list)
    for number, (filename, name, claim, calls) in enumerate(REGRESSIONS, 1):
        path = ROOT / filename
        matches = [f for f in functions(path) if f["name"] == name]
        assert len(matches) == 1, (filename, name)
        identifier = f"R{number:02d}"
        regressions[identifier] = {
            "source": source(path),
            "line": matches[0]["line"],
            "test": name,
            "scope": claim,
            "execution": "Referenced by the separately reported full Rust suite; this inventory script does not run tests.",
        }
        for call in calls.split():
            per_call[call].append(identifier)
    intents = defaultdict(list)
    for name, cls in REGISTRY.items():
        for pair in cls.wraps:
            intents[pair].append({"op": name, "declared_origin": cls.origin})

    entries = []
    for pallet in pallets:
        if pallet.name not in COVERED_PALLETS:
            continue
        for call in pallet.calls:
            pair = (pallet.name, call.name)
            key = ".".join(pair)
            wrapped = intents[pair]
            raw = call.name in RAW_ONLY.get(pallet.name, set())
            assert bool(wrapped) != raw, (key, wrapped, raw)
            evidence = guards(*pair)
            snippets = " ".join(g["guard"] for g in evidence)
            if "unconditional_deprecated_rejection" in snippets:
                origin = "deprecated_rejects_every_origin"
            elif "origin_ignored_compatibility_no_op" in snippets:
                origin = "compatibility_no_op_origin_ignored"
            elif pallet.name == "Utility" and call.name in {
                "batch",
                "batch_all",
                "force_batch",
                "if_else",
            }:
                origin = "non_none_preserves_origin_and_filters"
            elif (
                "ensure_sn_owner_or_root" in snippets
                or "ensure_subnet_owner_or_root" in snippets
            ):
                origin = "root_or_authorized_subnet_owner"
            elif "ensure_subnet_owner(" in snippets:
                origin = "authorized_subnet_owner"
            elif "ensure_root" in snippets and "ensure_signed" not in snippets:
                origin = "root"
            elif "ensure_signed" in snippets and "ensure_root" not in snippets:
                origin = "signed_with_operation_checks"
            elif "ensure_address_origin" in snippets:
                origin = "configured_evm_address_origin"
            elif (
                "UploadOrigin::ensure_origin" in snippets
                or "InstantiateOrigin::ensure_origin" in snippets
            ):
                origin = "configured_contract_origin_signed_in_runtime"
            elif "from_runtime_origin" in snippets and pallet.name == "Contracts":
                origin = "contract_runtime_origin_adapter"
            elif "ensure_none" in snippets:
                origin = "none_inherent_or_unsigned"
            else:
                origin = "unresolved_requires_source_review"
            tests = per_call.get(key, [])
            signed = origin in {
                "signed_with_operation_checks",
                "root_or_authorized_subnet_owner",
                "authorized_subnet_owner",
                "configured_evm_address_origin",
                "configured_contract_origin_signed_in_runtime",
                "non_none_preserves_origin_and_filters",
            }
            coverage = (
                "targeted_regressions_with_limited_scopes"
                if tests
                else (
                    "generic_origin_path_inference_only"
                    if signed
                    else "existing_origin_restriction_or_unresolved_no_targeted_mapping"
                )
            )
            entries.append(
                {
                    "call": key,
                    "index": [pallet.index, call_indices[pair]],
                    "sdk": {
                        "status": "wrapped" if wrapped else "raw_only",
                        "intents": wrapped,
                    },
                    "required_origin": origin,
                    "origin_evidence": evidence,
                    "coverage": coverage,
                    "regressions": tests,
                    "activated_end_to_end": "not_executed",
                    "authorization_path": (
                        "ML-DSA envelope -> same Signed(AccountId) -> existing call/origin/policy checks; compatibility inference, not per-operation proof."
                        if signed
                        else "Existing origin restriction applies; ML-DSA authentication does not confer Root, inherent, or other privileged authority."
                    ),
                }
            )
    known = {entry["call"] for entry in entries}
    assert not (set(per_call) - known), sorted(set(per_call) - known)
    for path in [
        ROOT / "sdk/python/codegen/check.py",
        ROOT / "runtime/src/lib.rs",
        ROOT / "runtime/src/hashed_extrinsic.rs",
        ROOT / "runtime/src/evm_origin.rs",
    ]:
        source(path)
    result = {
        "schema_version": 1,
        "fixture_sha256": sha(fixture_bytes),
        "script_sha256": sha(Path(__file__).read_bytes()),
        "release_wasm_sha256": fixture.get("releaseWasmSha256"),
        "spec_version": fixture["specVersion"],
        "transaction_version": fixture["transactionVersion"],
        "scope": {
            "source": "sdk/python/codegen/check.py:COVERED_PALLETS and RAW_ONLY; bittensor.intents.REGISTRY",
            "pallets": list(COVERED_PALLETS),
            "calls": len(entries),
            "other_metadata_calls_outside_sdk_inventory": [
                f"{p.name}.{c.name}"
                for p in pallets
                if p.name not in COVERED_PALLETS
                for c in p.calls
            ],
        },
        "limits": [
            "Static metadata/source inventory, not a new execution run or complete operation-parity proof.",
            "Origin extraction follows explicit guards and same-pallet helpers receiving origin; unresolved/adapter paths require review. Existing business-state, proxy and security-floor restrictions still apply.",
            "Regressions use runtime/pallet test environments with explicit test weights. A targeted regression only proves the scope named in its entry.",
            "No call has been exercised as an activated production ML-DSA finalized transaction; compiled production weight/activation gates remain closed.",
            "No complete per-call direct/proxy/all-PQ/mixed-multisig matrix, historical-upgrade UI/indexer matrix, or physical hardware signing is established by this report.",
        ],
        "origin_configuration": {
            "EVM.CallOrigin_and_WithdrawOrigin": "runtime/src/lib.rs -> evm_origin::EnsureAddressTruncatedNonZero",
            "Contracts.UploadOrigin_and_InstantiateOrigin": "runtime/src/lib.rs -> EnsureSigned<AccountId>",
        },
        "summary": {
            "origin": dict(
                sorted(Counter(e["required_origin"] for e in entries).items())
            ),
            "coverage": dict(sorted(Counter(e["coverage"] for e in entries).items())),
            "sdk": dict(sorted(Counter(e["sdk"]["status"] for e in entries).items())),
            "activated_end_to_end_executed": 0,
        },
        "regressions": regressions,
        "calls": entries,
        "source_sha256": {
            source(path): sha(path.read_bytes()) for path in sorted(touched)
        },
    }
    output = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(output)
    else:
        print(output, end="")
    print(json.dumps(result["summary"], indent=2), file=__import__("sys").stderr)


if __name__ == "__main__":
    main()
