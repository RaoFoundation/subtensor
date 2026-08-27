"""Free-balance requirements for planned TAO spends."""

from __future__ import annotations

import asyncio
from collections import defaultdict
from collections.abc import Iterable
from dataclasses import dataclass
from typing import TYPE_CHECKING, Optional

from .._generated import constants, storage
from ..balance import Balance
from ._money import UNBOUNDED, Spend

if TYPE_CHECKING:
    from .._substrate import Substrate


@dataclass(frozen=True)
class SpendProfile:
    """Bounded TAO spend and the latest prefix that must preserve the account.

    ``preserve_through`` is the cumulative spend immediately after a
    keep-alive child. Keeping the prefix instead of a batch-wide boolean avoids
    reserving the existential deposit after later expendable children.
    """

    total: Spend = None
    preserve_through: Optional[Balance] = None

    @classmethod
    def from_spend(cls, spend: Spend, *, preserve: bool = False) -> "SpendProfile":
        return cls(
            total=spend,
            preserve_through=spend if preserve and isinstance(spend, Balance) else None,
        )

    @classmethod
    def combine(cls, profiles: Iterable["SpendProfile"]) -> "SpendProfile":
        total = Balance.from_rao(0)
        preserve_through: Optional[Balance] = None
        bounded = False
        for profile in profiles:
            if profile.total is UNBOUNDED:
                return cls(total=UNBOUNDED)
            if not isinstance(profile.total, Balance):
                continue
            if profile.preserve_through is not None:
                candidate = total + profile.preserve_through
                if preserve_through is None or candidate > preserve_through:
                    preserve_through = candidate
            total += profile.total
            bounded = True
        return cls(total=total if bounded else None, preserve_through=preserve_through)


async def planned_fee_payer(
    substrate: "Substrate",
    *,
    proxy_for: Optional[str],
    signer_address: str,
    proxy_is_outer: bool,
) -> tuple[str, Optional[str]]:
    """Resolve the payer selected by the runtime's outer proxy wrapper."""
    if proxy_for is None or not proxy_is_outer:
        return signer_address, None
    try:
        delegates = await substrate.query_map(*storage.Proxy.RealPaysFee, [proxy_for])
    except Exception as error:
        return (
            signer_address,
            f"could not verify which proxy account pays the transaction fee: {error}",
        )
    opted_in = any(str(delegate) == signer_address for delegate, _value in delegates)
    return (proxy_for if opted_in else signer_address), None


async def affordability_blocks(
    substrate: "Substrate",
    *,
    profile: SpendProfile,
    fee: Optional[Balance],
    dispatch_origin: str,
    fee_payer: str,
) -> list[str]:
    """Return hard stops when a bounded spend cannot be funded.

    The semantic spend belongs to ``dispatch_origin`` while the outer
    extrinsic fee belongs to ``fee_payer``. They are usually the same account,
    but proxy and multisig execution deliberately separate them.
    """
    if not isinstance(profile.total, Balance):
        return []
    if fee is None:
        return ["could not verify affordability because the transaction fee is unavailable"]

    spends: dict[str, int] = defaultdict(int)
    fees: dict[str, int] = defaultdict(int)
    parts: dict[str, list[str]] = defaultdict(list)
    spends[dispatch_origin] += profile.total.rao
    parts[dispatch_origin].append(f"spend {profile.total}")
    fees[fee_payer] += fee.rao
    parts[fee_payer].append(f"fee ~{fee}")

    accounts = list(dict.fromkeys([dispatch_origin, fee_payer]))
    reads = [substrate.query(*storage.System.Account, [account]) for account in accounts]
    reads.append(substrate.constant(*constants.Balances.ExistentialDeposit))
    try:
        results = await asyncio.gather(*reads)
    except Exception as error:
        return [f"could not verify free TAO for this spend: {error}"]
    states = results[: len(accounts)]
    deposit = int(results[-1])

    blocks = []
    for address, state in zip(accounts, states):
        data = (state or {}).get("data") or {}
        free = int(data.get("free") or 0)
        frozen = max(0, int(data.get("frozen") or 0) - int(data.get("reserved") or 0))
        spend_rao = spends[address]
        fee_rao = fees[address]

        # Transaction payment withdraws the fee first with
        # ``Preservation::Preserve``. The semantic call then spends from its
        # dispatch origin and may or may not preserve that account. Both
        # intermediate and final balance constraints must hold.
        after_fee_floor = max(frozen, deposit) if fee_rao else frozen
        fee_stage_needed = fee_rao + after_fee_floor
        semantic_stage_needed = fee_rao + spend_rao + frozen
        semantic_floor = frozen
        if address == dispatch_origin and profile.preserve_through is not None:
            preserve_floor = max(frozen, deposit)
            preserve_needed = fee_rao + profile.preserve_through.rao + preserve_floor
            if preserve_needed > semantic_stage_needed:
                semantic_stage_needed = preserve_needed
                semantic_floor = preserve_floor
        needed = max(fee_stage_needed, semantic_stage_needed)
        if free < needed:
            reasons = list(parts[address])
            floor = after_fee_floor if fee_stage_needed >= semantic_stage_needed else semantic_floor
            if floor:
                label = (
                    "existential deposit"
                    if floor == deposit and deposit >= frozen
                    else "frozen balance"
                )
                reasons.append(f"{label} {Balance.from_rao(floor)}")
            blocks.append(
                f"free TAO ({Balance.from_rao(free)}) for {address} is below "
                f"the required {Balance.from_rao(needed)} ({' + '.join(reasons)})"
            )
    return blocks


__all__ = ["SpendProfile", "affordability_blocks", "planned_fee_payer"]
