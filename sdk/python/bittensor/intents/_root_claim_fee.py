"""Claim-fee preview for ``claim_root`` / ``claim_root_with_hotkey``.

Coldkey-wide claims reserve ``MAX_ROOT_CLAIM_WORK`` (256) weight units.
Single-hotkey claims reserve one basket's 129-unit envelope. Both refund down
to the work actually done.

This module estimates both numbers, compares the spent fee to accrued yield,
and tells the caller when a claim loses money or cannot even be included.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Optional

from .._generated import storage as st
from .._generated.runtime_apis import BetaBasketRuntimeApi
from ..balance import Balance
from ..sp_core import ss58_decode

# ``claim_root_scan`` ref_time / ``claim_root`` ref_time in weights.rs.
# A below-threshold walk only scans; a successful redeem pays the full row.
_SCAN_REF_TIME = 6
_REDEEM_REF_TIME = 70

# One full ``claim_root`` weight unit under LinearWeightToFee (~τ0.0004475).
# Both claim paths reserve ``MAX_ROOT_CLAIM_WORK`` of these, plus any
# non-weight base/length fee returned by ``payment_info``.
_APPROX_REDEEM_FEE_RAO = 447_500
_MAX_ROOT_CLAIM_WORK = 256
_MAX_ROOT_CLAIM_HOTKEY_WORK = 129

# Default ``RootClaimableThreshold`` (500_000 rao) when storage is empty.
_DEFAULT_THRESHOLD_RAO = 500_000


class _FeeView:
    """Public-only keypair shape for ``estimate_fee`` (zeroed signature)."""

    crypto_type = 1  # sr25519

    def __init__(self, address: str):
        self.ss58_address = address
        self.public_key = bytes(ss58_decode(address))


def _i96f32_rao(value: Any) -> int:
    if isinstance(value, dict):
        return int(value.get("bits") or 0) >> 32
    return int(value or 0) >> 32


@dataclass(frozen=True)
class RootClaimFeeQuote:
    """Best-effort reserved/spent fee picture for one root claim."""

    holdings: int
    networks: int
    reserved: Balance
    spent: Balance
    accrued: Balance
    free: Balance
    threshold: Balance
    below_threshold: bool

    @property
    def refund(self) -> Balance:
        return Balance.from_rao(max(0, self.reserved.rao - self.spent.rao))

    @property
    def loses_money(self) -> bool:
        return self.spent.rao > self.accrued.rao

    @property
    def reserve_shortfall(self) -> bool:
        return self.free.rao < self.reserved.rao

    def effects(self) -> list[str]:
        kinds = "holding" if self.holdings == 1 else "holdings"
        fee_line = f"reserved {self.reserved} at inclusion; spent ~{self.spent}"
        if self.refund.rao > 0:
            fee_line += f"; ~{self.refund} refunded"
        lines = [
            f"{self.holdings} basket {kinds} (fee scales with ALPHA types)",
            fee_line,
            f"accrued {self.accrued}",
        ]
        if self.below_threshold:
            lines.append(
                f"accrued is below the claim threshold ({self.threshold}); "
                "the claim is a no-op and you still pay the scan fee"
            )
        if self.loses_money:
            lines.append(
                f"this claim loses money: spent fee ~{self.spent} exceeds accrued {self.accrued}"
            )
        return lines

    def warnings(self) -> list[str]:
        out: list[str] = []
        if self.below_threshold:
            out.append(
                f"accrued {self.accrued} is below the claim threshold "
                f"({self.threshold}); the claim pays a scan fee and realizes nothing"
            )
        elif self.loses_money:
            out.append(
                f"this claim loses money: spent fee ~{self.spent} exceeds "
                f"accrued {self.accrued}; wait until more yield accrues"
            )
        return out

    def blocks(self) -> list[str]:
        if not self.reserve_shortfall:
            return []
        return [f"free TAO ({self.free}) is below the reserved claim fee ({self.reserved})"]


async def quote_root_claim_fee(
    substrate: Any,
    signer_address: str,
    *,
    hotkeys: Optional[list[str]],
    compose: Callable[[], Awaitable[Any]],
) -> Optional[RootClaimFeeQuote]:
    """Estimate reserved vs spent fee for a root claim.

    ``hotkeys`` is one validator (``claim_root_with_hotkey``) or ``None`` to
    walk every hotkey the coldkey root-stakes to (``claim_root``).

    Returns ``None`` when the basket runtime APIs are missing (offline
    harness) or any read fails. Callers must treat that as "no preview".
    """
    try:
        return await _quote(substrate, signer_address, hotkeys=hotkeys, compose=compose)
    except Exception:
        return None


async def _quote(
    substrate: Any,
    signer_address: str,
    *,
    hotkeys: Optional[list[str]],
    compose: Callable[[], Awaitable[Any]],
) -> Optional[RootClaimFeeQuote]:
    coldkey_wide = hotkeys is None
    if coldkey_wide:
        owed = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_root_basket_owed, [signer_address]
        )
        if owed is None:
            return None
        accrued_rao = int(owed)
        raw_keys = await substrate.query(*st.SubtensorModule.StakingHotkeys, [signer_address])
        hotkeys = [str(key) for key in (raw_keys or [])]
    else:
        if len(hotkeys) != 1:
            raise ValueError("per-validator quote expects exactly one hotkey")
        payout = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_basket_payout,
            [hotkeys[0], signer_address],
        )
        if payout is None:
            return None
        accrued_rao = int(payout)

    holdings = 0
    for hotkey in hotkeys:
        rows = await substrate.runtime_call(*BetaBasketRuntimeApi.get_validator_basket, [hotkey])
        if rows is None:
            return None
        holdings += len(rows)

    networks = await _existing_network_count(substrate)
    declared_work = _MAX_ROOT_CLAIM_WORK if coldkey_wide else _MAX_ROOT_CLAIM_HOTKEY_WORK
    threshold_rao = await _threshold_rao(substrate)
    free_rao = await _free_rao(substrate, signer_address)
    reserved = await _reserved_fee(
        substrate,
        signer_address,
        compose,
        declared_work=declared_work,
    )
    spent = _spent_fee(
        reserved,
        holdings,
        accrued_rao < threshold_rao,
        hotkey_count=max(len(hotkeys), 1),
        declared_work=declared_work,
    )

    return RootClaimFeeQuote(
        holdings=holdings,
        networks=max(networks, 1),
        reserved=reserved,
        spent=spent,
        accrued=Balance.from_rao(accrued_rao),
        free=Balance.from_rao(free_rao),
        threshold=Balance.from_rao(threshold_rao),
        below_threshold=accrued_rao < threshold_rao,
    )


async def _existing_network_count(substrate: Any) -> int:
    rows = await substrate.query_map(*st.SubtensorModule.NetworksAdded)
    return sum(1 for _netuid, added in (rows or []) if added)


async def _threshold_rao(substrate: Any) -> int:
    raw = await substrate.query(*st.SubtensorModule.RootClaimableThreshold, [0])
    if raw is None:
        return _DEFAULT_THRESHOLD_RAO
    decoded = _i96f32_rao(raw)
    return decoded if decoded > 0 else _DEFAULT_THRESHOLD_RAO


async def _free_rao(substrate: Any, ss58: str) -> int:
    account = await substrate.query(*st.System.Account, [ss58])
    return int(((account or {}).get("data") or {}).get("free") or 0)


async def _reserved_fee(
    substrate: Any,
    signer_address: str,
    compose: Callable[[], Awaitable[Any]],
    *,
    declared_work: int = _MAX_ROOT_CLAIM_WORK,
) -> Balance:
    try:
        call = await compose()
        return await substrate.estimate_fee(call, _FeeView(signer_address))
    except Exception:
        return Balance.from_rao(_APPROX_REDEEM_FEE_RAO * max(declared_work, 1))


def _spent_fee(
    reserved: Balance,
    holdings: int,
    scan_only: bool,
    *,
    hotkey_count: int = 1,
    declared_work: int = _MAX_ROOT_CLAIM_WORK,
) -> Balance:
    """Refund unused declared units; keep non-weight base/length fees intact.

    Runtime active units are ``max(hotkey_count, realized + swept, 1)``. The
    quote floors by the selected hotkey count so empty-basket validators still
    cost a full unit. ``estimate_fee`` prices ``declared_work`` plus extrinsic
    base/length; only the weight slice scales.
    """
    if reserved.rao <= 0:
        return reserved
    declared_work = max(declared_work, 1)
    declared_weight = _APPROX_REDEEM_FEE_RAO * declared_work
    weight_part = min(reserved.rao, declared_weight)
    base_part = max(0, reserved.rao - declared_weight)
    hotkeys = max(hotkey_count, 1)
    if scan_only:
        scan_weight = (
            weight_part * max(holdings, 0) * _SCAN_REF_TIME // (declared_work * _REDEEM_REF_TIME)
        )
        walk_weight = weight_part * hotkeys // declared_work
        spent_weight = walk_weight + scan_weight
    else:
        units = max(holdings, hotkeys)
        spent_weight = weight_part * units // declared_work
    return Balance.from_rao(min(reserved.rao, base_part + max(spent_weight, 0)))


async def cached_root_claim_quote(
    intent: Any,
    substrate: Any,
    signer_address: str,
    *,
    hotkeys: Optional[list[str]],
    compose: Callable[[], Awaitable[Any]],
) -> Optional[RootClaimFeeQuote]:
    """Return the quote for ``intent``, fetching at most once per instance."""
    if getattr(intent, "_root_claim_quote_loaded", False):
        return getattr(intent, "_root_claim_quote", None)
    quote = await quote_root_claim_fee(substrate, signer_address, hotkeys=hotkeys, compose=compose)
    intent._root_claim_quote = quote
    intent._root_claim_quote_loaded = True
    return quote
