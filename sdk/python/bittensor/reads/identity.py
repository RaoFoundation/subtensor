"""Identity and commitment reads."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any, Optional

from .. import metagraph as metagraph_module
from .._generated import storage as st
from ..balance import Balance
from .base import read, utf8_text


@dataclass
class Commitment:
    """On-chain commitment published by a hotkey on a subnet."""

    block: int
    deposit: Balance
    data: str  # committed bytes, utf-8 if possible, else 0x-hex
    fields: list  # raw decoded field variants, for non-Raw commitments


@read(
    "identity",
    {"coldkey_ss58": "string"},
    category="Identity & commitments",
    param_docs={"coldkey_ss58": "Coldkey whose identity to read."},
)
async def identity(view, coldkey_ss58: str) -> Optional[dict]:
    """The on-chain identity of a coldkey (name, links, description), or None."""
    value = await view.query(st.SubtensorModule.IdentitiesV2, [coldkey_ss58])
    return {k: utf8_text(v) for k, v in value.items()} if value else None


@read(
    "hotkey_identities",
    {"hotkey_ss58s": "array"},
    category="Identity & commitments",
    param_docs={"hotkey_ss58s": "Hotkeys to resolve identities for."},
)
async def hotkey_identities(view, hotkey_ss58s: list[str]) -> dict[str, dict]:
    """On-chain identities for hotkeys (via each hotkey's owner coldkey), keyed by hotkey."""
    hotkeys = list(dict.fromkeys(hotkey_ss58s))
    if not hotkeys:
        return {}
    owners = await asyncio.gather(
        *(view.query(st.SubtensorModule.Owner, [hotkey]) for hotkey in hotkeys)
    )
    owner_of = {hotkey: str(owner) for hotkey, owner in zip(hotkeys, owners) if owner}
    coldkeys = list(dict.fromkeys(owner_of.values()))
    values = await asyncio.gather(
        *(view.query(st.SubtensorModule.IdentitiesV2, [coldkey]) for coldkey in coldkeys)
    )
    identity_of = {
        coldkey: {k: utf8_text(v) for k, v in value.items()}
        for coldkey, value in zip(coldkeys, values)
        if value
    }
    return {
        hotkey: identity_of[coldkey]
        for hotkey, coldkey in owner_of.items()
        if coldkey in identity_of
    }


def _decode_commitment_data(fields: list) -> str:
    """Concatenate Raw* field bytes; utf-8 when possible, else 0x-hex."""
    data = b""
    for entry in fields:
        for variant, value in (entry or {}).items():
            if variant.startswith("Raw") and isinstance(value, str):
                data += bytes.fromhex(value.removeprefix("0x"))
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return "0x" + data.hex()


@read(
    "commitment",
    {"netuid": "integer", "hotkey_ss58": "string"},
    category="Identity & commitments",
    param_docs={
        "netuid": "Subnet the commitment was published on.",
        "hotkey_ss58": "Hotkey that published the commitment.",
    },
)
async def commitment(view, netuid: int, hotkey_ss58: str) -> Optional[Commitment]:
    """The commitment a hotkey has published on a subnet, or None."""
    record = await view.query(st.Commitments.CommitmentOf, [netuid, hotkey_ss58])
    if not record:
        return None
    fields = list((record.get("info") or {}).get("fields") or [])
    return Commitment(
        block=int(record.get("block") or 0),
        deposit=Balance.from_rao(int(record.get("deposit") or 0)),
        data=_decode_commitment_data(fields),
        fields=fields,
    )


@read(
    "revealed_commitment",
    {"netuid": "integer", "hotkey_ss58": "string"},
    category="Identity & commitments",
    param_docs={
        "netuid": "Subnet the commitment was published on.",
        "hotkey_ss58": "Hotkey that published the commitment.",
    },
)
async def revealed_commitment(view, netuid: int, hotkey_ss58: str) -> Any:
    """The revealed (timelock-decrypted) commitment for a hotkey on a subnet, or None."""
    return await view.query(st.Commitments.RevealedCommitments, [netuid, hotkey_ss58])


def _age_text(seconds: float) -> str:
    """Compact wall-clock age ('3d 4h', '12m'); '<1m' when tiny."""
    seconds = int(seconds)
    if seconds < 60:
        return "<1m"
    parts = []
    for suffix, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if seconds >= size:
            parts.append(f"{seconds // size}{suffix}")
            seconds %= size
        if len(parts) == 2:
            break
    return " ".join(parts)


@read(
    "commitments",
    {"netuid": "integer"},
    category="Identity & commitments",
    param_docs={"netuid": "Subnet whose commitments to list."},
)
async def commitments(view, netuid: int) -> list[dict]:
    """Every commitment on a subnet, newest first: hotkey, uid, content, block, age, reveal state.

    One row per hotkey that has (or had) a commitment — including sealed
    timelocked payloads waiting on drand (`is_revealed` false, `reveals_at`
    set) and fully-revealed ones whose live storage entry the chain already
    dropped. `commitment` is the currently visible content: the plaintext, or
    the latest chain-decrypted payload; null while still sealed.
    """
    records = await metagraph_module.fetch_commitments(view, netuid)
    return [
        {
            "hotkey": c.hotkey,
            "uid": c.uid,
            "commitment": c.value,
            "block": c.block,
            "duration": _age_text(c.duration.total_seconds()),
            "is_revealed": c.is_revealed,
            "status": c.status,
            "reveals_at": c.reveals_at.isoformat(timespec="seconds") if c.reveals_at else None,
        }
        for c in records.values()
    ]
