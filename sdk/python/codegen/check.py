"""CI checks for the generated layer.

- ``--drift <endpoint>``: regenerate from a live node and fail if the committed
  output differs (the "generated code must match the chain" gate).
- ``--names``: assert the error classification and the generated catalog agree
  in both directions — every name the SDK classifies still exists on chain (a
  rename is caught rather than silently degrading to UNKNOWN), and every name
  on chain classifies to a semantic code (a new runtime error must be
  deliberately mapped before it can ship).

Exit code 0 = ok, 1 = mismatch.
"""

from __future__ import annotations

import sys
from pathlib import Path

from .emit_python import artifacts
from .metadata import dump

OUT_DIR = Path(__file__).resolve().parent.parent / "bittensor" / "_generated"


def check_drift(endpoint: str) -> int:
    ir = dump(endpoint)
    mismatches = []
    for filename, content in artifacts(ir).items():
        path = OUT_DIR / filename
        if not path.exists() or path.read_text() != content:
            mismatches.append(filename)
    if mismatches:
        print(f"DRIFT: committed generated files differ from chain metadata: {mismatches}")
        print("Run: python -m codegen <endpoint>")
        return 1
    print("no drift: generated layer matches chain metadata")
    return 0


# Pallets whose calls must each have a deliberate status: wrapped by an intent,
# or listed here as raw-only (usable via substrate.compose, but no semantic wrapper).
COVERED_PALLETS = (
    "SubtensorModule",
    "Balances",
    "Proxy",
    "Utility",
    "Multisig",
    "Crowdloan",
    "Swap",
    "LimitOrders",
    "MevShield",
    "EVM",
)

RAW_ONLY: dict[str, set[str]] = {
    "SubtensorModule": {
        # sudo/admin/root-origin operations — deliberately not agent-executable
        "sudo_set_max_childkey_take",
        "sudo_set_min_childkey_take",
        "sudo_set_num_root_claims",
        "sudo_set_root_claim_threshold",
        "sudo_set_tx_childkey_take_rate_limit",
        "sudo_set_voting_power_ema_alpha",
        "set_tempo",
        "trigger_epoch",
        "dissolve_network",
        "root_dissolve_network",
        "set_activity_cutoff_factor",
        # legacy / superseded weight paths (mechanism variants are wrapped;
        # reveal_weights is wrapped by the RevealWeights intent for salt commits)
        "set_weights",
        "commit_weights",
        "commit_mechanism_weights",
        "reveal_mechanism_weights",
        "commit_crv3_mechanism_weights",
        "commit_timelocked_weights",
        "batch_set_weights",
        "batch_commit_weights",
        "batch_reveal_weights",
        # PoW registration — out of scope by design (faucet is testnet-only and
        # absent from the finney metadata this layer is generated against)
        "register",
        # coldkey swap: announce/execute/clear/dispute are wrapped by intents; these
        # remain raw — deprecated (schedule) or root-only (reset, arbitrary swap)
        "reset_coldkey_swap",
        "schedule_swap_coldkey",
        "swap_coldkey",
        "swap_hotkey_v2",
        # locks / liquidity-adjacent (lock_stake, move_lock, set_perpetual_lock
        # are wrapped by lock intents)
        "set_reject_locked_alpha",
        # alpha burn/recycle + buyback variants (add_stake_burn is wrapped)
        "burn_alpha",
        "recycle_alpha",
        "remove_stake_full_limit",
        "swap_stake_limit",
        "register_limit",
        # identity / metadata / misc (set_identity / set_subnet_identity /
        # update_symbol are wrapped)
        "register_network_with_identity",
        "set_auto_parent_delegation_enabled",
        "set_pending_childkey_cooldown",
        "enable_voting_power_tracking",
        "disable_voting_power_tracking",
    },
    "Balances": {
        # force_* are root-origin; burn/upgrade are niche
        "burn",
        "force_adjust_total_issuance",
        "force_set_balance",
        "force_transfer",
        "force_unreserve",
        "upgrade_accounts",
    },
    "Utility": {
        # non-atomic variants — the Batch intent wraps batch_all (atomic) only;
        # partial application is a footgun for agents
        "batch",
        "force_batch",
        # origin/dispatch plumbing — niche
        "as_derivative",
        "dispatch_as",
        "dispatch_as_fallible",
        "if_else",
        "with_weight",
    },
    "Multisig": {
        # deposit maintenance — niche; the four multisig operations are wrapped
        "poke_deposit",
    },
    "Swap": {
        # all user LP calls are DEPRECATED on-chain (per runtime metadata docs);
        # the rest are subnet-owner/admin. Reachable via the raw-call escape hatch.
        "add_liquidity",
        "modify_position",
        "remove_liquidity",
        "disable_lp",
        "set_fee_rate",
        "toggle_user_liquidity",
    },
    "LimitOrders": {
        # signed Order payloads (constructed off-chain) and admin-gated execution;
        # not a fit for a declarative intent. Reachable via the raw-call escape hatch.
        "cancel_order",
        "execute_orders",
        "execute_batched_orders",
        "set_pallet_status",
    },
    "MevShield": {
        # submit_encrypted is exposed via client.submit_shielded (a submission mode,
        # not a declarative intent — it encrypts a signed inner extrinsic). The rest
        # are the per-block inherent (announce_next_key), admin config setters, and
        # the low-level store_encrypted.
        "submit_encrypted",
        "store_encrypted",
        "announce_next_key",
        "set_max_extrinsic_weight",
        "set_max_pending_extrinsics_number",
        "set_on_initialize_weight",
        "set_stored_extrinsic_lifetime",
    },
    "EVM": {
        # raw EVM execution from a substrate origin — Ethereum tooling (or the
        # `btcli evm` group's JSON-RPC path) is the right way to run EVM code;
        # withdraw is wrapped by the EvmWithdraw intent
        "call",
        "create",
        "create2",
        # deployment whitelist admin (root/sudo; localnet setup uses these via
        # the raw-call escape hatch)
        "disable_whitelist",
        "set_whitelist",
    },
    "Proxy": {
        # the dispatch wrapper itself — used by the executor's proxy signing
        # mode (proxy_for=...), not expressed as a standalone intent
        "proxy",
        # announced-proxy flow (non-zero delay): proxy_announced is wrapped;
        # announce/reject/remove remain raw
        "announce",
        "reject_announcement",
        "remove_announcement",
        # pure (keyless) proxies are wrapped (create_pure/kill_pure); deposit
        # maintenance stays raw — niche
        "poke_deposit",
        "set_real_pays_fee",
    },
}


def check_coverage() -> int:
    from bittensor._generated import calls as generated_calls
    from bittensor.intents import REGISTRY

    wrapped = {pair for cls in REGISTRY.values() for pair in cls.wraps}
    problems: list[str] = []
    for pallet in COVERED_PALLETS:
        cls = getattr(generated_calls, pallet)
        chain_calls = {n for n, v in vars(cls).items() if isinstance(v, staticmethod)}
        raw_only = RAW_ONLY.get(pallet, set())

        missing = sorted(n for n in chain_calls if (pallet, n) not in wrapped and n not in raw_only)
        stale = sorted(n for n in raw_only if n not in chain_calls)
        both = sorted(n for n in raw_only if (pallet, n) in wrapped)
        if missing:
            problems.append(f"{pallet}: calls with no status (wrap or list raw-only): {missing}")
        if stale:
            problems.append(f"{pallet}: raw-only entries no longer on chain: {stale}")
        if both:
            problems.append(f"{pallet}: listed raw-only but also wrapped: {both}")

    if problems:
        for p in problems:
            print(f"COVERAGE: {p}")
        return 1
    total = sum(
        len(
            {n for n, v in vars(getattr(generated_calls, p)).items() if isinstance(v, staticmethod)}
        )
        for p in COVERED_PALLETS
    )
    print(f"coverage ok: all {total} calls in {COVERED_PALLETS} are wrapped or explicitly raw-only")
    return 0


def check_names() -> int:
    from bittensor._generated.errors import ERRORS
    from bittensor.error_map import NAME_TO_CODE, ErrorCode
    from bittensor.result import classify_error

    catalog = {info.name for info in ERRORS.values()}
    stale = sorted(name for name in NAME_TO_CODE if name not in catalog)
    unclassified = sorted(name for name in catalog if classify_error("", name) is ErrorCode.UNKNOWN)
    if stale:
        print(f"STALE: error names classified by the SDK but absent from chain: {stale}")
    if unclassified:
        print(
            "UNCLASSIFIED: chain error names with no semantic code "
            f"(add them to bittensor/error_map.py): {unclassified}"
        )
    if stale or unclassified:
        return 1
    print(
        f"names ok: all {len(catalog)} chain error names classify to a semantic code "
        "and no mapped name is stale"
    )
    return 0


def main() -> None:
    args = sys.argv[1:]
    if not args:
        print("usage: python -m codegen.check --names | --coverage | --drift <endpoint>")
        raise SystemExit(2)
    if args[0] == "--names":
        raise SystemExit(check_names())
    if args[0] == "--coverage":
        raise SystemExit(check_coverage())
    if args[0] == "--drift":
        endpoint = args[1] if len(args) > 1 else "ws://127.0.0.1:9944"
        raise SystemExit(check_drift(endpoint))
    print(f"unknown option: {args[0]}")
    raise SystemExit(2)


if __name__ == "__main__":
    main()
