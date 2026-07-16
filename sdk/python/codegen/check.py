"""CI checks for the generated layer.

- ``--drift <endpoint>``: regenerate from a live node and fail if the committed
  output differs (the "generated code must match the chain" gate).
- ``--names``: assert the error classification and the generated catalog agree
  in both directions — every name the SDK classifies still exists on chain (a
  rename is caught rather than silently degrading to UNKNOWN), and every name
  on chain classifies to a semantic code (a new runtime error must be
  deliberately mapped before it can ship).
- ``--units``: assert the hyperparameter hand table agrees with the unit each
  parameter's storage value type identity dictates. Pre-newtype metadata
  carries no unit-bearing identities, so the gate passes trivially there; it
  bites after a regen against a runtime with PerU16/TaoBalance newtypes.
- ``--namespaces``: assert the committed ``bittensor/namespaces.pyi`` stub
  matches what the read registry generates — a read added without regenerating
  the stub (or a category with no namespace class) fails here.

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
    expected = artifacts(ir)
    mismatches = []
    for filename, content in expected.items():
        path = OUT_DIR / filename
        if not path.exists() or path.read_text() != content:
            mismatches.append(filename)
    orphans = sorted(
        p.name
        for p in OUT_DIR.iterdir()
        if p.is_file() and p.name not in expected and not p.name.startswith(".")
    )
    if mismatches:
        print(f"DRIFT: committed generated files differ from chain metadata: {mismatches}")
        print("Run: python -m codegen <endpoint>")
    if orphans:
        print(f"DRIFT: files in {OUT_DIR} not produced by codegen (delete them): {orphans}")
    if mismatches or orphans:
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
    "Commitments",
    "AdminUtils",
    "Contracts",
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
        "trigger_epoch",
        "dissolve_network",
        "root_dissolve_network",
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
        "register_limit",
        # identity / metadata / misc (set_identity / set_subnet_identity /
        # update_symbol are wrapped)
        "register_network_with_identity",
        "set_auto_parent_delegation_enabled",
        "set_pending_childkey_cooldown",
        "enable_voting_power_tracking",
        "disable_voting_power_tracking",
        # Deprecated no-op compatibility stubs (call indices 139/140). Functional
        # updates go through AdminUtils.sudo_set_tempo / sudo_set_activity_cutoff_factor.
        "set_tempo",
        "set_activity_cutoff_factor",
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
    "Commitments": {
        # hotkey-origin commitment payload; no semantic intent yet (raw-call /
        # governance e2e). Fee routes to the owning coldkey when non-zero.
        "set_commitment",
        # root/sudo storage limit
        "set_max_space",
    },
    "AdminUtils": {
        # root/sudo-origin chain administration — deliberately not agent-executable.
        # Enumerated explicitly (not computed) so a newly added unwrapped call is
        # flagged as missing and requires a deliberate wrap-or-raw-only decision.
        "schedule_grandpa_change",
        # legacy absolute-blocks cutoff — superseded by the owner-settable
        # `sudo_set_activity_cutoff_factor` (per-mille of tempo), which the
        # SetHyperparameter intent wraps as `activity_cutoff_factor`
        "sudo_set_activity_cutoff",
        "sudo_set_adjustment_interval",
        "sudo_set_admin_freeze_window",
        "sudo_set_ck_burn",
        "sudo_set_coldkey_swap_announcement_delay",
        "sudo_set_coldkey_swap_reannouncement_delay",
        "sudo_set_commit_reveal_version",
        "sudo_set_default_take",
        "sudo_set_difficulty",
        "sudo_set_dissolve_network_schedule_duration",
        "sudo_set_ema_price_halving_period",
        "sudo_set_evm_chain_id",
        "sudo_set_kappa",
        "sudo_set_lock_reduction_interval",
        "sudo_set_max_allowed_validators",
        "sudo_set_max_epochs_per_block",
        "sudo_set_max_mechanism_count",
        # unused uneven-split setter — removed from the intent/CLI surface;
        # still reachable via `btcli call` if needed
        "sudo_set_mechanism_emission_split",
        "sudo_set_max_registrations_per_block",
        "sudo_set_min_allowed_uids",
        "sudo_set_min_delegate_take",
        "sudo_set_min_difficulty",
        "sudo_set_min_non_immune_uids",
        "sudo_set_net_tao_flow_enabled",
        "sudo_set_network_immunity_period",
        "sudo_set_network_min_lock_cost",
        "sudo_set_network_rate_limit",
        "sudo_set_network_registration_allowed",
        "sudo_set_nominator_min_required_stake",
        "sudo_set_owner_hparam_rate_limit",
        "sudo_set_rao_recycled",
        "sudo_set_recycle_or_burn",
        "sudo_set_sn_owner_hotkey",
        "sudo_set_stake_threshold",
        "sudo_set_start_call_delay",
        "sudo_set_subnet_limit",
        "sudo_set_subnet_moving_alpha",
        "sudo_set_subnet_owner_cut",
        "sudo_set_subtoken_enabled",
        "sudo_set_tao_flow_cutoff",
        "sudo_set_tao_flow_normalization_exponent",
        "sudo_set_tao_flow_smoothing_factor",
        "sudo_set_target_registrations_per_interval",
        "sudo_set_total_issuance",
        "sudo_set_tx_delegate_take_rate_limit",
        "sudo_set_tx_rate_limit",
        "sudo_set_weights_set_rate_limit",
        "sudo_toggle_evm_precompile",
        "swap_authorities",
    },
    "Contracts": {
        # ink/WASM contract execution — use Ethereum tooling or the raw-call
        # escape hatch; deprecated *_old_weight variants stay for on-chain Call
        # migration only
        "call",
        "call_old_weight",
        "instantiate",
        "instantiate_old_weight",
        "instantiate_with_code",
        "instantiate_with_code_old_weight",
        "migrate",
        "remove_code",
        "set_code",
        "upload_code",
    },
}


def check_coverage() -> int:
    from bittensor._generated import calls as generated_calls
    from bittensor.intents import REGISTRY
    from bittensor.reads import REGISTRY as READS

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

    # Verify integrity: a declared verify must name a registered read, and a
    # root-origin write with no paired verify read is a docs bug.
    for op, intent_cls in sorted(REGISTRY.items()):
        if intent_cls.verify is not None and intent_cls.verify not in READS:
            problems.append(
                f"{op}: verify names {intent_cls.verify!r}, which is not a registered read"
            )
        if intent_cls.origin == "root" and intent_cls.verify is None:
            problems.append(f"{op}: origin is 'root' but no verify read is declared")

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
    from bittensor.error_descriptions import DESCRIPTIONS
    from bittensor.error_map import NAME_TO_CODE, ErrorCode
    from bittensor.result import classify_error

    catalog = {info.name for info in ERRORS.values()}
    stale = sorted(name for name in NAME_TO_CODE if name not in catalog)
    unclassified = sorted(name for name in catalog if classify_error("", name) is ErrorCode.UNKNOWN)
    undescribed = sorted(name for name in NAME_TO_CODE if name not in DESCRIPTIONS)
    orphan_descriptions = sorted(name for name in DESCRIPTIONS if name not in NAME_TO_CODE)
    if stale:
        print(f"STALE: error names classified by the SDK but absent from chain: {stale}")
    if unclassified:
        print(
            "UNCLASSIFIED: chain error names with no semantic code "
            f"(add them to bittensor/error_map.py): {unclassified}"
        )
    if undescribed:
        print(
            "UNDESCRIBED: classified error names with no description "
            f"(add them under bittensor/error_descriptions/<pallet>.py): {undescribed}"
        )
    if orphan_descriptions:
        print(
            "ORPHANED: described error names no longer classified "
            f"(remove them from bittensor/error_descriptions/<pallet>.py): {orphan_descriptions}"
        )
    empty = sorted(name for name, text in DESCRIPTIONS.items() if not text.strip())
    if empty:
        print(
            "EMPTY: described error names with blank prose "
            f"(fill them in bittensor/error_descriptions/<pallet>.py): {empty}"
        )
    if stale or unclassified or undescribed or orphan_descriptions or empty:
        return 1
    print(
        f"names ok: all {len(catalog)} chain error names classify to a semantic code, "
        "every classified name is described (non-empty), and no mapped name is stale"
    )
    return 0


def check_namespaces() -> int:
    from .emit_namespaces import OUT_PATH, generate

    expected = generate()
    if not OUT_PATH.exists() or OUT_PATH.read_text() != expected:
        print("DRIFT: bittensor/namespaces.pyi is stale vs the read registry")
        print("Run: uv run python -m codegen.emit_namespaces")
        return 1
    from bittensor.reads import REGISTRY

    print(f"namespaces ok: namespaces.pyi covers all {len(REGISTRY)} registered reads")
    return 0


def check_units() -> int:
    from bittensor import hyperparams

    problems: list[str] = []
    derived = 0
    for name, meta in hyperparams.HYPERPARAMS.items():
        if not meta.short:
            problems.append(f"{name}: no short description (the listing's blurb column)")
        elif len(meta.short) > hyperparams.SHORT_MAX:
            problems.append(
                f"{name}: short description is {len(meta.short)} chars "
                f"(max {hyperparams.SHORT_MAX}): {meta.short!r}"
            )
        metadata_kind = hyperparams.metadata_kind(name)
        if metadata_kind is None:
            # No dedicated storage value, or the (pre-newtype) metadata
            # carries no unit-bearing identity — the hand table stands alone.
            continue
        derived += 1
        if metadata_kind != meta.kind:
            item = hyperparams.STORAGE_ITEMS[name]
            ident = getattr(item, "value_type_ident", "")
            problems.append(
                f"{name}: hand table says {meta.kind!r} but storage "
                f"{item.container}.{item.name} is {ident!r} (kind {metadata_kind!r})"
            )
    if problems:
        for p in problems:
            print(f"UNITS: {p}")
        return 1
    print(
        f"units ok: {derived} metadata-derived hyperparameter kinds agree with the hand "
        f"table ({len(hyperparams.HYPERPARAMS) - derived} carry no unit-bearing identity), "
        f"and all {len(hyperparams.HYPERPARAMS)} carry a short description"
    )
    return 0


def main() -> None:
    args = sys.argv[1:]
    if not args:
        print(
            "usage: python -m codegen.check "
            "--names | --coverage | --units | --namespaces | --drift <endpoint>"
        )
        raise SystemExit(2)
    if args[0] == "--names":
        raise SystemExit(check_names())
    if args[0] == "--coverage":
        raise SystemExit(check_coverage())
    if args[0] == "--units":
        raise SystemExit(check_units())
    if args[0] == "--namespaces":
        raise SystemExit(check_namespaces())
    if args[0] == "--drift":
        endpoint = args[1] if len(args) > 1 else "ws://127.0.0.1:9944"
        raise SystemExit(check_drift(endpoint))
    print(f"unknown option: {args[0]}")
    raise SystemExit(2)


if __name__ == "__main__":
    main()
