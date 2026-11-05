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

DEFAULT_ANNOUNCEMENTS_PATH = Path.home() / ".bittensor" / "btcli_proxy_announcements.json"


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
    force_proxy_type: Optional[str] = None,
    summary: str = "",
    call_data: Optional[str] = None,
) -> dict[str, Any]:
    """Append a pending announcement and return the stored row.

    ``call_data`` is the exact SCALE call bytes (0x-hex) whose Blake2-256 is
    ``call_hash``. Execute must replay those bytes — rebuilding from
    ``inner_op``/``inner_args`` can diverge after prices or balances move.
    """
    rows = load_announcements()
    index = next_index(rows)
    row = {
        "index": index,
        "controller_wallet": controller_wallet,
        "delegate_ss58": delegate_ss58,
        "real_ss58": real_ss58,
        "call_hash": call_hash,
        "inner_op": inner_op,
        "inner_args": inner_args,
        "category": category,
        "delay_blocks": int(delay_blocks),
        "announced_at_block": int(announced_at_block),
        "executable_at_block": int(announced_at_block) + int(delay_blocks),
        "force_proxy_type": force_proxy_type,
        "summary": summary or category,
        "status": "pending",
        "created_at": int(time.time()),
    }
    if call_data:
        row["call_data"] = call_data
    rows.append(row)
    save_announcements(rows)
    return row


ACTIVE_ANNOUNCEMENT_STATUSES = frozenset({"pending", "awaiting_approval"})


def list_for_wallet(wallet_name: str, *, include_done: bool = False) -> list[dict[str, Any]]:
    rows = [
        r
        for r in load_announcements()
        if r.get("controller_wallet") == wallet_name
        and (include_done or r.get("status") in ACTIVE_ANNOUNCEMENT_STATUSES)
    ]
    return sorted(rows, key=lambda r: int(r.get("index") or 0))


def get_by_index(wallet_name: str, index: int) -> Optional[dict[str, Any]]:
    for row in load_announcements():
        if row.get("controller_wallet") == wallet_name and int(row.get("index") or 0) == index:
            return row
    return None


def update_announcement(
    wallet_name: str, index: int, updates: dict[str, Any]
) -> Optional[dict[str, Any]]:
    rows = load_announcements()
    updated = None
    for i, row in enumerate(rows):
        if row.get("controller_wallet") == wallet_name and int(row.get("index") or 0) == index:
            row = {**row, **updates}
            rows[i] = row
            updated = row
            break
    if updated is None:
        return None
    save_announcements(rows)
    return updated


def mark_status(wallet_name: str, index: int, status: str) -> Optional[dict[str, Any]]:
    return update_announcement(
        wallet_name, index, {"status": status, "updated_at": int(time.time())}
    )


def remove_announcement(wallet_name: str, index: int) -> bool:
    rows = load_announcements()
    filtered = [
        r
        for r in rows
        if not (r.get("controller_wallet") == wallet_name and int(r.get("index") or 0) == index)
    ]
    existed = len(filtered) != len(rows)
    if existed:
        save_announcements(filtered)
    return existed


def move_wallet_announcements(from_wallet: str, to_wallet: str) -> int:
    """Rebind pending rows after controller rotate. Returns count moved."""
    rows = load_announcements()
    moved = 0
    for i, row in enumerate(rows):
        if (
            row.get("controller_wallet") == from_wallet
            and row.get("status") in ACTIVE_ANNOUNCEMENT_STATUSES
        ):
            rows[i] = {**row, "controller_wallet": to_wallet}
            moved += 1
    if moved:
        save_announcements(rows)
    return moved


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
