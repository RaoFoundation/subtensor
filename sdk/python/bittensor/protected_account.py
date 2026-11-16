"""Protected-account local config and allow-list mapping.

A protected account is a keyless pure proxy whose only full controller is the
user's wallet (single coldkey or multisig), granted with a non-zero announcement
delay. Day-to-day value moves are announced and wait out that delay; optional
``--allow`` items become zero-delay restricted proxies for instant ops.
"""

from __future__ import annotations

import json
import os
import re
from hashlib import blake2b
from pathlib import Path
from typing import Any, Optional

from ._transport.codec import ss58_decode, ss58_encode
from .receiving import is_receiving_address, parse_recipient
from .settings import BLOCKTIME, SS58_FORMAT

DEFAULT_PROTECTED_PATH = Path.home() / ".bittensor" / "btcli_protected.json"

# User-facing --allow tokens -> existing ProxyType names. Only types that cannot
# transfer free balance, manage proxies, or run a coldkey swap are accepted.
ALLOW_ITEM_TO_PROXY_TYPE: dict[str, str] = {
    "staking": "Staking",
    "childkeys": "ChildKeys",
    "childkey": "ChildKeys",
    "basket": "BasketTrading",
    "basket-swaps": "BasketTrading",
    "basket_swaps": "BasketTrading",
    "gamma": "GammaSpend",
    "root-claim": "RootClaim",
    "root_claim": "RootClaim",
    "rootclaim": "RootClaim",
    "validate": "Validate",
    "weights": "Weights",
    "owner": "Owner",
    "swap-hotkey": "SwapHotkey",
    "swap_hotkey": "SwapHotkey",
    "subnet-lease": "SubnetLeaseBeneficiary",
    "subnet_lease": "SubnetLeaseBeneficiary",
}

# Tokens that would defeat the delay model if granted as zero-delay allows.
FORBIDDEN_ALLOW_ITEMS: dict[str, str] = {
    "stake": (
        "stake sounds narrower than the full Staking proxy authority; use "
        "'staking' after reviewing its complete permissions"
    ),
    "unstake": (
        "unstake sounds narrower than the full Staking proxy authority; use "
        "'staking' after reviewing its complete permissions"
    ),
    "registration": (
        "Registration can burn protected-account TAO immediately and is not "
        "offered as a protected-account allow"
    ),
    "lock": (
        "Lock can freeze protected-account stake permanently and is not offered "
        "as a protected-account allow"
    ),
    "any": "Any can transfer and change proxies immediately",
    "transfer": "Transfer moves free balance immediately",
    "smalltransfer": "SmallTransfer moves free balance immediately",
    "small-transfer": "SmallTransfer moves free balance immediately",
    "small_transfer": "SmallTransfer moves free balance immediately",
    "nontransfer": "NonTransfer can manage proxies (and defeat the delay)",
    "non-transfer": "NonTransfer can manage proxies (and defeat the delay)",
    "non_transfer": "NonTransfer can manage proxies (and defeat the delay)",
    "noncritical": "NonCritical can transfer free balance immediately",
    "non-critical": "NonCritical can transfer free balance immediately",
    "non_critical": "NonCritical can transfer free balance immediately",
}

# Intent ops that a given ProxyType can cover for the instant (delay-0) path.
# Proxy management / transfers / coldkey swap are intentionally absent: those
# always go through the delayed Any controller.
PROXY_TYPE_INTENT_OPS: dict[str, frozenset[str]] = {
    "Staking": frozenset(
        {
            "add_stake",
            "remove_stake",
            "move_stake",
            "swap_stake",
            "add_stake_limit",
            "remove_stake_limit",
            "unstake_all",
            "unstake_all_alpha",
            "stake_into_basket",
            "claim_root",
            "claim_root_with_hotkey",
        }
    ),
    "ChildKeys": frozenset({"set_children", "set_childkey_take"}),
    "BasketTrading": frozenset({"swap_basket", "swap_basket_many"}),
    "Registration": frozenset({"burned_register", "pow_register"}),
    "Lock": frozenset({"lock_stake", "move_lock", "set_perpetual_lock"}),
    "GammaSpend": frozenset({"spend_gamma"}),
    "RootClaim": frozenset({"claim_root", "claim_root_with_hotkey"}),
    "Validate": frozenset(
        {
            "serve_axon",
            "serve_axon_tls",
            "serve_prometheus",
            "reset_axon",
            "associate_evm_key",
        }
    ),
    "Weights": frozenset({"set_weights", "commit_weights", "reveal_weights"}),
    "Owner": frozenset({"set_hyperparameter", "set_subnet_identity"}),
    "SwapHotkey": frozenset({"swap_hotkey"}),
    "SubnetLeaseBeneficiary": frozenset(
        {"start_call", "set_subnet_identity", "set_hyperparameter", "set_service"}
    ),
}

# Zero-delay types that defeat the announcement delay if also held by the controller.
DEFEATING_ZERO_DELAY_TYPES = frozenset(
    {"Any", "NonCritical", "Transfer", "SmallTransfer", "NonTransfer"}
)

_DURATION_RE = re.compile(
    r"^\s*(\d+(?:\.\d+)?)\s*(h|hr|hrs|hour|hours|d|day|days|m|min|mins|minute|minutes|s|sec|secs|second|seconds|b|block|blocks)?\s*$",
    re.IGNORECASE,
)


def protected_path() -> Path:
    return Path(os.getenv("BTCLI_PROTECTED_PATH") or DEFAULT_PROTECTED_PATH)


def load_protected() -> list[dict[str, Any]]:
    path = protected_path()
    if not path.is_file():
        return []
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return []
    return data if isinstance(data, list) else []


def save_protected(entries: list[dict[str, Any]]) -> Path:
    path = protected_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    try:
        tmp.write_text(json.dumps(entries, indent=2) + "\n")
        os.replace(tmp, path)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
    return path


def _scope_matches(entry: dict[str, Any], genesis_hash: Optional[str]) -> bool:
    """Match one chain exactly; an omitted scope selects legacy entries only."""
    stored = entry.get("genesis_hash")
    return (str(stored).lower() if stored is not None else None) == (
        str(genesis_hash).lower() if genesis_hash is not None else None
    )


def get_by_controller(
    wallet_name: str, *, genesis_hash: Optional[str] = None
) -> Optional[dict[str, Any]]:
    """Find a wallet's record on one chain, without falling back to unscoped data."""
    for entry in load_protected():
        if entry.get("controller_wallet") == wallet_name and _scope_matches(entry, genesis_hash):
            return entry
    return None


def get_by_protected_ss58(
    ss58: str, *, genesis_hash: Optional[str] = None
) -> Optional[dict[str, Any]]:
    for entry in load_protected():
        if _account_identity(entry.get("protected_ss58")) == _account_identity(
            ss58
        ) and _scope_matches(entry, genesis_hash):
            return entry
    return None


def upsert_protected(entry: dict[str, Any]) -> dict[str, Any]:
    wallet = entry.get("controller_wallet")
    if not wallet:
        raise ValueError("protected entry requires controller_wallet")
    if not entry.get("protected_ss58"):
        raise ValueError("protected entry requires protected_ss58")
    genesis_hash = entry.get("genesis_hash")
    if genesis_hash is not None:
        if not genesis_hash:
            raise ValueError("protected entry genesis_hash must not be empty")
        entry = {**entry, "genesis_hash": str(genesis_hash).lower()}
    entries = [
        e
        for e in load_protected()
        if not (e.get("controller_wallet") == wallet and _scope_matches(e, genesis_hash))
    ]
    entries.append(entry)
    save_protected(entries)
    return entry


def remove_protected(wallet_name: str, *, genesis_hash: Optional[str] = None) -> bool:
    entries = load_protected()
    filtered = [
        e
        for e in entries
        if not (e.get("controller_wallet") == wallet_name and _scope_matches(e, genesis_hash))
    ]
    existed = len(filtered) != len(entries)
    save_protected(filtered)
    return existed


def move_protected_controller(
    from_wallet: str,
    to_wallet: str,
    *,
    controller_ss58: Optional[str] = None,
    genesis_hash: Optional[str] = None,
) -> Optional[dict[str, Any]]:
    """Rebind the local record after a successful controller rotate.

    ``controller_ss58`` must be the new controller's on-chain address when known:
    announcement reconcile keys off that address, so leaving the old ss58 would
    hide the new controller's pending calls. A destination bound to another pure
    on the same chain is refused before either binding is changed.
    """
    entries = load_protected()
    entry = next(
        (
            row
            for row in entries
            if row.get("controller_wallet") == from_wallet and _scope_matches(row, genesis_hash)
        ),
        None,
    )
    if entry is None:
        return None
    for destination in entries:
        if (
            destination.get("controller_wallet") == to_wallet
            and _scope_matches(destination, genesis_hash)
            and _account_identity(destination.get("protected_ss58"))
            != _account_identity(entry["protected_ss58"])
        ):
            raise ValueError(
                f"wallet {to_wallet!r} already controls protected account "
                f"{destination.get('protected_ss58')} on this network; "
                "both bindings have been preserved"
            )
    updated = {**entry, "controller_wallet": to_wallet}
    if controller_ss58:
        updated["controller_ss58"] = controller_ss58
    retained = [
        row
        for row in entries
        if not (
            row.get("controller_wallet") in (from_wallet, to_wallet)
            and _scope_matches(row, genesis_hash)
        )
    ]
    save_protected([*retained, updated])
    return updated


def bind_legacy_controller(wallet_name: str, genesis_hash: str) -> Optional[dict[str, Any]]:
    """Bind a legacy record after explicit user choice and live-chain validation.

    This performs no chain validation itself. Existing scoped and unscoped
    records are never silently merged or replaced during migration.
    """
    if not genesis_hash:
        raise ValueError("binding a legacy protected account requires a genesis_hash")
    entries = load_protected()
    scoped = get_by_controller(wallet_name, genesis_hash=genesis_hash)
    for i, entry in enumerate(entries):
        if entry.get("controller_wallet") != wallet_name or not _scope_matches(entry, None):
            continue
        if scoped is not None:
            raise ValueError(
                "this wallet already has a protected account on this network; "
                "the legacy and scoped records have both been preserved"
            )
        updated = {**entry, "genesis_hash": str(genesis_hash).lower()}
        entries[i] = updated
        save_protected(entries)
        return updated
    return scoped


# SCALE index of ``ProxyType::Any`` in ``common/src/proxy.rs`` (first variant).
_PROXY_TYPE_ANY_SCALE = 0
_LEGACY_PURE_PREFIX = b"modlpy/proxy____"  # [u8; 16], no length prefix when encoded


def derive_legacy_pure_account_id(
    spawner_public_key: bytes,
    *,
    height: int,
    ext_index: int,
    proxy_type_scale: int = _PROXY_TYPE_ANY_SCALE,
    index: int = 0,
) -> bytes:
    """Recompute a legacy ``Proxy::create_pure`` AccountId (blake2_256 of SCALE fields)."""
    if len(spawner_public_key) != 32:
        raise ValueError("spawner public key must be 32 bytes")
    payload = (
        _LEGACY_PURE_PREFIX
        + spawner_public_key
        + int(height).to_bytes(4, "little")
        + int(ext_index).to_bytes(4, "little")
        + bytes([int(proxy_type_scale) & 0xFF])
        + int(index).to_bytes(2, "little")
    )
    return blake2b(payload, digest_size=32).digest()


def derive_legacy_pure_ss58(
    spawner_ss58: str,
    *,
    height: int,
    ext_index: int,
    proxy_type: str = "Any",
    index: int = 0,
    ss58_format: Optional[int] = None,
) -> str:
    """SS58 form of :func:`derive_legacy_pure_account_id` for ``ProxyType::Any`` only."""
    if proxy_type != "Any":
        raise ValueError(f"local pure recompute only supports ProxyType::Any (got {proxy_type!r})")
    public = bytes.fromhex(ss58_decode(spawner_ss58))
    account = derive_legacy_pure_account_id(public, height=height, ext_index=ext_index, index=index)
    return ss58_encode(account, ss58_format if ss58_format is not None else SS58_FORMAT)


def parse_delay_to_blocks(raw: str | int, *, blocktime: float = BLOCKTIME) -> int:
    """Parse ``36h`` / ``2d`` / ``10800`` (blocks) into a block count.

    Bare integers are blocks. Suffixed values use ``blocktime`` seconds/block
    (default 12s). Raises ``ValueError`` on empty/invalid input or a result of 0.
    """
    if isinstance(raw, int):
        if raw <= 0:
            raise ValueError("delay must be greater than 0 blocks")
        return raw
    text = str(raw).strip()
    match = _DURATION_RE.match(text)
    if not match:
        raise ValueError(
            f"invalid delay {raw!r}; use blocks (e.g. 10800) or a duration (e.g. 36h, 2d, 90m)"
        )
    amount = float(match.group(1))
    unit = (match.group(2) or "b").lower()
    if unit in ("b", "block", "blocks"):
        blocks = int(amount)
    else:
        seconds_per_unit = {
            "s": 1,
            "sec": 1,
            "secs": 1,
            "second": 1,
            "seconds": 1,
            "m": 60,
            "min": 60,
            "mins": 60,
            "minute": 60,
            "minutes": 60,
            "h": 3600,
            "hr": 3600,
            "hrs": 3600,
            "hour": 3600,
            "hours": 3600,
            "d": 86400,
            "day": 86400,
            "days": 86400,
        }[unit]
        blocks = round(amount * seconds_per_unit / blocktime)
    if blocks <= 0:
        raise ValueError(
            "delay must resolve to more than 0 blocks (a protected account cannot use delay 0)"
        )
    return blocks


def resolve_allow_item(item: str) -> str:
    """Map a user ``--allow`` token to a ProxyType, or raise with a clear reason."""
    key = item.strip().lower().replace(" ", "-")
    if key in FORBIDDEN_ALLOW_ITEMS:
        raise ValueError(
            f"--allow {item!r} refused: {FORBIDDEN_ALLOW_ITEMS[key]}. "
            "Transfers and proxy/swap changes must go through the delayed controller."
        )
    proxy_type = ALLOW_ITEM_TO_PROXY_TYPE.get(key)
    if proxy_type is None:
        known = ", ".join(sorted(set(ALLOW_ITEM_TO_PROXY_TYPE)))
        raise ValueError(
            f"--allow {item!r} has no suitably narrow existing ProxyType. "
            f"Known allow items: {known}. "
            "Finer proxy types are a separate later job — refusing rather than "
            "granting something broader."
        )
    return proxy_type


def resolve_allow_items(items: list[str]) -> list[str]:
    """Deduplicate allow tokens into ProxyType names (stable order)."""
    out: list[str] = []
    seen: set[str] = set()
    for item in items:
        proxy_type = resolve_allow_item(item)
        if proxy_type not in seen:
            seen.add(proxy_type)
            out.append(proxy_type)
    return out


def _account_identity(address: Optional[str]) -> Optional[str]:
    """Compare receiving descriptors by their chain account, retaining legacy inputs."""
    return (
        parse_recipient(address).account
        if isinstance(address, str) and is_receiving_address(address)
        else address
    )


def instant_proxy_type_for(
    intent_op: str,
    record: dict[str, Any],
    *,
    dispatcher_ss58: Optional[str] = None,
    intent: Optional[Any] = None,
) -> Optional[str]:
    """Return the zero-delay allow ProxyType covering ``intent_op``, if usable now.

    Only confirmed ``allows`` (not ``pending_allows``) are considered. When
    ``--allow-key`` points at a different address than the dispatcher, the
    controller cannot use the instant path — the grant lives on the allow-key.
    Pass the intent when routing so argument-dependent calls can be checked too.
    """
    # These root-claim variants build Utility.batch_all, which Staking does
    # not admit even though it allows the individual calls in the batch.
    if intent_op in {"remove_stake", "remove_stake_limit", "move_stake", "unstake_all"} and getattr(
        intent, "claim", False
    ):
        return None
    # This legacy catalog entry calls a RootConfigCalls setter, not the
    # SubnetManagementCalls group admitted by Owner/SubnetLeaseBeneficiary.
    if (
        intent_op == "set_hyperparameter"
        and getattr(intent, "name", None) == "registration_allowed"
    ):
        return None
    allow_key = record.get("allow_key_ss58")
    if (
        allow_key
        and dispatcher_ss58
        and _account_identity(allow_key) != _account_identity(dispatcher_ss58)
    ):
        return None
    allows = record.get("allows") or []
    for proxy_type in allows:
        ops = PROXY_TYPE_INTENT_OPS.get(proxy_type, frozenset())
        if intent_op in ops:
            return proxy_type
    return None


def defeating_zero_delay_proxies(
    delegations: list[dict[str, Any]], controller_ss58: str
) -> list[str]:
    """Proxy types on the protected account that let ``controller`` skip the delay."""
    hits: list[str] = []
    for row in delegations:
        if _account_identity(row.get("delegate")) != _account_identity(controller_ss58):
            continue
        if int(row.get("delay") or 0) != 0:
            continue
        proxy_type = str(row.get("proxy_type") or "")
        if proxy_type in DEFEATING_ZERO_DELAY_TYPES:
            hits.append(proxy_type)
    return hits


def controller_grants_before_delay(
    delegations: list[dict[str, Any]],
    controller_ss58: str,
    *,
    delay_blocks: int,
) -> list[dict[str, Any]]:
    """Controller grants whose delay is strictly shorter than ``delay_blocks``.

    ``Proxy.find_proxy`` can select a shorter matching ``Any`` grant even when a
    longer one also exists, so protect must refuse reuse whenever the controller
    retains any earlier execution path.
    """
    return [
        row
        for row in delegations
        if _account_identity(row.get("delegate")) == _account_identity(controller_ss58)
        and int(row.get("delay") or 0) < int(delay_blocks)
    ]


def controller_any_delays(delegations: list[dict[str, Any]], controller_ss58: str) -> list[int]:
    """Delays of every ``Any`` grant held by ``controller_ss58`` (stable order)."""
    return [
        int(row.get("delay") or 0)
        for row in delegations
        if _account_identity(row.get("delegate")) == _account_identity(controller_ss58)
        and row.get("proxy_type") == "Any"
    ]


def controller_holds_allows(record: dict[str, Any], controller_ss58: str) -> bool:
    """True when confirmed/pending allows are granted to the controller itself.

    A distinct ``allow_key_ss58`` keeps restricted grants off the controller, so
    rotation must not move or revoke them. When unset or equal to the controller,
    zero-delay allows live on the controller and must migrate with it.
    """
    allow_key = record.get("allow_key_ss58")
    if not allow_key:
        return True
    return _account_identity(allow_key) == _account_identity(controller_ss58)


def controller_owned_allow_types(record: dict[str, Any], controller_ss58: str) -> list[str]:
    """Confirmed allow ProxyTypes held by ``controller_ss58`` (stable order)."""
    if not controller_holds_allows(record, controller_ss58):
        return []
    out: list[str] = []
    seen: set[str] = set()
    for proxy_type in record.get("allows") or []:
        name = str(proxy_type)
        if name and name not in seen:
            seen.add(name)
            out.append(name)
    return out


def allow_key_grants(
    delegations: list[dict[str, Any]], allow_key_ss58: str
) -> list[dict[str, Any]]:
    """Zero-delay restricted grants held by ``allow_key_ss58`` (excludes delayed Any)."""
    out: list[dict[str, Any]] = []
    for row in delegations:
        if _account_identity(row.get("delegate")) != _account_identity(allow_key_ss58):
            continue
        if str(row.get("proxy_type") or "") == "Any":
            continue
        if int(row.get("delay") or 0) != 0:
            continue
        out.append(row)
    return out


def protected_setup_pending(record: dict[str, Any]) -> bool:
    """Incomplete setup must be resumed before automatic account routing."""
    setup = record.get("setup")
    if setup is not None:
        return setup.get("status") != "ready"
    return (record.get("coldkey_swap") or {}).get("status") == "pending"


def announcement_proxy_grants(row: dict[str, Any]) -> list[dict[str, Any]]:
    """Find authority grants in saved payloads, independently of display categories."""
    grants: list[dict[str, Any]] = []

    def visit(encoded: dict[str, Any]) -> None:
        op = encoded.get("op")
        if op == "add_proxy":
            grants.append(
                {
                    "delegate_ss58": encoded.get("delegate_ss58"),
                    "proxy_type": encoded.get("proxy_type", "Staking"),
                    "delay": int(encoded.get("delay") or 0),
                }
            )
        elif op == "batch":
            for child in encoded.get("intents") or []:
                visit(child)
        elif op in ("execute_proxy_announced",):
            visit({**(encoded.get("inner_args") or {}), "op": encoded.get("inner_op")})

    op = row.get("inner_op")
    args = dict(row.get("inner_args") or {})
    # Older local records may only have a category and AddProxy arguments.
    if op is None and str(row.get("category") or "").startswith("allow:"):
        op = "add_proxy"
        args.setdefault("proxy_type", row["category"].split(":", 1)[1])
    visit({**args, "op": op})
    return grants


def pending_allow_grant_labels(
    announcements: list[dict[str, Any]],
    *,
    allow_key_ss58: Optional[str] = None,
) -> list[str]:
    """Labels for pending zero-delay grants, including ordinary proxy commands.

    ``disallow:`` rows do not clear these — the original AddProxy commit remains
    executable until cancelled or consumed. Optionally filter to grants whose
    inner ``delegate_ss58`` is ``allow_key_ss58``.
    """
    labels: list[str] = []
    for row in announcements:
        if row.get("status") not in (None, "pending", "awaiting_approval"):
            continue
        for grant in announcement_proxy_grants(row):
            target = grant["delegate_ss58"]
            if grant["delay"] != 0 or (
                allow_key_ss58
                and target
                and _account_identity(target) != _account_identity(allow_key_ss58)
            ):
                continue
            index = row.get("index")
            who = target or "unknown-delegate"
            label = f"#{index} allow:{grant['proxy_type']} → {who}"
            if label not in labels:
                labels.append(label)
    return labels


def allow_key_change_blockers(
    *,
    old_allow_key_ss58: str,
    new_allow_key_ss58: str,
    allows: list[str],
    pending_allows: list[str],
    delegations: list[dict[str, Any]],
    pending_allow_announcements: Optional[list[str]] = None,
) -> list[str]:
    """Human-readable reasons an ``--allow-key`` retarget must be refused."""
    if _account_identity(old_allow_key_ss58) == _account_identity(new_allow_key_ss58):
        return []
    blockers: list[str] = []
    if allows:
        blockers.append(f"confirmed allows on old key: {allows}")
    if pending_allows:
        blockers.append(f"pending allows on old key: {pending_allows}")
    if pending_allow_announcements:
        blockers.append(f"outstanding allow announcements: {pending_allow_announcements}")
    on_chain = [
        f"{row.get('proxy_type')}@0" for row in allow_key_grants(delegations, old_allow_key_ss58)
    ]
    if on_chain:
        blockers.append(f"on-chain zero-delay grants on old key: {on_chain}")
    return blockers


def rotation_complete(
    delegations: list[dict[str, Any]],
    *,
    new_controller_ss58: str,
    old_controller_ss58: Optional[str],
    delay_blocks: int,
) -> bool:
    """True when ``new`` holds the delayed Any and ``old`` has no grants left."""
    delay = int(delay_blocks or 0)
    new_ok = any(
        _account_identity(d.get("delegate")) == _account_identity(new_controller_ss58)
        and d.get("proxy_type") == "Any"
        and int(d.get("delay") or 0) == delay
        for d in delegations
    )
    if not new_ok:
        return False
    old_cleared = (
        not old_controller_ss58
        or _account_identity(old_controller_ss58) == _account_identity(new_controller_ss58)
        or not any(
            _account_identity(d.get("delegate")) == _account_identity(old_controller_ss58)
            for d in delegations
        )
    )
    return old_cleared


def default_protected_name(wallet_name: str) -> str:
    return f"{wallet_name}-protected"


def blocks_to_local_eta(blocks: int, *, blocktime: float = BLOCKTIME) -> str:
    """Human ETA string for ``blocks`` from now (local timezone)."""
    from datetime import datetime, timedelta

    from .timelock import format_duration

    if blocks <= 0:
        return "now"
    wait = timedelta(seconds=blocks * blocktime)
    eta = datetime.now().astimezone() + wait
    if wait >= timedelta(minutes=1):
        rounded = timedelta(minutes=round(wait.total_seconds() / 60))
    else:
        rounded = wait
    return f"~{format_duration(rounded)} ({eta:%Y-%m-%d %H:%M %Z})"
