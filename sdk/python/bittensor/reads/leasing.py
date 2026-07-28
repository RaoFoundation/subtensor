"""Subnet lease and crowdloan reads."""

from __future__ import annotations

from typing import Any, Optional

from .._generated import storage as st
from ..balance import Balance
from .base import read


def _lease(lease_id: Any, v: dict) -> dict:
    end = v.get("end_block")
    return {
        "lease_id": int(lease_id),
        "beneficiary": str(v["beneficiary"]),
        "coldkey": str(v["coldkey"]),
        "hotkey": str(v["hotkey"]),
        "emissions_share": int(v["emissions_share"]),  # percent 0..100
        "end_block": int(end) if end is not None else None,
        "netuid": int(v["netuid"]),
        "cost": Balance.from_rao(int(v["cost"])),
    }


@read(
    "lease",
    {"lease_id": "integer"},
    category="Leases & crowdloans",
    param_docs={"lease_id": "Id of the subnet lease to look up."},
)
async def lease(view, lease_id: int) -> Optional[dict]:
    """A subnet lease by id (beneficiary, emissions share, end block, netuid, cost), or None.

    `emissions_share` is a percentage from 0 to 100; `end_block` is None for a
    perpetual lease.
    """
    value = await view.query(st.SubtensorModule.SubnetLeases, [lease_id])
    return _lease(lease_id, value) if value else None


@read("leases", {}, category="Leases & crowdloans")
async def leases(view) -> list[dict]:
    """Every subnet lease on the network."""
    entries = await view.query_map(st.SubtensorModule.SubnetLeases)
    return [_lease(k, v) for k, v in entries]


@read(
    "crowdloan",
    {"crowdloan_id": "integer"},
    category="Leases & crowdloans",
    param_docs={"crowdloan_id": "Id of the crowdloan to look up."},
)
async def crowdloan(view, crowdloan_id: int) -> Optional[dict]:
    """A crowdloan's state (creator, deposit, raised, cap, end, target/call), or None.

    Amounts are TAO; `end` is the block number at which contributions close.
    """
    value = await view.query(st.Crowdloan.Crowdloans, [crowdloan_id])
    if not value:
        return None
    target = value.get("target_address")
    return {
        "creator": str(value.get("creator")),
        "deposit": Balance.from_rao(int(value.get("deposit") or 0)),
        "min_contribution": Balance.from_rao(int(value.get("min_contribution") or 0)),
        "cap": Balance.from_rao(int(value.get("cap") or 0)),
        "raised": Balance.from_rao(int(value.get("raised") or 0)),
        "end": int(value.get("end") or 0),
        "finalized": bool(value.get("finalized")),
        "target_address": str(target) if target else None,
        "funds_account": str(value.get("funds_account")) if value.get("funds_account") else None,
    }


@read("crowdloans", {}, category="Leases & crowdloans")
async def crowdloans(view) -> list[dict]:
    """All crowdloans on chain (id and summary fields)."""
    rows = await view.query_map(st.Crowdloan.Crowdloans)
    out: list[dict] = []
    for crowdloan_id, value in rows:
        if not value:
            continue
        out.append(
            {
                "id": int(crowdloan_id),
                "creator": str(value.get("creator")),
                "raised_tao": Balance.from_rao(int(value.get("raised") or 0)).tao,
                "cap_tao": Balance.from_rao(int(value.get("cap") or 0)).tao,
                "finalized": bool(value.get("finalized")),
            }
        )
    return sorted(out, key=lambda row: row["id"])


@read(
    "crowdloan_contributors",
    {"crowdloan_id": "integer"},
    category="Leases & crowdloans",
    param_docs={"crowdloan_id": "Id of the crowdloan whose contributors to list."},
)
async def crowdloan_contributors(view, crowdloan_id: int) -> list[dict]:
    """Contributors and amounts for a crowdloan, with amounts in TAO."""
    rows = await view.query_map(st.Crowdloan.Contributions, [crowdloan_id])
    return [
        {
            "contributor": str(contributor),
            "amount_tao": Balance.from_rao(int(amount or 0)).tao,
        }
        for contributor, amount in rows
    ]
