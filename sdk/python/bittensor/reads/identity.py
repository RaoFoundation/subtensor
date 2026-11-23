"""Identity and commitment reads."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from decimal import Decimal
from typing import Any, Optional

from .. import metagraph as metagraph_module
from .._generated import storage as st
from .._transport.errors import StorageFunctionNotFound
from ..balance import Balance
from ..settings import GLOBAL_MAX_SUBNET_COUNT
from .base import read, utf8_text


@dataclass
class Commitment:
    """On-chain commitment published by a hotkey on a subnet."""

    block: int
    deposit: Balance
    data: str  # committed bytes, utf-8 if possible, else 0x-hex
    fields: list  # raw decoded field variants, for non-Raw commitments


@dataclass
class ChannelCommitment(Commitment):
    """Active channel fields and separately labelled timelock reveal history."""

    revealed: list[dict] = field(default_factory=list)


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
        for variant, value in metagraph_module._field_items(entry):
            if (variant.startswith("Raw") or variant == "BigRaw") and isinstance(value, str):
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


def _tao_field(value: Any) -> Balance:
    if isinstance(value, dict):
        value = value.get("amount") or value.get("value") or 0
    return Balance.from_rao(int(value or 0))


def _fixed_number(value: Any) -> str:
    if isinstance(value, dict):
        value = value.get("bits") or 0
    return format(Decimal(int(value or 0)) / 2**64, "f")


def _burn_parameters(config: dict, *, live: bool) -> dict:
    enabled = bool(config.get("enabled"))
    return {
        "enabled": enabled,
        "burn": _tao_field(config.get("burn")) if live or enabled else None,
        "stored_burn": _tao_field(config.get("burn")),
        "min_burn": _tao_field(config.get("min_burn")),
        "max_burn": _tao_field(config.get("max_burn")),
        "half_life": int(config.get("half_life") or 0),
        "increase_mult": _fixed_number(config.get("increase_mult")),
        "last_update_block": int(config.get("last_update_block") or 0),
        "price_is_decayed": live,
    }


@read(
    "commit_burn",
    {"netuid": "integer", "mechid": "integer", "channel": "integer"},
    category="Identity & commitments",
    param_docs={
        "netuid": "Subnet whose commitment channel to inspect.",
        "mechid": "Mechanism whose channel to inspect.",
        "channel": "Independent commitment channel index (0..255).",
    },
)
async def commit_burn(view, netuid: int, mechid: int = 0, channel: int = 0) -> Optional[dict]:
    """Live decayed channel burn price and its stored configuration.

    `burn` is the price a commitment would pay now, or None when the burn is
    disabled/unconfigured. The runtime API returns no record when disabled, so
    this read also inspects `CommitBurnConfig` to show `enabled` and the other
    parameters. Direct storage readers should not treat stored `burn` as live:
    decay is lazy; this read's `price_is_decayed` marks the runtime quote.
    """
    if not 0 <= channel <= 255:
        raise ValueError("channel must be in 0..255")
    view = await view.at()
    index = mechid * GLOBAL_MAX_SUBNET_COUNT + netuid
    live, stored = await asyncio.gather(
        view.runtime(
            st.Item("SubnetRegistrationRuntimeApi", "get_commit_burn"),
            [netuid, mechid, channel],
        ),
        view.query(st.Item("Commitments", "CommitBurnConfig"), [(index, channel)]),
    )
    if live is not None:
        return _burn_parameters(live, live=True)
    if stored is None:
        return None
    return _burn_parameters(stored, live=False)


@read(
    "mech_weight_delegate",
    {"netuid": "integer", "mechid": "integer", "hotkey_ss58": "string"},
    category="Weights & bonds",
    param_docs={
        "netuid": "Subnet containing the validator hotkey.",
        "mechid": "Mechanism whose delegate to inspect.",
        "hotkey_ss58": "Validator hotkey whose delegate to inspect.",
    },
)
async def mech_weight_delegate(
    view, netuid: int, mechid: int = 0, hotkey_ss58: Optional[str] = None
) -> Optional[str]:
    """The account authorized to set weights for this hotkey/mechanism, or None."""
    if hotkey_ss58 is None:
        raise ValueError("hotkey_ss58 is required")
    delegate = await view.runtime(
        st.Item("SubnetRegistrationRuntimeApi", "get_mech_weight_delegate"),
        [netuid, mechid, hotkey_ss58],
    )
    return str(delegate) if delegate is not None else None


def _commitment_record(record: dict) -> Commitment:
    fields = list((record.get("info") or {}).get("fields") or [])
    return Commitment(
        block=int(record.get("block") or 0),
        deposit=Balance.from_rao(int(record.get("deposit") or 0)),
        data=_decode_commitment_data(fields),
        fields=fields,
    )


async def _decode_commitment_registration(view, data: Any) -> Any:
    """Use a view's decoder or the client's underlying substrate for old snapshots."""
    if hasattr(view, "decode_commitment_registration"):
        return await view.decode_commitment_registration(data)
    client = getattr(view, "_client", view)
    substrate = getattr(client, "_substrate", client)
    return await substrate.decode_commitment_registration(data)


@read(
    "channel_commitment",
    {"netuid": "integer", "mechid": "integer", "channel": "integer", "hotkey_ss58": "string"},
    category="Identity & commitments",
    param_docs={
        "netuid": "Subnet containing the mechanism and hotkey.",
        "mechid": "Mechanism whose independent channel to read.",
        "channel": "Independent channel index (0..255).",
        "hotkey_ss58": "Hotkey whose channel commitment to read.",
    },
)
async def channel_commitment(
    view, netuid: int, mechid: int = 0, channel: int = 0, hotkey_ss58: Optional[str] = None
) -> Optional[ChannelCommitment]:
    """One hotkey's independent channel commitment, or None when absent.

    Reads the v2 runtime API and decodes the returned SCALE registration. On a
    node without v2, reads the equivalent channel storage instead; both paths
    distinguish this target from the legacy `commitment` read. The `revealed`
    list reports plaintext and reveal blocks even after the active record is removed.
    """
    if not 0 <= channel <= 255:
        raise ValueError("channel must be in 0..255")
    if hotkey_ss58 is None:
        raise ValueError("hotkey_ss58 is required")
    storage_index = mechid * GLOBAL_MAX_SUBNET_COUNT + netuid
    key = [storage_index, hotkey_ss58, channel]
    try:
        encoded = await view.runtime(
            st.Item("SubnetRegistrationRuntimeApi", "get_channel_commitment"),
            [netuid, mechid, channel, hotkey_ss58],
        )
    except (ValueError, StorageFunctionNotFound) as error:
        if "not found" not in str(error).lower():
            raise
        try:
            encoded = await view.query(st.Commitments.ChannelCommitmentOf, key)
        except StorageFunctionNotFound:
            return None
    try:
        reveals = await view.query(st.Commitments.ChannelRevealedCommitments, key)
    except StorageFunctionNotFound:
        reveals = None
    if encoded is None and not reveals:
        return None
    if isinstance(encoded, str):
        # The transport renders byte vectors as UTF-8 when possible, or as 0x-prefixed hex.
        if encoded.startswith("0x"):
            try:
                encoded = bytes.fromhex(encoded[2:])
            except ValueError:
                encoded = encoded.encode("utf-8")
        else:
            encoded = encoded.encode("utf-8")
    record = (
        encoded
        if isinstance(encoded, dict) or encoded is None
        else await _decode_commitment_registration(view, encoded)
    )
    active = _commitment_record(record or {})
    return ChannelCommitment(
        active.block,
        active.deposit,
        active.data,
        active.fields,
        [
            # Transport strings are already rendered as plaintext or hex.
            {"data": data if isinstance(data, str) else utf8_text(data), "block": int(block)}
            for data, block in reveals or []
        ],
    )


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
    timelocked payloads waiting on drand (`status` ``sealed``), terminal
    on-chain reveal failures retained for audit (`status` ``failed``), and
    fully-revealed ones whose live storage entry the chain already dropped.
    `commitment` is the currently visible content: the plaintext, or the
    latest chain-decrypted payload; null while sealed or failed.
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
