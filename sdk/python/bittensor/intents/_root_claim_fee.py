"""Claim-fee preview for ``claim_root`` / ``claim_root_with_hotkey``.

Coldkey-wide claims reserve ``MAX_ROOT_CLAIM_WORK`` (256) weight units.
Single-hotkey claims reserve one basket's 129-unit envelope. Both refund down
to the work actually done.

This module estimates both numbers, compares the spent fee to accrued yield,
and tells the caller when a claim loses money or cannot even be included.
"""

from __future__ import annotations

import asyncio
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
# The coldkey-wide path reserves ``MAX_ROOT_CLAIM_WORK`` of these; the
# single-hotkey path reserves ``MAX_ROOT_CLAIM_HOTKEY_WORK``. Both also pay
# any non-weight base/length fee returned by ``payment_info``.
_APPROX_REDEEM_FEE_RAO = 447_500
_MAX_ROOT_CLAIM_WORK = 256
_MAX_ROOT_CLAIM_HOTKEY_WORK = 129

# Default ``RootClaimableThreshold`` (500_000 rao) when storage is empty.
_DEFAULT_THRESHOLD_RAO = 500_000


def root_claim_declared_work(hotkeys: Optional[list[str]]) -> int:
    """Return the runtime's declared work envelope for the selected call."""
    return _MAX_ROOT_CLAIM_WORK if hotkeys is None else _MAX_ROOT_CLAIM_HOTKEY_WORK


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


def _admission_blocks(hotkeys: int, networks: int, holdings: int) -> list[str]:
    work = hotkeys * networks
    if work <= _MAX_ROOT_CLAIM_WORK and holdings <= _MAX_ROOT_CLAIM_WORK:
        return []
    reasons: list[str] = []
    if work > _MAX_ROOT_CLAIM_WORK:
        reasons.append(f"{hotkeys} hotkeys × {networks} networks = {work}")
    if holdings > _MAX_ROOT_CLAIM_WORK:
        reasons.append(f"{holdings} basket holdings")
    remediation = (
        "claim one validator at a time with claim_root_with_hotkey"
        if hotkeys > 1
        else "the claim cannot be admitted until this basket's work is reduced"
    )
    return [
        "root claim exceeds the 256-unit admission limit ("
        + "; ".join(reasons)
        + f"); {remediation}"
    ]


@dataclass(frozen=True)
class RootClaimAdmission:
    """Structural work the runtime checks before charging the declared fee."""

    hotkeys: tuple[str, ...]
    holding_counts: tuple[int, ...]
    networks: int

    @property
    def holdings(self) -> int:
        return sum(self.holding_counts)

    @property
    def too_heavy(self) -> bool:
        return (
            len(self.hotkeys) * self.networks > _MAX_ROOT_CLAIM_WORK
            or self.holdings > _MAX_ROOT_CLAIM_WORK
        )

    def blocks(self) -> list[str]:
        return _admission_blocks(len(self.hotkeys), self.networks, self.holdings)


@dataclass(frozen=True)
class RootClaimWork:
    """Actual runtime work split by full redemption and lightweight scans."""

    hotkeys: int
    redeem_holdings: int
    scan_holdings: int


@dataclass(frozen=True)
class RootClaimReserve:
    """Mandatory affordability state, independent of optional payout preview."""

    reserved: Balance
    free: Balance
    exact: bool

    def blocks(self) -> list[str]:
        if self.free.rao >= self.reserved.rao:
            return []
        return [f"free TAO ({self.free}) is below the reserved claim fee ({self.reserved})"]


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
    hotkeys: int
    eligible_hotkeys: int
    below_threshold_hotkeys: int
    redeemable: Balance

    @property
    def refund(self) -> Balance:
        return Balance.from_rao(max(0, self.reserved.rao - self.spent.rao))

    @property
    def loses_money(self) -> bool:
        return self.spent.rao > self.redeemable.rao

    @property
    def reserve_shortfall(self) -> bool:
        return self.free.rao < self.reserved.rao

    @property
    def below_threshold(self) -> bool:
        return self.eligible_hotkeys == 0 and self.below_threshold_hotkeys > 0

    @property
    def too_heavy(self) -> bool:
        return (
            self.hotkeys * self.networks > _MAX_ROOT_CLAIM_WORK
            or self.holdings > _MAX_ROOT_CLAIM_WORK
        )

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
        elif self.below_threshold_hotkeys:
            lines.append(
                f"{self.below_threshold_hotkeys} of {self.hotkeys} validators are below "
                f"the per-validator claim threshold ({self.threshold}) and remain unclaimed"
            )
        if self.loses_money:
            lines.append(
                f"this claim loses money: spent fee ~{self.spent} exceeds "
                f"redeemable accrued {self.redeemable}"
            )
        return lines

    def warnings(self) -> list[str]:
        out: list[str] = []
        if self.below_threshold:
            out.append(
                f"accrued {self.accrued} is below the claim threshold "
                f"({self.threshold}); the claim pays a scan fee and realizes nothing"
            )
        elif self.below_threshold_hotkeys:
            out.append(
                f"{self.below_threshold_hotkeys} of {self.hotkeys} validators are individually "
                f"below the claim threshold ({self.threshold}); their yield remains accrued"
            )
        if not self.below_threshold and self.loses_money:
            out.append(
                f"this claim loses money: spent fee ~{self.spent} exceeds "
                f"redeemable accrued {self.redeemable}; wait until more yield accrues"
            )
        return out

    def blocks(self) -> list[str]:
        out: list[str] = []
        if self.too_heavy:
            out.extend(_admission_blocks(self.hotkeys, self.networks, self.holdings))
        if self.reserve_shortfall:
            out.append(f"free TAO ({self.free}) is below the reserved claim fee ({self.reserved})")
        return out


async def root_claim_admission(
    substrate: Any,
    claimant_address: str,
    *,
    hotkeys: Optional[list[str]],
) -> RootClaimAdmission:
    """Read only the state used by the runtime's 256-unit admission guard.

    Unlike the fee/yield quote, this check is not best-effort: callers use a
    failed read as a hard stop because signing an unverifiable claim can burn
    the full unreduced declared fee.
    """
    if hotkeys is None:
        raw_keys = await substrate.query(*st.SubtensorModule.StakingHotkeys, [claimant_address])
        selected = tuple(str(key) for key in (raw_keys or []))
    else:
        if len(hotkeys) != 1:
            raise ValueError("per-validator admission expects exactly one hotkey")
        selected = tuple(hotkeys)

    semaphore = asyncio.Semaphore(16)

    async def holding_count(hotkey: str) -> int:
        async with semaphore:
            rows = await substrate.runtime_call(
                *BetaBasketRuntimeApi.get_validator_basket,
                [hotkey],
            )
        if rows is None:
            raise RuntimeError(f"validator basket is unavailable for {hotkey}")
        return len(rows)

    holding_counts = await asyncio.gather(*(holding_count(hotkey) for hotkey in selected))

    networks = await _existing_network_count(substrate)
    return RootClaimAdmission(
        hotkeys=selected,
        holding_counts=tuple(holding_counts),
        networks=networks,
    )


async def quote_root_claim_fee(
    substrate: Any,
    claimant_address: str,
    *,
    fee_payer_address: Optional[str] = None,
    hotkeys: Optional[list[str]],
    compose: Callable[[], Awaitable[Any]],
    call: Any = None,
    admission: Optional[RootClaimAdmission] = None,
    reserve: Optional[RootClaimReserve] = None,
) -> Optional[RootClaimFeeQuote]:
    """Estimate reserved vs spent fee for a root claim.

    ``hotkeys`` is one validator (``claim_root_with_hotkey``) or ``None`` to
    walk every hotkey the coldkey root-stakes to (``claim_root``).

    Returns ``None`` when the basket runtime APIs are missing (offline
    harness) or any read fails. Callers must treat that as "no preview".
    """
    try:
        return await _quote(
            substrate,
            claimant_address,
            fee_payer_address=fee_payer_address or claimant_address,
            hotkeys=hotkeys,
            compose=compose,
            call=call,
            admission=admission,
            reserve=reserve,
        )
    except Exception:
        return None


async def _quote(
    substrate: Any,
    claimant_address: str,
    *,
    fee_payer_address: str,
    hotkeys: Optional[list[str]],
    compose: Callable[[], Awaitable[Any]],
    call: Any,
    admission: Optional[RootClaimAdmission],
    reserve: Optional[RootClaimReserve],
) -> Optional[RootClaimFeeQuote]:
    coldkey_wide = hotkeys is None
    declared_work = root_claim_declared_work(hotkeys)
    if admission is None:
        admission = await root_claim_admission(
            substrate,
            claimant_address,
            hotkeys=hotkeys,
        )
    elif hotkeys is not None and tuple(hotkeys) != admission.hotkeys:
        raise ValueError("root-claim admission does not match the selected hotkey")
    selected_hotkeys = list(admission.hotkeys)

    if reserve is None:
        reserve = await root_claim_reserve(
            substrate,
            fee_payer_address,
            compose=compose,
            call=call,
            declared_work=declared_work,
        )

    if coldkey_wide:
        positions = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_root_basket_positions,
            [claimant_address],
        )
        if positions is None:
            return None
        by_hotkey = {str(hotkey): int(payout) for hotkey, _shares, payout in positions}
        # The runtime API omits validators for which this coldkey has no owed
        # shares. Preserve that distinction: a missing position exits before
        # the runtime's basket scan and is not a below-threshold entitlement.
        payouts: list[Optional[int]] = [by_hotkey.get(hotkey) for hotkey in selected_hotkeys]
        accrued_rao = sum(payout for payout in payouts if payout is not None)
    else:
        payout = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_basket_payout,
            [selected_hotkeys[0], claimant_address],
        )
        if payout is None:
            return None
        payouts = [int(payout)]
        accrued_rao = payouts[0]

    threshold_rao = await _threshold_rao(substrate)

    holding_counts = list(admission.holding_counts)
    eligible = [payout is not None and payout >= threshold_rao for payout in payouts]
    below_threshold = [payout is not None and payout < threshold_rao for payout in payouts]
    redeem_holdings = sum(
        count for count, can_redeem in zip(holding_counts, eligible) if can_redeem
    )
    scan_holdings = sum(count for count, below in zip(holding_counts, below_threshold) if below)
    redeemable_rao = sum(
        payout for payout, can_redeem in zip(payouts, eligible) if payout is not None and can_redeem
    )
    holdings = sum(holding_counts)
    spent = _spent_fee(
        reserve.reserved,
        RootClaimWork(
            hotkeys=max(len(selected_hotkeys), 1),
            redeem_holdings=redeem_holdings,
            scan_holdings=scan_holdings,
        ),
        declared_work=declared_work,
    )

    return RootClaimFeeQuote(
        holdings=holdings,
        networks=admission.networks,
        reserved=reserve.reserved,
        spent=spent,
        accrued=Balance.from_rao(accrued_rao),
        free=reserve.free,
        threshold=Balance.from_rao(threshold_rao),
        hotkeys=len(selected_hotkeys),
        eligible_hotkeys=sum(eligible),
        below_threshold_hotkeys=sum(below_threshold),
        redeemable=Balance.from_rao(redeemable_rao),
    )


async def _existing_network_count(substrate: Any) -> int:
    rows = await substrate.query_map(*st.SubtensorModule.NetworksAdded)
    if rows is None:
        raise RuntimeError("existing-network map is unavailable")
    return max(sum(1 for _netuid, added in rows if added), 1)


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
    call: Any = None,
) -> Balance:
    return (
        await _reserved_fee_with_status(
            substrate,
            signer_address,
            compose,
            call=call,
            declared_work=declared_work,
        )
    )[0]


async def _reserved_fee_with_status(
    substrate: Any,
    signer_address: str,
    compose: Callable[[], Awaitable[Any]],
    *,
    call: Any = None,
    declared_work: int = _MAX_ROOT_CLAIM_WORK,
) -> tuple[Balance, bool]:
    try:
        if call is None:
            call = await compose()
        return await substrate.estimate_fee(call, _FeeView(signer_address)), True
    except Exception:
        return Balance.from_rao(_APPROX_REDEEM_FEE_RAO * max(declared_work, 1)), False


async def root_claim_reserve(
    substrate: Any,
    fee_payer_address: str,
    *,
    compose: Callable[[], Awaitable[Any]],
    call: Any = None,
    declared_work: int = _MAX_ROOT_CLAIM_WORK,
) -> RootClaimReserve:
    """Read mandatory reserve/free state even when yield preview is unavailable."""
    free_rao = await _free_rao(substrate, fee_payer_address)
    reserved, exact = await _reserved_fee_with_status(
        substrate,
        fee_payer_address,
        compose,
        call=call,
        declared_work=declared_work,
    )
    return RootClaimReserve(
        reserved=reserved,
        free=Balance.from_rao(free_rao),
        exact=exact,
    )


def _spent_fee(
    reserved: Balance,
    work: RootClaimWork,
    *,
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
    active = max(work.redeem_holdings, work.hotkeys, 1)
    active_weight = weight_part * active // declared_work
    scan_weight = (
        weight_part
        * max(work.scan_holdings, 0)
        * _SCAN_REF_TIME
        // (declared_work * _REDEEM_REF_TIME)
    )
    spent_weight = active_weight + scan_weight
    return Balance.from_rao(min(reserved.rao, base_part + max(spent_weight, 0)))
