"""Local plaintext store for delayed proxy announcements.

On-chain ``Proxy.announce`` stores only the call hash (a commit). This store
keeps the plaintext intent so ``btcli proxy show --index N`` and
``btcli proxy execute --index N`` can reveal and dispatch it after the delay.
Not encrypted beyond the on-chain hash commit (by design).

Named ``proxy_announcements`` (not ``vault``) to avoid colliding with
``bittensor.vault`` / ``--signer vault`` (Polkadot Vault).
"""

from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Any, Optional

from .receiving import is_receiving_address, parse_recipient

DEFAULT_ANNOUNCEMENTS_PATH = Path.home() / ".bittensor" / "btcli_proxy_announcements.json"


def normalize_call_semantics(value: Any) -> Any:
    """Convert decoded SCALE values into stable JSON-comparable primitives."""
    if hasattr(value, "value"):
        value = value.value
    if isinstance(value, bytes):
        return "0x" + value.hex()
    if isinstance(value, dict):
        return {str(key): normalize_call_semantics(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [normalize_call_semantics(item) for item in value]
    return value


def announcements_path() -> Path:
    return Path(os.getenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH") or DEFAULT_ANNOUNCEMENTS_PATH)


def load_announcements() -> list[dict[str, Any]]:
    path = announcements_path()
    if not path.is_file():
        return []
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return []
    if not isinstance(data, dict):
        return []
    rows = data.get("announcements")
    return rows if isinstance(rows, list) else []


def save_announcements(rows: list[dict[str, Any]]) -> Path:
    path = announcements_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {"version": 1, "announcements": rows}
    tmp = path.with_suffix(path.suffix + ".tmp")
    try:
        tmp.write_text(json.dumps(payload, indent=2) + "\n")
        os.replace(tmp, path)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
    return path


def next_index(rows: Optional[list[dict[str, Any]]] = None) -> int:
    rows = load_announcements() if rows is None else rows
    if not rows:
        return 1
    return max(int(r.get("index") or 0) for r in rows) + 1


def _scope_matches(row: dict[str, Any], genesis_hash: Optional[str]) -> bool:
    stored = row.get("genesis_hash")
    return (str(stored).lower() if stored is not None else None) == (
        str(genesis_hash).lower() if genesis_hash is not None else None
    )


def _same_commitment(left: dict[str, Any], right: dict[str, Any]) -> bool:
    """Same call commitment and authority, regardless of announcement instance."""
    return (
        _scope_matches(left, right.get("genesis_hash"))
        and left.get("controller_wallet") == right.get("controller_wallet")
        and _account_identity(left.get("delegate_ss58"))
        == _account_identity(right.get("delegate_ss58"))
        and _account_identity(left.get("real_ss58")) == _account_identity(right.get("real_ss58"))
        and str(left.get("call_hash") or "").lower() == str(right.get("call_hash") or "").lower()
    )


def _same_identity(left: dict[str, Any], right: dict[str, Any]) -> bool:
    """One concrete on-chain announcement, including same-block duplicates."""
    return (
        _same_commitment(left, right)
        and int(left.get("announced_at_block") or 0) == int(right.get("announced_at_block") or 0)
        and int(left.get("announcement_ordinal") or 0)
        == int(right.get("announcement_ordinal") or 0)
    )


def _account_identity(address: Optional[str]) -> Optional[str]:
    return (
        parse_recipient(address).account
        if isinstance(address, str) and is_receiving_address(address)
        else address
    )


def add_announcement(
    *,
    controller_wallet: str,
    delegate_ss58: str,
    real_ss58: str,
    call_hash: str,
    inner_op: str,
    inner_args: dict[str, Any],
    category: str,
    delay_blocks: int,
    announced_at_block: int,
    announcement_ordinal: int = 0,
    force_proxy_type: Optional[str] = None,
    summary: str = "",
    call_data: Optional[str] = None,
    runtime_spec_version: Optional[int] = None,
    call_semantics: Optional[dict[str, Any]] = None,
    genesis_hash: Optional[str] = None,
) -> dict[str, Any]:
    """Append a pending announcement and return the stored row.

    ``call_data`` is the exact SCALE call bytes (0x-hex) whose Blake2-256 is
    ``call_hash``. Execute must replay those bytes — rebuilding from
    ``inner_op``/``inner_args`` can diverge after prices or balances move.
    """
    if genesis_hash is not None and not genesis_hash:
        raise ValueError("announcement genesis_hash must not be empty")
    rows = load_announcements()
    row = {
        "index": next_index(rows),
        "controller_wallet": controller_wallet,
        "delegate_ss58": delegate_ss58,
        "real_ss58": real_ss58,
        "call_hash": call_hash,
        "inner_op": inner_op,
        "inner_args": inner_args,
        "category": category,
        "delay_blocks": int(delay_blocks),
        "announced_at_block": int(announced_at_block),
        "announcement_ordinal": int(announcement_ordinal),
        "executable_at_block": int(announced_at_block) + int(delay_blocks),
        "force_proxy_type": force_proxy_type,
        "summary": summary or category,
        "status": "pending",
        "created_at": int(time.time()),
    }
    if genesis_hash is not None:
        row["genesis_hash"] = str(genesis_hash).lower()
    if call_data:
        row["call_data"] = call_data
    if runtime_spec_version is not None:
        row["runtime_spec_version"] = int(runtime_spec_version)
    if call_semantics is not None:
        row["call_semantics"] = normalize_call_semantics(call_semantics)
    # Re-announcing the same commitment, or finishing multisig approvals, must
    # retain the index previously shown by discovery or an earlier submission.
    for i, existing in enumerate(rows):
        if _same_identity(existing, row):
            row = {**existing, **row, "index": existing["index"]}
            row.pop("plaintext_missing", None)
            row.pop("on_chain", None)
            rows[i] = row
            for j, duplicate in enumerate(rows):
                if j != i and _same_identity(duplicate, row):
                    rows[j] = {**duplicate, "status": "superseded"}
            break
    else:
        rows.append(row)
    save_announcements(rows)
    return row


def upsert_on_chain_announcement(
    *,
    controller_wallet: str,
    genesis_hash: str,
    delegate_ss58: str,
    real_ss58: str,
    call_hash: str,
    delay_blocks: int,
    announced_at_block: int,
    announcement_ordinal: int = 0,
) -> dict[str, Any]:
    """Persist a discovered commitment with a stable index and recover saved bytes.

    Historical rows remain useful if a previously executed/cancelled call is
    announced again, or an older client incorrectly marked a live call gone.
    Reconcile all statuses so those saved payloads never become unknown hashes.
    """
    if not genesis_hash:
        raise ValueError("on-chain announcements require a genesis_hash")
    rows = load_announcements()
    chain = {
        "controller_wallet": controller_wallet,
        "genesis_hash": str(genesis_hash).lower(),
        "delegate_ss58": delegate_ss58,
        "real_ss58": real_ss58,
        "call_hash": call_hash,
        "delay_blocks": int(delay_blocks),
        "announced_at_block": int(announced_at_block),
        "announcement_ordinal": int(announcement_ordinal),
        "executable_at_block": int(announced_at_block) + int(delay_blocks),
        "status": "pending",
        "on_chain": True,
    }
    for i, existing in enumerate(rows):
        if _same_identity(existing, chain):
            # Older clients could append a row for each multisig approval. Keep
            # the first displayed index while recovering any later plaintext.
            for duplicate in rows:
                if (
                    not existing.get("inner_op")
                    and duplicate.get("inner_op")
                    and _same_identity(duplicate, chain)
                ):
                    existing = {**existing, **duplicate, "index": existing["index"]}
            row = {**existing, **chain}
            row["plaintext_missing"] = not bool(row.get("inner_op"))
            rows[i] = row
            for j, duplicate in enumerate(rows):
                if j != i and _same_identity(duplicate, chain):
                    rows[j] = {**duplicate, "status": "superseded"}
            break
    else:
        template = next((existing for existing in rows if _same_commitment(existing, chain)), None)
        row = {
            **(template or {}),
            **chain,
            "index": (
                template["index"]
                if template and template.get("status") != "pending"
                else next_index(rows)
            ),
            "inner_op": (template or {}).get("inner_op", ""),
            "inner_args": (template or {}).get("inner_args", {}),
            "category": (template or {}).get("category", "unknown"),
            "force_proxy_type": (template or {}).get("force_proxy_type", "Any"),
            "summary": (template or {}).get("summary", f"on-chain announcement {call_hash}"),
            "plaintext_missing": not bool((template or {}).get("inner_op")),
            "created_at": int(time.time()),
        }
        if template and template.get("status") != "pending":
            rows[rows.index(template)] = row
        else:
            rows.append(row)
    save_announcements(rows)
    return row


ACTIVE_ANNOUNCEMENT_STATUSES = frozenset({"pending", "awaiting_approval"})


def list_for_wallet(
    wallet_name: str, *, include_done: bool = False, genesis_hash: Optional[str] = None
) -> list[dict[str, Any]]:
    rows = [
        r
        for r in load_announcements()
        if r.get("controller_wallet") == wallet_name
        and _scope_matches(r, genesis_hash)
        and (include_done or r.get("status") in ACTIVE_ANNOUNCEMENT_STATUSES)
    ]
    return sorted(rows, key=lambda r: int(r.get("index") or 0))


def get_by_index(
    wallet_name: str, index: int, *, genesis_hash: Optional[str] = None
) -> Optional[dict[str, Any]]:
    for row in load_announcements():
        if (
            row.get("controller_wallet") == wallet_name
            and int(row.get("index") or 0) == index
            and _scope_matches(row, genesis_hash)
        ):
            return row
    return None


def update_announcement(
    wallet_name: str,
    index: int,
    updates: dict[str, Any],
    *,
    genesis_hash: Optional[str] = None,
) -> Optional[dict[str, Any]]:
    rows = load_announcements()
    updated = None
    for i, row in enumerate(rows):
        if (
            row.get("controller_wallet") == wallet_name
            and int(row.get("index") or 0) == index
            and _scope_matches(row, genesis_hash)
        ):
            row = {**row, **updates}
            rows[i] = row
            updated = row
            break
    if updated is None:
        return None
    save_announcements(rows)
    return updated


def mark_status(
    wallet_name: str, index: int, status: str, *, genesis_hash: Optional[str] = None
) -> Optional[dict[str, Any]]:
    return update_announcement(
        wallet_name,
        index,
        {"status": status, "updated_at": int(time.time())},
        genesis_hash=genesis_hash,
    )


def remove_announcement(
    wallet_name: str, index: int, *, genesis_hash: Optional[str] = None
) -> bool:
    rows = load_announcements()
    filtered = [
        r
        for r in rows
        if not (
            r.get("controller_wallet") == wallet_name
            and int(r.get("index") or 0) == index
            and _scope_matches(r, genesis_hash)
        )
    ]
    existed = len(filtered) != len(rows)
    if existed:
        save_announcements(filtered)
    return existed


def move_wallet_announcements(
    from_wallet: str, to_wallet: str, *, genesis_hash: Optional[str] = None
) -> int:
    """Rebind pending rows after controller rotate. Returns count moved."""
    rows = load_announcements()
    moved = 0
    for i, row in enumerate(rows):
        if (
            row.get("controller_wallet") == from_wallet
            and _scope_matches(row, genesis_hash)
            and row.get("status") in ACTIVE_ANNOUNCEMENT_STATUSES
        ):
            rows[i] = {**row, "controller_wallet": to_wallet}
            moved += 1
    if moved:
        save_announcements(rows)
    return moved


def bind_legacy_announcements(wallet_name: str, genesis_hash: str, *, real_ss58: str) -> int:
    """Bind explicitly validated legacy rows without changing their payloads/statuses.

    The caller must validate the pure/account binding on the selected chain and
    obtain the user's explicit migration choice before invoking this helper.
    Conflicting scoped commitments are refused without modifying either copy.
    """
    if not genesis_hash:
        raise ValueError("binding legacy announcements requires a genesis_hash")
    rows = load_announcements()
    scoped = [row for row in rows if _scope_matches(row, genesis_hash)]
    bound = 0
    for i, row in enumerate(rows):
        if (
            row.get("controller_wallet") != wallet_name
            or _account_identity(row.get("real_ss58")) != _account_identity(real_ss58)
            or not _scope_matches(row, None)
        ):
            continue
        updated = {**row, "genesis_hash": str(genesis_hash).lower()}
        if any(_same_identity(other, updated) for other in scoped):
            raise ValueError(
                "cannot bind legacy announcement: a matching commitment already exists "
                "on this network; both stored copies have been preserved"
            )
        rows[i] = updated
        bound += 1
    if bound:
        save_announcements(rows)
    return bound


def category_for_op(op: str) -> str:
    """Short display category for pending lists."""
    if op in {"transfer", "transfer_all"}:
        return "transfer"
    if op.startswith("remove_stake") or op in {"unstake_all", "unstake_all_alpha"}:
        return "unstake"
    if op.startswith("add_stake") or op in {"move_stake", "swap_stake", "move_swap_stake"}:
        return "stake"
    if "basket" in op:
        return "basket"
    if op in {"add_proxy", "remove_proxy", "remove_proxies", "kill_pure_proxy"}:
        return "proxy"
    if "coldkey" in op:
        return "coldkey-swap"
    if op == "batch":
        return "batch"
    return op.replace("_", "-")
