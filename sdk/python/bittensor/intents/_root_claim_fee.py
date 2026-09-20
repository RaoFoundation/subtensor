"""Claim-fee preview for ``claim_root`` / ``claim_root_with_hotkey``.

Coldkey-wide claims declare ``MAX_ROOT_CLAIM_WORK`` (256) weight units for
admission. Single-hotkey claims declare one basket's 129-unit envelope — or,
since spec 468, only the cheap cash path (a scan plus one transfer, plus the
flush of the credits actually queued) when the fund's TAO cash slot is ready to
pay the claim (``root_claim_cash_ready``). Since spec 467 the fee wrapper
charges either call as if only ``ROOT_CLAIM_FEE_ALLOWANCE`` (4) units were
declared, never more than what was declared; the rest of the declaration is a
fee subsidy. Both still refund down to the work actually done when that is
below the allowance. The reserve is what people see leave their free balance,
and it is at least the fee that finally settles.

This module estimates both numbers, compares the spent fee to accrued yield,
and tells the caller when a claim loses money or cannot even be included.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Optional

from .._generated import storage as st
from .._generated.runtime_apis import BetaBasketRuntimeApi, StakeInfoRuntimeApi
from ..balance import Balance
from ..sp_core import ss58_decode

# Mirrors of the runtime's claim pricing (spec 468). Sources:
# ``pallets/subtensor/src/weights.rs`` (``claim_root``, ``claim_root_scan``),
# ``pallets/subtensor/src/staking/claim_root.rs`` (``basket_nav_sweep_weight``,
# ``root_claim_weight_for_work``), ``basket_flush.rs`` (flush bound) and
# ``runtime/src/staking_fee.rs`` (``ROOT_CLAIM_FEE_ALLOWANCE``,
# ``root_claim_fee_weight``). Update together with a re-benchmark.
_ROCKSDB_READ_PS = 25_000_000
_ROCKSDB_WRITE_PS = 100_000_000
# ``LinearWeightToFee``: 0.00025 rao per ref_time unit, rounded to nearest.
_WEIGHT_FEE_PARTS = 250_000
_PERBILL = 1_000_000_000
_MAX_ROOT_CLAIM_WORK = 256
_MAX_ROOT_CLAIM_HOTKEY_WORK = 129
# ``MAX_BASKET_ROWS`` and the flat flush allowance every claim declares:
# ``10 * MAX_BASKET_ROWS`` quotes and ``2 * MAX_BASKET_ROWS`` rows.
_MAX_BASKET_ROWS = 256
_FLUSH_BOUND_QUOTES = 10 * _MAX_BASKET_ROWS
_FLUSH_BOUND_ROWS = 2 * _MAX_BASKET_ROWS
# Runtime ``ROOT_CLAIM_FEE_ALLOWANCE``: claim units the fee wrapper charges for.
_ROOT_CLAIM_FEE_ALLOWANCE = 4
# Cash path (spec 468): ``root_claim_cash_extra_weight`` reads and writes, and the
# per-hotkey flush bound ``4Q + 2H`` quotes / ``Q`` rows with ``H`` at the row cap.
_CASH_EXTRA_READS = 4
_CASH_EXTRA_WRITES = 3
# Default ``BasketCashClaimCap`` (1% of guarded NAV per day) and the bucket-room
# fraction the cash path needs to open (``BASKET_CASH_READY_BUCKET_FRACTION``).
_DEFAULT_CASH_CLAIM_CAP = 65_535 // 100
_CASH_READY_BUCKET_FRACTION = 4
_REFILL_BLOCKS = 7_200
_U16_MAX = 65_535
# Runtime ``fee_weight_cap_459`` for both claim calls: the call weight quoted on spec
# 459 (includes the 60_000_000 ref_time dispatch-extension fold the model below omits).
_ROOT_CLAIM_CAP_459_REF_TIME = 249_916_000_000
_EXTENSION_FOLD_REF_TIME = 60_000_000


def _claim_root_ref_time(units: int) -> int:
    """``WeightInfo::claim_root(h)`` ref_time."""
    return (
        567_441_000
        + 437_318_379 * units
        + (23 + 22 * units) * _ROCKSDB_READ_PS
        + (18 + 16 * units) * _ROCKSDB_WRITE_PS
    )


def _claim_root_scan_ref_time(units: int) -> int:
    """``WeightInfo::claim_root_scan(h)`` ref_time."""
    return 309_208_000 + 263_604_241 * units + (6 + 21 * units) * _ROCKSDB_READ_PS


def _basket_flush_ref_time(quotes: int, rows: int) -> int:
    """``Pallet::basket_flush_weight`` ref_time: NAV-sweep quotes plus redeem-priced rows."""
    sweep = 0 if quotes == 0 else (10_000_000 + 4 * _ROCKSDB_READ_PS) * max(quotes, 1)
    redeem = 0 if rows == 0 else _claim_root_ref_time(rows)
    return sweep + redeem


def _claim_ref_time(units: int, quotes: int, rows: int) -> int:
    """``Pallet::root_claim_weight_for_work(units, flush)`` ref_time."""
    return (
        _claim_root_ref_time(units)
        + _claim_root_scan_ref_time(units)
        + _basket_flush_ref_time(quotes, rows)
    )


def _weight_fee_rao(ref_time: int) -> int:
    """``LinearWeightToFee`` on a ref_time: nearest rao, ties round down."""
    scaled = ref_time * _WEIGHT_FEE_PARTS
    quotient, remainder = divmod(scaled, _PERBILL)
    return quotient + (1 if remainder * 2 > _PERBILL else 0)


def root_claim_declared_work(hotkeys: Optional[list[str]]) -> int:
    """Admission envelope for this claim path."""
    if hotkeys is not None:
        return _MAX_ROOT_CLAIM_HOTKEY_WORK
    return _MAX_ROOT_CLAIM_WORK


def _fee_units(limit: int) -> int:
    """Claim units the fee wrapper charges for a call admitted under ``limit``."""
    return min(limit, _ROOT_CLAIM_FEE_ALLOWANCE)


def root_claim_declared_ref_time(limit: int) -> int:
    """Declared call weight (ref_time) of a claim admitted under ``limit`` units on the
    full redemption path."""
    return _claim_ref_time(limit, _FLUSH_BOUND_QUOTES, _FLUSH_BOUND_ROWS)


def root_claim_cash_declared_ref_time(queued: int) -> int:
    """Declared call weight (ref_time) of a single-hotkey claim on the cash path
    (``Pallet::root_claim_cash_declared_weight``): one redeemed unit, a scan over the
    129-unit row cap, the flush of ``queued`` credits (``4Q + 2H`` quotes, ``Q`` rows)
    and the cash bookkeeping reads and writes."""
    queued = max(0, min(queued, _FLUSH_BOUND_ROWS))
    rows = _MAX_ROOT_CLAIM_HOTKEY_WORK
    return (
        _claim_root_ref_time(1)
        + _claim_root_scan_ref_time(rows)
        + _basket_flush_ref_time(4 * queued + 2 * rows, queued)
        + _CASH_EXTRA_READS * _ROCKSDB_READ_PS
        + _CASH_EXTRA_WRITES * _ROCKSDB_WRITE_PS
    )


def root_claim_charged_ref_time(limit: int, *, declared_ref_time: Optional[int] = None) -> int:
    """Weight the fee wrapper charges: the allowance plus one hotkey's flush work for
    that many queued credits and holdings (``4Q + 2H`` quotes, ``Q`` rows), never above
    what the call declared (``declared_ref_time``; the full envelope when omitted)."""
    units = _fee_units(limit)
    allowance = min(
        _claim_ref_time(units, 6 * units, units),
        _ROOT_CLAIM_CAP_459_REF_TIME - _EXTENSION_FOLD_REF_TIME,
    )
    if declared_ref_time is None:
        declared_ref_time = root_claim_declared_ref_time(limit)
    return min(allowance, declared_ref_time)


def root_claim_fee_discount_ref_time(limit: int, *, declared_ref_time: Optional[int] = None) -> int:
    """``FeeWeightDiscount`` the runtime subtracts for one claim under ``limit`` declared
    at ``declared_ref_time`` (the full envelope when omitted). A batch or proxy of claims
    is discounted by the sum over its inner claims."""
    if declared_ref_time is None:
        declared_ref_time = root_claim_declared_ref_time(limit)
    return max(
        0,
        declared_ref_time - root_claim_charged_ref_time(limit, declared_ref_time=declared_ref_time),
    )


def _bucket_level_at(stored: Optional[tuple[int, int]], now: int, budget: int) -> int:
    """``Pallet::basket_bucket_level_at``: stored level plus the linear refill, clamped."""
    if stored is None:
        return budget
    level, last_block = stored
    elapsed = max(0, now - last_block)
    return min(level + budget * elapsed // _REFILL_BLOCKS, budget)


def root_claim_cash_ready(
    cash_rao: int,
    threshold_rao: int,
    cap: int,
    bucket: Optional[tuple[int, int]],
    now: int,
) -> bool:
    """``Pallet::root_claim_cash_ready`` offline: the fund's root (cash) slot holds at
    least the claim threshold (and one rao) and its cash-claim bucket, sized from the cash
    slot as a lower bound of the guarded NAV, holds at least a quarter of a day's budget."""
    if cash_rao <= 0 or cash_rao < threshold_rao:
        return False
    budget_floor = cash_rao * cap // _U16_MAX
    room = _bucket_level_at(bucket, now, budget_floor)
    return room > 0 and room * _CASH_READY_BUCKET_FRACTION >= budget_floor


def _approx_declared_fee_rao(limit: int) -> int:
    """Weight slice of the quoted fee for a claim under ``limit`` (no base/length part)."""
    return _weight_fee_rao(root_claim_charged_ref_time(limit))


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


def _admission_blocks(
    hotkeys: int,
    holdings: int,
    limit: int,
    selection_scans: int,
) -> list[str]:
    work = hotkeys + holdings
    if work <= limit and selection_scans <= limit:
        return []
    remediation = (
        "claim one validator at a time with claim_root_with_hotkey"
        if hotkeys > 1
        else "the claim cannot be admitted until this basket's work is reduced"
    )
    reasons = []
    if work > limit:
        hotkey_label = "hotkey" if hotkeys == 1 else "hotkeys"
        holding_label = "holding" if holdings == 1 else "holdings"
        reasons.append(
            f"{hotkeys} root {hotkey_label} + {holdings} basket {holding_label} = {work}"
        )
    if selection_scans > limit:
        reasons.append(f"{selection_scans} staking-hotkey relationships to classify")
    return [
        f"root claim exceeds the {limit:,}-unit admission limit ("
        + "; ".join(reasons)
        + f"); {remediation}"
    ]


@dataclass(frozen=True)
class RootClaimAdmission:
    """Structural work the runtime checks before charging the declared fee."""

    hotkeys: tuple[str, ...]
    holding_counts: tuple[int, ...]
    networks: int
    limit: int
    selection_scans: int

    @property
    def holdings(self) -> int:
        return sum(self.holding_counts)

    @property
    def too_heavy(self) -> bool:
        return len(self.hotkeys) + self.holdings > self.limit or self.selection_scans > self.limit

    def blocks(self) -> list[str]:
        return _admission_blocks(
            len(self.hotkeys),
            self.holdings,
            self.limit,
            self.selection_scans,
        )


@dataclass(frozen=True)
class RootClaimWork:
    """Actual runtime work split by full redemption and lightweight scans."""

    hotkeys: int
    redeem_holdings: int
    scan_holdings: int
    selection_scans: int = 0


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
    admission_limit: int
    selection_scans: int
    # Spec 468: a single-hotkey claim the fund's cash slot can pay declares only the
    # cheap cash path and sells no holdings.
    cash_path: bool = False

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
            self.hotkeys + self.holdings > self.admission_limit
            or self.selection_scans > self.admission_limit
        )

    def facts(self) -> list[tuple[str, str]]:
        """The fee picture as (label, value) pairs for structured renderers."""
        kinds = "holding" if self.holdings == 1 else "holdings"
        rows = [
            ("holdings", f"{self.holdings} basket {kinds} (fee scales with ALPHA types)"),
            ("reserved", f"{self.reserved} at inclusion"),
            ("spent", f"~{self.spent}"),
        ]
        if self.refund.rao > 0:
            rows.append(("refunded", f"~{self.refund}"))
        rows.append(("accrued", str(self.accrued)))
        if self.cash_path:
            rows.append(("path", "paid from the fund's TAO cash; no holdings sold"))
        return rows

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
        if self.cash_path:
            lines.append(
                "the fund's TAO cash covers claims: paid from cash at the guarded mark, "
                "no holdings sold, cheap declared weight"
            )
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
            out.extend(
                _admission_blocks(
                    self.hotkeys,
                    self.holdings,
                    self.admission_limit,
                    self.selection_scans,
                )
            )
        if self.reserve_shortfall:
            out.append(f"free TAO ({self.free}) is below the reserved claim fee ({self.reserved})")
        return out


async def root_claim_admission(
    substrate: Any,
    claimant_address: str,
    *,
    hotkeys: Optional[list[str]],
) -> RootClaimAdmission:
    """Read only the state used by the runtime's fixed admission guard.

    Unlike the fee/yield quote, this check is not best-effort: callers use a
    failed read as a hard stop because signing an unverifiable claim can burn
    the full unreduced declared fee.
    """
    if hotkeys is None:
        raw_keys = await substrate.query(*st.SubtensorModule.StakingHotkeys, [claimant_address])
        raw_keys = tuple(str(key) for key in (raw_keys or []))
        stake_rows = await substrate.runtime_call(
            *StakeInfoRuntimeApi.get_stake_info_for_coldkey,
            [claimant_address],
        )
        if stake_rows is None:
            raise RuntimeError("coldkey stake positions are unavailable")
        root_hotkeys = {
            str(row["hotkey"])
            for row in stake_rows
            if int(row["netuid"]) == 0 and int(row["stake"]) > 0
        }
        watermarks = await asyncio.gather(
            *(
                substrate.query(
                    *st.SubtensorModule.BasketClaimed,
                    [hotkey, claimant_address],
                )
                for hotkey in raw_keys
            )
        )
        selected = tuple(
            hotkey
            for hotkey, watermark in zip(raw_keys, watermarks)
            if hotkey in root_hotkeys or int(watermark or 0) < 0
        )
        selection_scans = len(raw_keys)
    else:
        if len(hotkeys) != 1:
            raise ValueError("per-validator admission expects exactly one hotkey")
        selected = tuple(hotkeys)
        selection_scans = 0

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
        limit=root_claim_declared_work(hotkeys),
        selection_scans=selection_scans,
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
            declared_work=admission.limit,
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
    cash_path = False
    if not coldkey_wide:
        cash_path = await _cash_path_ready(substrate, selected_hotkeys[0], threshold_rao)

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
            selection_scans=admission.selection_scans,
        ),
        admission.limit,
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
        admission_limit=admission.limit,
        selection_scans=admission.selection_scans,
        cash_path=cash_path,
    )


async def _cash_path_ready(substrate: Any, hotkey: str, threshold_rao: int) -> bool:
    """Best-effort mirror of ``root_claim_cash_ready`` for the fee preview: reads the
    fund's cash slot, the cash-claim cap and bucket, and the current block. Storage items
    the connected runtime does not have (pre-468) read as their defaults."""
    rows = await substrate.runtime_call(*BetaBasketRuntimeApi.get_validator_basket, [hotkey])
    cash_rao = sum(int(row[1]) for row in (rows or []) if int(row[0]) == 0)
    cap_item = getattr(st.SubtensorModule, "BasketCashClaimCap", None)
    cap = _DEFAULT_CASH_CLAIM_CAP
    if cap_item is not None:
        raw_cap = await substrate.query(*cap_item)
        if raw_cap is not None:
            cap = int(raw_cap)
    bucket: Optional[tuple[int, int]] = None
    bucket_item = getattr(st.SubtensorModule, "BasketCashClaimBucket", None)
    if bucket_item is not None:
        raw_bucket = await substrate.query(*bucket_item, [hotkey])
        if raw_bucket is not None:
            bucket = (int(raw_bucket[0]), int(raw_bucket[1]))
    now = int(await substrate.query(*st.System.Number) or 0)
    return root_claim_cash_ready(cash_rao, threshold_rao, cap, bucket, now)


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
    call: Any = None,
    declared_work: int = _MAX_ROOT_CLAIM_WORK,
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
        return Balance.from_rao(_approx_declared_fee_rao(max(declared_work, 1))), False


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
    declared_work: int = _MAX_ROOT_CLAIM_WORK,
) -> Balance:
    """Refund unused charged weight; keep non-weight base/length fees intact.

    Runtime actual weight is ``claim_root(active) + claim_root_scan(scanned)`` plus the
    flush work really done, with ``active = max(selected hotkeys, relationships
    classified, realized + swept, 1)``. Classifying a relationship reads its root
    share-pool state, so it is priced as a full hotkey unit. The flush work is not
    knowable offline and is left out, so ``spent`` is a floor. ``estimate_fee`` prices
    the charged allowance plus extrinsic base/length; the fee wrapper caps the charge
    at that allowance, so spent never exceeds reserved.
    """
    if reserved.rao <= 0:
        return reserved
    charged = _approx_declared_fee_rao(declared_work)
    weight_part = min(reserved.rao, charged)
    base_part = reserved.rao - weight_part
    active = max(work.redeem_holdings, work.hotkeys, work.selection_scans, 1)
    actual = _weight_fee_rao(
        _claim_root_ref_time(active) + _claim_root_scan_ref_time(max(work.scan_holdings, 0))
    )
    return Balance.from_rao(min(reserved.rao, base_part + actual))
