"""Protected-account local config and allow-list mapping.

A protected account is a keyless pure proxy whose only full controller is the
user's wallet (single coldkey or multisig), granted with a non-zero announcement
delay. Day-to-day value moves are announced and wait out that delay; optional
``--allow`` items become zero-delay restricted proxies for instant ops.
"""

from __future__ import annotations

import json
import os
from hashlib import blake2b
from pathlib import Path
from typing import Any, Optional

from ._policy import evaluate_policy
from ._transport.codec import ss58_decode, ss58_encode
from .receiving import is_receiving_address, parse_recipient
from .settings import BLOCKTIME, SS58_FORMAT

DEFAULT_PROTECTED_PATH = Path.home() / ".bittensor" / "btcli_protected.json"

# User-facing --allow tokens -> existing ProxyType names. Only types that cannot
# transfer free balance, manage proxies, or run a coldkey swap are accepted.
ALLOW_ITEM_TO_PROXY_TYPE: dict[str, str] = evaluate_policy("protected.constants", {})[
    "ALLOW_ITEM_TO_PROXY_TYPE"
]

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
    name: frozenset(ops)
    for name, ops in evaluate_policy("protected.constants", {})["PROXY_TYPE_INTENT_OPS"].items()
}

# Zero-delay types that defeat the announcement delay if also held by the controller.
DEFEATING_ZERO_DELAY_TYPES = frozenset(
    evaluate_policy("protected.constants", {})["DEFEATING_ZERO_DELAY_TYPES"]
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
    return evaluate_policy(
        "protected.scope_matches", {"stored": entry.get("genesis_hash"), "selected": genesis_hash}
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
    return _config_change("upsert", record=entry)


def remove_protected(wallet_name: str, *, genesis_hash: Optional[str] = None) -> bool:
    return _config_change("remove", wallet=wallet_name, genesis_hash=genesis_hash)


def move_protected_controller(
    from_wallet: str,
    to_wallet: str,
    *,
    controller_ss58: Optional[str] = None,
    genesis_hash: Optional[str] = None,
) -> Optional[dict[str, Any]]:
    return _config_change(
        "move",
        wallet=from_wallet,
        to_wallet=to_wallet,
        wallet_display=repr(to_wallet),
        controller_ss58=controller_ss58,
        genesis_hash=genesis_hash,
    )


def bind_legacy_controller(wallet_name: str, genesis_hash: str) -> Optional[dict[str, Any]]:
    if not genesis_hash:
        raise ValueError("binding a legacy protected account requires a genesis_hash")
    return _config_change("bind_legacy", wallet=wallet_name, genesis_hash=genesis_hash)


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
    payload = evaluate_policy(
        "protected.pure_preimage",
        {
            "spawner": spawner_public_key,
            "height": int(height),
            "ext_index": int(ext_index),
            "proxy_type_scale": int(proxy_type_scale) & 0xFF,
            "index": int(index),
        },
    )
    return blake2b(bytes.fromhex(payload[2:]), digest_size=32).digest()


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
    try:
        return int(
            evaluate_policy(
                "protected.delay",
                {
                    "raw": raw,
                    "block_time": blocktime,
                    "unbounded": True,
                    "integer_input": isinstance(raw, int),
                },
            )
        )
    except TypeError as error:
        raise ValueError(
            f"invalid delay {raw!r}; use blocks (e.g. 10800) or a duration (e.g. 36h, 2d, 90m)"
        ) from error
    except ValueError as error:
        if not isinstance(raw, int) and "greater than 0" in str(error):
            raise ValueError(
                "delay must resolve to more than 0 blocks (a protected account cannot use delay 0)"
            ) from error
        raise


def resolve_allow_item(item: str) -> str:
    result = evaluate_policy("protected.allow_resolution", {"item": item})
    if result["kind"] == "forbidden":
        raise ValueError(
            f"--allow {item!r} refused: {FORBIDDEN_ALLOW_ITEMS[result['key']]}. "
            "Transfers and proxy/swap changes must go through the delayed controller."
        )
    if result["kind"] == "unknown":
        known = ", ".join(sorted(set(ALLOW_ITEM_TO_PROXY_TYPE)))
        raise ValueError(
            f"--allow {item!r} has no suitably narrow existing ProxyType. "
            f"Known allow items: {known}. "
            "Finer proxy types are a separate later job — refusing rather than "
            "granting something broader."
        )
    return result["proxy_type"]


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
    return evaluate_policy(
        "protected.instant_proxy",
        {
            "op": intent_op,
            "allows": record.get("allows"),
            "allow_key": _account_identity(record.get("allow_key_ss58")),
            "dispatcher": _account_identity(dispatcher_ss58),
            "intent": {
                "claim": bool(getattr(intent, "claim", False)),
                "name": getattr(intent, "name", None),
            },
        },
    )


def defeating_zero_delay_proxies(
    delegations: list[dict[str, Any]], controller_ss58: str
) -> list[str]:
    return [
        str(delegations[i].get("proxy_type") or "")
        for i in _grant_indices(delegations, controller_ss58, "defeating")
    ]


def controller_grants_before_delay(
    delegations: list[dict[str, Any]], controller_ss58: str, *, delay_blocks: int
) -> list[dict[str, Any]]:
    return [
        delegations[i]
        for i in _grant_indices(delegations, controller_ss58, "before_delay", delay_blocks)
    ]


def controller_any_delays(delegations: list[dict[str, Any]], controller_ss58: str) -> list[int]:
    return [
        int(delegations[i].get("delay") or 0)
        for i in _grant_indices(delegations, controller_ss58, "any")
    ]


def controller_holds_allows(record: dict[str, Any], controller_ss58: str) -> bool:
    return evaluate_policy(
        "protected.holds_allows",
        {
            "allow_key": _account_identity(record.get("allow_key_ss58")),
            "controller": _account_identity(controller_ss58),
        },
    )


def controller_owned_allow_types(record: dict[str, Any], controller_ss58: str) -> list[str]:
    return evaluate_policy(
        "protected.owned_allows",
        {
            "allow_key": _account_identity(record.get("allow_key_ss58")),
            "controller": _account_identity(controller_ss58),
            "allows": [str(name) for name in record.get("allows") or [] if str(name)],
        },
    )


def allow_key_grants(
    delegations: list[dict[str, Any]], allow_key_ss58: str
) -> list[dict[str, Any]]:
    return [delegations[i] for i in _grant_indices(delegations, allow_key_ss58, "allow")]


def protected_setup_pending(record: dict[str, Any]) -> bool:
    return evaluate_policy("protected.setup_pending", record)


def announcement_proxy_grants(row: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {"delegate_ss58": None, **grant}
        for grant in evaluate_policy("protected.announcement_grants", row)
    ]


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
    grants = [announcement_proxy_grants(row) for row in announcements]
    selected = evaluate_policy(
        "protected.pending_allow_grants",
        {
            "rows": [
                {
                    "status": row.get("status"),
                    "grants": [
                        {**grant, "identity": _account_identity(grant.get("delegate_ss58")) or None}
                        for grant in grants[index]
                    ],
                }
                for index, row in enumerate(announcements)
            ],
            "allow_key": _account_identity(allow_key_ss58) or None,
        },
    )
    labels = []
    for item in selected:
        row = announcements[item["row_index"]]
        grant = grants[item["row_index"]][item["grant_index"]]
        who = grant.get("delegate_ss58") or "unknown-delegate"
        label = f"#{row.get('index')} allow:{grant['proxy_type']} → {who}"
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
    selected = evaluate_policy(
        "protected.allow_key_blockers",
        {
            "old": _account_identity(old_allow_key_ss58),
            "new": _account_identity(new_allow_key_ss58),
            "allows": allows,
            "pending_allows": pending_allows,
            "pending_announcements": pending_allow_announcements,
            "grants": [
                {**row, "delegate": _account_identity(row.get("delegate"))} for row in delegations
            ],
        },
    )
    on_chain = [f"{delegations[index].get('proxy_type')}@0" for index in selected["live_indices"]]
    labels = {
        "allows": f"confirmed allows on old key: {allows}",
        "pending_allows": f"pending allows on old key: {pending_allows}",
        "pending_announcements": f"outstanding allow announcements: {pending_allow_announcements}",
        "live": f"on-chain zero-delay grants on old key: {on_chain}",
    }
    return [labels[kind] for kind in selected["kinds"]]


def rotation_complete(
    delegations: list[dict[str, Any]],
    *,
    new_controller_ss58: str,
    old_controller_ss58: Optional[str],
    delay_blocks: int,
) -> bool:
    return evaluate_policy(
        "protected.rotation_complete",
        {
            "grants": [
                {
                    **row,
                    "delegate": _account_identity(row.get("delegate")),
                    "delay": int(row.get("delay") or 0),
                }
                for row in delegations
            ],
            "new_controller": _account_identity(new_controller_ss58),
            "old_controller": _account_identity(old_controller_ss58),
            "delay_blocks": int(delay_blocks or 0),
        },
    )


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


def _grant_indices(delegations, controller, mode, delay_blocks=None):
    return evaluate_policy(
        "protected.grant_indices",
        {
            "grants": [
                {
                    **row,
                    "delegate": _account_identity(row.get("delegate")),
                    "delay": int(row.get("delay") or 0),
                }
                for row in delegations
            ],
            "controller": _account_identity(controller),
            "mode": mode,
            "delay_blocks": delay_blocks,
        },
    )


def _config_change(action, **input):
    entries = load_protected()
    addresses = [
        row.get(key)
        for row in [*entries, input.get("record") or {}, input]
        for key in ("controller_ss58", "protected_ss58", "destination")
    ]
    identities = {
        address: _account_identity(address) for address in addresses if isinstance(address, str)
    }
    result = evaluate_policy(
        "protected.config_transition",
        {
            "action": action,
            "rows": entries,
            "identities": identities,
            "python_messages": True,
            **input,
        },
    )
    save_protected(result["value"])
    return result["result"]
