"""Shared helpers for CLI commands that aggregate wallet or chain data.

Units are kept explicit end to end: ``free`` is exact on-chain TAO; stake is a
list of per-subnet alpha positions (each subnet's own currency); the only TAO
scalar derived from stake is ``stake_value`` — a spot-price mark that excludes
the slippage and fees a real unstake would incur. Alpha amounts are never
summed across subnets.
"""

from __future__ import annotations

import asyncio
from typing import Optional

from .. import config as cfg
from .. import wallets
from .._generated import runtime_apis as api
from ..balance import Balance
from ..client import Client
from ..reads import StakePosition, StakeValuation

STAKE_VALUE_BASIS = "spot price; excludes slippage/fees of an actual unstake"

# Positions whose spot TAO value is below this are hidden from human tables
# (JSON always carries every position). τ0.001 = 1_000_000 rao.
DUST_VALUE_TAO = 0.001


def split_dust(records: list[dict]) -> tuple[list[dict], list[dict]]:
    """Split stake/delegation records into (kept, dust) by spot TAO value.

    Whole records below the threshold are dust; within kept records that carry
    a per-hotkey ``positions`` breakdown, dust positions are pruned from the
    human view and returned as dust too.
    """
    kept: list[dict] = []
    dust: list[dict] = []
    for record in records:
        if record["value_tao"] < DUST_VALUE_TAO:
            dust.append(record)
            continue
        positions = record.get("positions")
        if positions:
            dusty = [p for p in positions if p["value_tao"] < DUST_VALUE_TAO]
            if dusty:
                dust.extend(dusty)
                record = {
                    **record,
                    "positions": [p for p in positions if p["value_tao"] >= DUST_VALUE_TAO],
                }
        kept.append(record)
    return kept, dust


def dust_note(dust: list[dict]) -> str:
    total = sum(r["value_tao"] for r in dust)
    subnets = sum(1 for r in dust if "positions" in r)
    positions = len(dust) - subnets
    parts = []
    if subnets:
        parts.append(f"{subnets} dust subnet{'s' if subnets != 1 else ''}")
    if positions:
        parts.append(f"{positions} dust position{'s' if positions != 1 else ''}")
    return (
        f"[dim]{' and '.join(parts)} hidden "
        f"(each < τ{DUST_VALUE_TAO}, ≈ τ{total:.6f} total) — pass --dust to show[/dim]"
    )


def list_coldkeys(path: str) -> list[tuple[str, str]]:
    """Return ``(wallet_name, coldkey_ss58)`` for every coldkey under ``path``."""
    out: list[tuple[str, str]] = []
    for ck in wallets.list_wallets_detailed(path):
        if ck.ss58:
            out.append((ck.name, ck.ss58))
    return out


def hotkey_name_map(path: str) -> dict[str, str]:
    """Map hotkey ss58 -> local hotkey name for every wallet under ``path``."""
    out: dict[str, str] = {}
    for ck in wallets.list_wallets_detailed(path):
        for hk in ck.hotkeys:
            if hk.ss58:
                out.setdefault(hk.ss58, hk.name)
    return out


def local_address_names(path: str) -> dict[str, str]:
    """Map ss58 -> local name: wallet hotkeys plus address-book contacts."""
    out = hotkey_name_map(path)
    for entry in cfg.load_addresses():
        address, name = entry.get("address"), entry.get("name")
        if address and name:
            out.setdefault(address, name)
    return out


async def chain_identity_names(client: Client, hotkey_ss58s: list[str]) -> dict[str, str]:
    """Map hotkey ss58 -> on-chain identity name (via the hotkey's owner coldkey).

    Only hotkeys whose owner published an identity with a name appear in the
    result. Used to label stake accounts we have no local name for.
    """
    identities = await client.read("hotkey_identities", hotkey_ss58s=hotkey_ss58s)
    out: dict[str, str] = {}
    for hotkey, identity in identities.items():
        name = identity.get("name")
        if name:
            out[hotkey] = str(name)
    return out


def netuid_groups(
    positions: list[StakePosition],
    valuation: StakeValuation,
    hotkey_names: dict[str, str],
    identity_names: Optional[dict[str, str]] = None,
    extra: Optional[dict] = None,
    takes: Optional[dict[tuple[int, str], float]] = None,
) -> list[dict]:
    """Collapse positions to per-netuid groups for the human view: the subnet
    total plus a per-hotkey breakdown (largest first), hotkeys labeled with
    their local wallet name when one exists, else their on-chain identity name.

    ``takes`` maps ``(netuid, hotkey)`` to a delegate's take fraction; matching
    positions carry it so the renderer can annotate delegated stake in place
    (zero takes are dropped — they'd annotate almost every leaf with noise).
    """
    identity_names = identity_names or {}
    takes = takes or {}
    by_netuid: dict[int, list[StakePosition]] = {}
    for pos in positions:
        by_netuid.setdefault(pos.netuid, []).append(pos)
    groups: list[dict] = []
    for netuid in sorted(by_netuid):
        group = by_netuid[netuid]
        # The per-position balances were symbol-tagged at decode; the group sum
        # keeps the same subnet's symbol.
        stake = Balance(sum(p.stake.rao for p in group), netuid, group[0].stake.symbol)
        value = valuation.spot_value(stake)
        groups.append(
            {
                **(extra or {}),
                "netuid": netuid,
                "stake": str(stake),
                "value": str(value),
                "value_tao": value.tao,
                "positions": [
                    {
                        "stake": str(p.stake),
                        "value_tao": valuation.spot_value(p.stake).tao,
                        "hotkey": p.hotkey,
                        "label": hotkey_names.get(p.hotkey)
                        or identity_names.get(p.hotkey, p.hotkey),
                        "named": p.hotkey in hotkey_names,
                        "identity": p.hotkey not in hotkey_names and p.hotkey in identity_names,
                        "take": takes.get((p.netuid, p.hotkey)) or None,
                    }
                    for p in sorted(group, key=lambda p: -p.stake.rao)
                ],
            }
        )
    return groups


STAKE_LIST_TITLE = "stake (per-subnet currency: TAO on netuid 0, alpha elsewhere)"


def annotate_stake_groups_with_locks(
    groups: list[dict],
    locks_by_netuid: dict[int, dict],
    availability_by_netuid: dict[int, dict],
    hotkey_names: dict[str, str],
    identity_names: Optional[dict[str, str]] = None,
) -> None:
    """Annotate stake-list groups with free/locked totals and lock-hotkey hints.

    Mutates ``groups`` in place. A subnet note shows ``locked · free`` when any
    mass is locked; positions whose hotkey is not the lock target get a leaf
    note so the split "lock → owner, stake → vali" setup is obvious.
    """
    identity_names = identity_names or {}
    for group in groups:
        netuid = int(group["netuid"])
        avail = availability_by_netuid.get(netuid)
        lock = locks_by_netuid.get(netuid)
        if not avail and not lock:
            continue
        locked = avail["locked"] if avail else None
        available = avail["available"] if avail else None
        notes: list[str] = []
        lock_hotkey = lock["hotkey"] if lock else None
        lock_label = None
        if lock_hotkey and locked is not None and locked.rao > 0:
            lock_label = (
                hotkey_names.get(lock_hotkey) or identity_names.get(lock_hotkey) or lock_hotkey
            )
            # Keep the header note short — long locked/free figures sit on the
            # stake line (see ``availability_note``) so they are not clipped.
            notes.append(f"lock → {lock_label}")
        if locked is not None and locked.rao > 0 and available is not None:
            group["availability_note"] = f"{locked} locked · {available} free"
        if notes:
            existing = group.get("note")
            group["note"] = " · ".join([existing, *notes] if existing else notes)
        if not lock_hotkey or not lock_label:
            continue
        for position in group.get("positions", []):
            if position.get("hotkey") == lock_hotkey:
                continue
            leaf = f"lock on {lock_label}"
            prior = position.get("note")
            position["note"] = f"{prior} · {leaf}" if prior else leaf


def enrich_stake_records_with_locks(
    records: list[dict],
    locks_by_netuid: dict[int, dict],
    availability_by_netuid: dict[int, dict],
) -> None:
    """Add subnet-level lock/availability fields onto flat stake-list JSON rows."""
    for record in records:
        netuid = int(record["netuid"])
        avail = availability_by_netuid.get(netuid)
        lock = locks_by_netuid.get(netuid)
        if avail:
            record["locked"] = str(avail["locked"])
            record["locked_amount"] = avail["locked"].amount
            record["available"] = str(avail["available"])
            record["available_amount"] = avail["available"].amount
        if lock and avail and avail["locked"].rao > 0:
            record["lock_hotkey"] = lock["hotkey"]
            record["is_perpetual"] = bool(lock.get("is_perpetual"))


def human_balance_fields(row: dict) -> dict:
    """Compact human view of a balance row; the full record (``*_tao`` duplicates,
    basis string) is for JSON consumers."""
    fields = {
        "wallet": f"{row['wallet']} ({row['coldkey']})",
        "free": row["free"],
        "stake": f"{row['stake_positions']} positions · {row['stake_subnets']} subnets",
        "stake_value": f"{row['stake_value']}  (spot, excl. slippage/fees)",
    }
    if row.get("locked_subnets"):
        fields["locked_value"] = (
            f"{row['locked_value']}  ({row['locked_subnets']} subnets; part of stake_value)"
        )
    fields["total_value"] = row["total_value"]
    fields["block"] = row["block"]
    return fields


def _wallet_balance_row(
    name: str, coldkey_ss58: str, free: Balance, valuation: StakeValuation
) -> dict[str, object]:
    """One wallet's balance row: exact free TAO plus spot-valued stake."""
    total_value = free + valuation.stake_value
    return {
        "wallet": name,
        "coldkey": coldkey_ss58,
        "free": free,
        "free_tao": free.tao,
        "stake_positions": len(valuation.positions),
        "stake_subnets": len({p.netuid for p in valuation.positions}),
        "stake_value": valuation.stake_value,
        "stake_value_tao": valuation.stake_value.tao,
        "total_value": total_value,
        "total_value_tao": total_value.tao,
        "stake_value_basis": STAKE_VALUE_BASIS,
        "block": valuation.block,
    }


async def fetch_coldkey_balances_and_valuations(
    client: Client, coldkeys: list[tuple[str, str]]
) -> tuple[dict[str, Balance], dict[str, StakeValuation]]:
    """Free balances and block-pinned stake valuations for many coldkeys, batched."""
    ss58s = [ss58 for _, ss58 in coldkeys]
    valuations = await client.read("stake_value_for_coldkeys", coldkey_ss58s=ss58s)
    block = next(iter(valuations.values())).block if valuations else None
    free_by_addr = await client.balances.get_many(ss58s, block=block)
    return free_by_addr, valuations


async def _locked_value(
    client: Client, coldkey_ss58: str, valuation: StakeValuation
) -> tuple[Balance, int]:
    """Spot TAO value of the coldkey's locked stake and the subnets it spans.

    Locks pin existing stake positions, so this is a subset of ``stake_value``,
    not an addition to it. One runtime call covers every staked subnet
    (``get_stake_availability_for_coldkeys`` returns rolled-forward locked mass
    per netuid — the same figure as per-subnet ``coldkey_lock`` reads).
    """
    netuids = sorted({p.netuid for p in valuation.positions})
    if not netuids:
        return Balance(0), 0
    availability = await client.runtime(
        api.StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
        [[coldkey_ss58], netuids],
        block=valuation.block,
    )
    per_netuid = (availability or {}).get(coldkey_ss58) or {}
    locked_rao = 0
    subnets = 0
    for netuid, entry in per_netuid.items():
        locked = Balance.from_rao(int(entry.get("locked") or 0), int(netuid))
        if not locked.rao:
            continue
        locked_rao += valuation.spot_value(locked).rao
        subnets += 1
    return Balance(locked_rao), subnets


async def coldkey_lock_context(
    client: Client, coldkey_ss58: str, positions: list[StakePosition]
) -> tuple[dict[int, dict], dict[int, dict]]:
    """A coldkey's conviction locks and free/locked stake split, keyed by netuid.

    Returns ``(locks_by_netuid, availability_by_netuid)`` — the inputs of
    :func:`annotate_stake_groups_with_locks` and
    :func:`enrich_stake_records_with_locks`. Only subnets present in
    ``positions`` are queried; both reads collapse to one runtime call each.
    """
    netuids = sorted({p.netuid for p in positions})
    locks, availability = await asyncio.gather(
        client.read("locks_for_coldkey", coldkey_ss58=coldkey_ss58),
        client.read(
            "stake_availability_for_coldkey",
            coldkey_ss58=coldkey_ss58,
            netuids=netuids,
        ),
    )
    locks_by_netuid = {int(lock["netuid"]): lock for lock in locks}
    availability_by_netuid = {int(row["netuid"]): row for row in availability}
    return locks_by_netuid, availability_by_netuid


async def wallet_balance_row(client: Client, name: str, coldkey_ss58: str) -> dict[str, object]:
    """Free TAO, spot-valued stake, locked value, and total value for one coldkey."""
    valuation = await client.read("stake_value_for_coldkey", coldkey_ss58=coldkey_ss58)
    free, (locked_value, locked_subnets) = await asyncio.gather(
        client.balances.get(coldkey_ss58, block=valuation.block),
        _locked_value(client, coldkey_ss58, valuation),
    )
    row = _wallet_balance_row(name, coldkey_ss58, free, valuation)
    row["locked_value"] = locked_value
    row["locked_value_tao"] = locked_value.tao
    row["locked_subnets"] = locked_subnets
    return row


async def wallet_balance_rows(
    client: Client, coldkeys: list[tuple[str, str]]
) -> list[dict[str, object]]:
    """Balance rows for many coldkeys in three batched RPC calls at one block."""
    if not coldkeys:
        return []
    free_by_addr, valuations = await fetch_coldkey_balances_and_valuations(client, coldkeys)
    rows: list[dict[str, object]] = []
    for name, ss58 in coldkeys:
        rows.append(_wallet_balance_row(name, ss58, free_by_addr[ss58], valuations[ss58]))
    return rows


def filter_stakes(positions: list[StakePosition], netuid: Optional[int]) -> list[StakePosition]:
    if netuid is None:
        return positions
    return [position for position in positions if position.netuid == netuid]


def _stake_record(position: StakePosition, valuation: StakeValuation) -> dict[str, object]:
    """One stake position with its unit spelled out and its spot TAO value."""
    value = valuation.spot_value(position.stake)
    return {
        "netuid": position.netuid,
        "hotkey": position.hotkey,
        "stake": str(position.stake),
        "stake_amount": position.stake.amount,
        "stake_unit": "TAO" if position.netuid == 0 else f"alpha (netuid {position.netuid})",
        "value": str(value),
        "value_tao": value.tao,
    }


async def wallet_overview_rows(
    client: Client,
    coldkeys: list[tuple[str, str]],
    netuid: Optional[int] = None,
) -> tuple[list[dict[str, object]], dict[str, StakeValuation], dict[str, tuple[dict, dict]]]:
    """Stake overview for many coldkeys in a few batched RPC calls at one block.

    Returns the JSON-shaped rows plus the underlying valuations (positions and
    spot prices) and per-coldkey lock contexts (see :func:`coldkey_lock_context`)
    for human renderings that need more than the flat records. Rows whose
    coldkey holds conviction locks carry the locked spot value and subnet count;
    their stake records carry the locked/free split via
    :func:`enrich_stake_records_with_locks`.
    """
    if not coldkeys:
        return [], {}, {}
    free_by_addr, valuations = await fetch_coldkey_balances_and_valuations(client, coldkeys)
    contexts = await asyncio.gather(
        *[coldkey_lock_context(client, ss58, valuations[ss58].positions) for _, ss58 in coldkeys]
    )
    lock_ctx = {ss58: ctx for (_, ss58), ctx in zip(coldkeys, contexts)}
    rows: list[dict[str, object]] = []
    for name, ss58 in coldkeys:
        balance = _wallet_balance_row(name, ss58, free_by_addr[ss58], valuations[ss58])
        stakes = filter_stakes(valuations[ss58].positions, netuid)
        locks_by_netuid, availability_by_netuid = lock_ctx[ss58]
        locked_rows = [row for row in availability_by_netuid.values() if row["locked"].rao > 0]
        locked_value = Balance(
            sum(valuations[ss58].spot_value(row["locked"]).rao for row in locked_rows)
        )
        records = [_stake_record(position, valuations[ss58]) for position in stakes]
        enrich_stake_records_with_locks(records, locks_by_netuid, availability_by_netuid)
        rows.append(
            {
                "wallet": name,
                "coldkey": ss58,
                "free": balance["free"],
                "free_tao": balance["free_tao"],
                "stake_value": balance["stake_value"],
                "stake_value_tao": balance["stake_value_tao"],
                "stake_value_basis": STAKE_VALUE_BASIS,
                "locked_value": locked_value,
                "locked_value_tao": locked_value.tao,
                "locked_subnets": len(locked_rows),
                "positions": len(stakes),
                "stakes": records,
            }
        )
    return rows, valuations, lock_ctx


def _delegation_record(delegation, valuation: StakeValuation) -> dict[str, object]:
    """One nomination with its unit spelled out and its spot TAO value."""
    netuid = delegation.netuid
    value = valuation.spot_value(delegation.stake)
    return {
        "netuid": netuid,
        "delegate_hotkey": delegation.delegate.hotkey,
        "take": delegation.delegate.take,
        "stake": str(delegation.stake),
        "stake_amount": delegation.stake.amount,
        "stake_unit": "TAO" if netuid == 0 else f"alpha (netuid {netuid})",
        "value": str(value),
        "value_tao": value.tao,
    }


async def wallet_inspect_data(
    client: Client, name: str, coldkey_ss58: str
) -> tuple[dict[str, object], StakeValuation]:
    """Detailed wallet view in one connection with parallel reads.

    Returns the JSON-shaped record plus the underlying valuation (positions and
    spot prices) for human renderings that need more than the flat records.
    """
    valuation, delegated, identity = await asyncio.gather(
        client.read("stake_value_for_coldkey", coldkey_ss58=coldkey_ss58),
        client.read("delegated", coldkey_ss58=coldkey_ss58),
        client.read("identity", coldkey_ss58=coldkey_ss58),
    )
    free = await client.balances.get(coldkey_ss58, block=valuation.block)
    balance = _wallet_balance_row(name, coldkey_ss58, free, valuation)
    data = {
        "balance": balance,
        "stakes": [_stake_record(position, valuation) for position in valuation.positions],
        "delegations": [_delegation_record(d, valuation) for d in delegated],
        "identity": identity,
    }
    return data, valuation
