"""Claim-fee preview for ``claim_root`` / ``claim_root_with_hotkey``.

Coldkey-wide claims declare ``MAX_ROOT_CLAIM_WORK`` (256) weight units for
admission. Since runtime 495 a single-hotkey claim declares the weight of the
validator's own fund: its stored basket rows and its queued dividend credits,
up to a fund of 258 claim units (256 subnets, the root cash slot and the
validator). Each declaration also has a fixed part for the settlement of the
claimant's deferred-dividend debts, which the fund does not bound. A runtime
before 495 admits 130 claim units and counts only the validator and its stored
basket rows. The admission check here applies the rules of the runtime version
at the block it reads the fund from, so a claim that passes it is not refused
by an older chain.

Since spec 467 the fee wrapper charges either call as if only
``ROOT_CLAIM_FEE_ALLOWANCE`` (4) units were declared; the rest of the envelope
is a fee subsidy. Both still refund down to the work actually done when that is
below the allowance. The reserve is what people see leave their free balance,
and it is at least the fee that finally settles.

This module estimates both numbers, compares the spent fee to accrued yield,
and tells the caller when a claim loses money or cannot even be included.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Optional

from .._generated import storage as st
from .._generated.runtime_apis import BetaBasketRuntimeApi, StakeInfoRuntimeApi
from ..balance import Balance
from ..sp_core import ss58_decode

# Mirrors of the runtime's claim pricing (fee allowance from spec 467; single-hotkey
# admission ceiling 258 and fund-sized declaration from spec 495). Sources:
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
# Runtime ``MAX_ROOT_CLAIM_HOTKEY_WORK``: the largest fund one single-hotkey claim admits,
# in claim units (the validator, its basket rows, and the rows its queue can open).
_MAX_ROOT_CLAIM_HOTKEY_WORK = 258
# Runtime ``MIN_ROOT_CLAIM_HOTKEY_WORK``: the smallest single-hotkey declaration.
_MIN_ROOT_CLAIM_HOTKEY_WORK = 4
# First runtime ``spec_version`` with the single-hotkey rules above. An earlier runtime
# admits ``_MAX_ROOT_CLAIM_HOTKEY_WORK_BEFORE_495`` claim units, counts the validator and
# its stored basket rows only, and does not count queued dividend credits.
_FUND_SIZED_ROOT_CLAIM_SPEC = 495
_MAX_ROOT_CLAIM_HOTKEY_WORK_BEFORE_495 = 130
# Runtime ``root_claim_debt_settlement_weight_bound``: every single-hotkey claim declares
# the settlement of the most deferred-dividend debts that one claimant can hold on one
# validator (``MAX_ROOT_CLAIM_WORK``). That is one read and one write for each debt, and
# three more of each for the debt map and the two watermarks.
_DEBT_SETTLEMENT_BOUND_REF_TIME = (_MAX_ROOT_CLAIM_WORK + 3) * (
    _ROCKSDB_READ_PS + _ROCKSDB_WRITE_PS
)
# ``MAX_BASKET_ROWS`` and the flat flush allowance every claim declares:
# ``10 * MAX_BASKET_ROWS`` quotes and ``2 * MAX_BASKET_ROWS`` rows.
_MAX_BASKET_ROWS = 256
_FLUSH_BOUND_QUOTES = 10 * _MAX_BASKET_ROWS
_FLUSH_BOUND_ROWS = 2 * _MAX_BASKET_ROWS
# Runtime ``ROOT_CLAIM_FEE_ALLOWANCE``: claim units the fee wrapper charges for.
_ROOT_CLAIM_FEE_ALLOWANCE = 4
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


def _flush_quotes_ref_time(quotes: int) -> int:
    """``Pallet::basket_flush_weight`` ref_time for ``quotes`` read-only quotes and no
    rows: the NAV-sweep price, plus seven stability-check reads for each quote up to
    ``MAX_BASKET_ROWS``."""
    if quotes == 0:
        return 0
    return (10_000_000 + 4 * _ROCKSDB_READ_PS) * quotes + (
        min(quotes, _MAX_BASKET_ROWS) * 7 * _ROCKSDB_READ_PS
    )


def _hotkey_claim_bound_ref_time(units: int, queued: int) -> int:
    """``Pallet::root_claim_hotkey_weight_for_work(units, queued)`` ref_time: ``units``
    claim units, two rows of work for each queued credit, and the quotes of the flush."""
    quotes = 0 if queued == 0 else 4 * queued + 2 * units
    return (
        _claim_root_ref_time(units + 2 * queued)
        + _claim_root_scan_ref_time(units)
        + _flush_quotes_ref_time(quotes)
    )


# Runtime ``ROOT_CLAIM_HOTKEY_COUNTS``: a transaction counts a fund at most two times. It
# counts one time and keeps the count; the first transaction of a block can count twice.
_HOTKEY_CLAIM_COUNTS = 2
# Runtime ``ROOT_CLAIM_HOTKEY_COUNT_READS``: reads of the kept count and of the
# same-transaction guard that one declaration covers.
_HOTKEY_CLAIM_COUNT_READS = 10


def _hotkey_claim_sizing_ref_time(rows: int, queued: int) -> int:
    """``Pallet::root_claim_hotkey_sizing_weight(rows, queued)`` ref_time. One count of the
    fund reads each stored row and each queued credit, and probes the end of both maps.
    The declaration has ``_HOTKEY_CLAIM_COUNTS`` counts, the reads that find the kept count
    again, and the one write that keeps it.

    A hotkey without a fund has no kept count: each computation of its weight reads that
    there is none and probes the end of both maps, and nothing is written."""
    if rows == 0 and queued == 0:
        return _HOTKEY_CLAIM_COUNT_READS * 3 * _ROCKSDB_READ_PS
    count = rows + queued + 2
    return (
        _HOTKEY_CLAIM_COUNTS * count + _HOTKEY_CLAIM_COUNT_READS
    ) * _ROCKSDB_READ_PS + _ROCKSDB_WRITE_PS


def root_claim_hotkey_declared_ref_time(rows: int = 0, queued: int = 0, new_rows: int = 0) -> int:
    """Declared call weight (ref_time) of ``claim_root_with_hotkey`` since runtime 495.

    ``rows`` is the number of stored basket rows of the validator's fund, ``queued`` its
    queued dividend credits, and ``new_rows`` the queued credits on subnets the fund does
    not hold. The declaration is the weight bound for that fund, never below the smallest
    declaration (four claim units) and never above the ceiling (258 claim units). A fund
    above the ceiling declares the ceiling, and the chain refuses its claim. Each
    declaration includes the same debt settlement bound: the claim settles the claimant's
    deferred-dividend debts first, and the fund does not bound them.
    """
    ceiling_rows = _MAX_ROOT_CLAIM_HOTKEY_WORK - 1
    units = 1 + rows + new_rows
    if units > _MAX_ROOT_CLAIM_HOTKEY_WORK:
        return (
            _hotkey_claim_bound_ref_time(_MAX_ROOT_CLAIM_HOTKEY_WORK, ceiling_rows)
            + _hotkey_claim_sizing_ref_time(ceiling_rows, ceiling_rows)
            + _DEBT_SETTLEMENT_BOUND_REF_TIME
        )
    floor_units = _MIN_ROOT_CLAIM_HOTKEY_WORK
    floor = (
        _claim_root_ref_time(2 * floor_units)
        + _claim_root_scan_ref_time(floor_units)
        + _flush_quotes_ref_time(6 * floor_units)
        + _hotkey_claim_sizing_ref_time(floor_units, floor_units)
    )
    sized = _hotkey_claim_bound_ref_time(units, queued) + _hotkey_claim_sizing_ref_time(
        rows, queued
    )
    return max(sized, floor) + _DEBT_SETTLEMENT_BOUND_REF_TIME


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


def _single_hotkey_claim_counts_queued_rows(spec_version: Optional[int]) -> bool:
    """Whether a single-hotkey claim on a runtime with ``spec_version`` has the runtime
    495 rules: a limit of 258 claim units, with one unit for each queued dividend credit
    on a subnet the fund does not hold. ``None`` means the newest known runtime."""
    return spec_version is None or spec_version >= _FUND_SIZED_ROOT_CLAIM_SPEC


def root_claim_declared_work(
    hotkeys: Optional[list[str]], spec_version: Optional[int] = None
) -> int:
    """Admission envelope for this claim path on a runtime with ``spec_version``
    (``None``: the newest known runtime)."""
    if hotkeys is None:
        return _MAX_ROOT_CLAIM_WORK
    if _single_hotkey_claim_counts_queued_rows(spec_version):
        return _MAX_ROOT_CLAIM_HOTKEY_WORK
    return _MAX_ROOT_CLAIM_HOTKEY_WORK_BEFORE_495


def _fee_units(limit: int) -> int:
    """Claim units the fee wrapper charges for a call admitted under ``limit``."""
    return min(limit, _ROOT_CLAIM_FEE_ALLOWANCE)


def root_claim_declared_ref_time(limit: int) -> int:
    """Declared call weight (ref_time) of a coldkey-wide claim admitted under ``limit``
    units. A single-hotkey claim declares the weight of its fund instead; see
    :func:`root_claim_hotkey_declared_ref_time`."""
    return _claim_ref_time(limit, _FLUSH_BOUND_QUOTES, _FLUSH_BOUND_ROWS)


def root_claim_charged_ref_time(limit: int) -> int:
    """Weight the fee wrapper charges: the allowance plus one hotkey's flush work for
    that many queued credits and holdings (``4Q + 2H`` quotes, ``Q`` rows)."""
    units = _fee_units(limit)
    return min(
        _claim_ref_time(units, 6 * units, units),
        _ROOT_CLAIM_CAP_459_REF_TIME - _EXTENSION_FOLD_REF_TIME,
    )


def root_claim_fee_discount_ref_time(limit: int) -> int:
    """``FeeWeightDiscount`` the runtime subtracts for one coldkey-wide claim under
    ``limit``. A batch or proxy of claims is discounted by the sum over its inner claims."""
    return max(0, root_claim_declared_ref_time(limit) - root_claim_charged_ref_time(limit))


def root_claim_hotkey_fee_discount_ref_time(
    rows: int = 0, queued: int = 0, new_rows: int = 0
) -> int:
    """``FeeWeightDiscount`` the runtime subtracts for one single-hotkey claim: what the
    fund-sized declaration reserves above the charged allowance. The fee is the same for
    every fund."""
    return max(
        0,
        root_claim_hotkey_declared_ref_time(rows, queued, new_rows)
        - root_claim_charged_ref_time(_MAX_ROOT_CLAIM_HOTKEY_WORK),
    )


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
    queued_new: int = 0,
) -> list[str]:
    work = hotkeys + holdings + queued_new
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
        queued_part = (
            f" + {queued_new} queued dividend {'credit' if queued_new == 1 else 'credits'}"
            " on subnets the fund does not hold"
            if queued_new
            else ""
        )
        reasons.append(
            f"{hotkeys} root {hotkey_label} + {holdings} basket {holding_label}{queued_part}"
            f" = {work}"
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
    #: Per selected hotkey, queued dividend credits on subnets the fund does not hold.
    #: Each can open a basket row while the claim settles the queue, so the
    #: single-validator limit counts it from runtime 495. Empty for a coldkey-wide claim,
    #: whose queue has its own allowance, and on a runtime before 495.
    queued_new_counts: tuple[int, ...] = ()
    #: Runtime ``spec_version`` at the block the fund was read from. It selects the
    #: single-validator limit and the counting rule. ``None`` when not read from a chain.
    spec_version: Optional[int] = None

    @property
    def holdings(self) -> int:
        return sum(self.holding_counts)

    @property
    def queued_new(self) -> int:
        return sum(self.queued_new_counts)

    @property
    def too_heavy(self) -> bool:
        return (
            len(self.hotkeys) + self.holdings + self.queued_new > self.limit
            or self.selection_scans > self.limit
        )

    def blocks(self) -> list[str]:
        return _admission_blocks(
            len(self.hotkeys),
            self.holdings,
            self.limit,
            self.selection_scans,
            self.queued_new,
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
    #: Fund rows the claim leaves unsold as dust (spec 468), summed over the selected
    #: validators. Zero on runtimes without the dust-aware preview.
    dust_rows: int = 0
    #: Estimated value of those skipped slices, left in the fund for the other holders.
    forfeited: Balance = field(default_factory=lambda: Balance.from_rao(0))
    #: Queued dividend credits on subnets the fund does not hold, which the
    #: single-validator admission limit counts (runtime 495).
    queued_new: int = 0

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
            self.hotkeys + self.holdings + self.queued_new > self.admission_limit
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
        if self.dust_rows:
            rows.append(("redeemable", str(self.redeemable)))
            rows.append(
                (
                    "dust",
                    f"{self.dust_rows} rows skipped; ~{self.forfeited} stays in the fund",
                )
            )
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
        if self.dust_rows:
            lines.append(
                f"{self.dust_rows} dust {'row' if self.dust_rows == 1 else 'rows'} skipped: "
                f"redeems {self.redeemable}, ~{self.forfeited} stays in the fund"
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
                    self.queued_new,
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

    Every read here is at one block: the chain head when the check starts. The
    runtime ``spec_version`` in the state of that block selects the rules for a
    single-validator claim. A runtime before 495 admits 130 claim units and counts
    the validator and its stored basket rows. From runtime 495 the limit is 258
    claim units, and each queued dividend credit on a subnet the fund does not hold
    is one more unit.
    """
    block_hash = await substrate.block_hash()
    spec_version = int(await substrate.spec_version(block_hash))
    if hotkeys is None:
        raw_keys = await substrate.query(
            *st.SubtensorModule.StakingHotkeys, [claimant_address], block_hash
        )
        raw_keys = tuple(str(key) for key in (raw_keys or []))
        stake_rows = await substrate.runtime_call(
            *StakeInfoRuntimeApi.get_stake_info_for_coldkey,
            [claimant_address],
            block_hash,
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
                    block_hash,
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

    async def holding_netuids(hotkey: str) -> list[int]:
        async with semaphore:
            rows = await substrate.runtime_call(
                *BetaBasketRuntimeApi.get_validator_basket,
                [hotkey],
                block_hash,
            )
        if rows is None:
            raise RuntimeError(f"validator basket is unavailable for {hotkey}")
        return [int(row[0]) for row in rows]

    holdings = await asyncio.gather(*(holding_netuids(hotkey) for hotkey in selected))

    queued_new_counts: tuple[int, ...] = ()
    if hotkeys is not None and _single_hotkey_claim_counts_queued_rows(spec_version):
        # From runtime 495 the single-validator limit also counts the rows that queued
        # dividend credits can open: one for each credit on a subnet the fund does not
        # hold. An earlier runtime does not count them.
        queued = await substrate.query_map(
            *st.SubtensorModule.PendingBasketDeposits, [selected[0]], block_hash
        )
        held = set(holdings[0])
        queued_new_counts = (sum(1 for netuid, _alpha in queued or [] if int(netuid) not in held),)

    networks = await _existing_network_count(substrate, block_hash)
    return RootClaimAdmission(
        hotkeys=selected,
        holding_counts=tuple(len(netuids) for netuids in holdings),
        networks=networks,
        limit=root_claim_declared_work(hotkeys, spec_version),
        selection_scans=selection_scans,
        queued_new_counts=queued_new_counts,
        spec_version=spec_version,
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

    holding_counts = list(admission.holding_counts)
    previews = await _claim_previews(
        substrate, claimant_address, selected_hotkeys, coldkey_wide=coldkey_wide
    )
    if previews is not None:
        # Spec 468 runtime: the dust rules applied exactly as the claim applies them.
        # `payouts` is what the claim pays (and what the threshold applies to); the full
        # entitlement is reported separately as accrued.
        payouts: list[Optional[int]] = [p.redeemable if p is not None else None for p in previews]
        accrued_rao = sum(p.accrued for p in previews if p is not None)
        sell_counts = [
            p.rows_to_sell if p is not None else count for p, count in zip(previews, holding_counts)
        ]
        # The claim first consolidates the fund's sub-threshold rows into root cash — one
        # swap each, charged as full claim units whether or not the redemption then goes
        # ahead — and scans the rows that remain after that preparation.
        swept_counts = [p.swept if p is not None else 0 for p in previews]
        scan_counts = [
            p.rows if p is not None else count for p, count in zip(previews, holding_counts)
        ]
        dust_rows = sum(p.dust_rows for p in previews if p is not None)
        forfeited_rao = sum(p.forfeited for p in previews if p is not None)
    elif coldkey_wide:
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
        payouts = [by_hotkey.get(hotkey) for hotkey in selected_hotkeys]
        accrued_rao = sum(payout for payout in payouts if payout is not None)
        sell_counts = holding_counts
        swept_counts = [0 for _ in holding_counts]
        scan_counts = holding_counts
        dust_rows = 0
        forfeited_rao = 0
    else:
        payout = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_basket_payout,
            [selected_hotkeys[0], claimant_address],
        )
        if payout is None:
            return None
        payouts = [int(payout)]
        accrued_rao = payouts[0]
        sell_counts = holding_counts
        swept_counts = [0 for _ in holding_counts]
        scan_counts = holding_counts
        dust_rows = 0
        forfeited_rao = 0

    threshold_rao = await _threshold_rao(substrate)

    # The runtime is a no-op for a payout of zero (every row dust) as well as for one
    # below the threshold; neither burns anything.
    eligible = [payout is not None and payout > 0 and payout >= threshold_rao for payout in payouts]
    below_threshold = [
        payout is not None and not can_redeem for payout, can_redeem in zip(payouts, eligible)
    ]
    # Runtime: `claim_root(max(hotkeys, realized + swept, ..)) + claim_root_scan(rows −
    # realized)`. Consolidation runs before the threshold check, so its swaps count even
    # for a fund whose redemption is then a no-op; rows sold count only when it goes ahead.
    redeem_holdings = sum(swept_counts) + sum(
        sold for sold, can_redeem in zip(sell_counts, eligible) if can_redeem
    )
    # Every row left after preparation is scanned; the ones not sold (skipped dust, or a
    # whole fund below the threshold) are charged at the per-row scan cost.
    scan_holdings = sum(
        count - sold
        for count, sold, can_redeem in zip(scan_counts, sell_counts, eligible)
        if can_redeem
    ) + sum(count for count, below in zip(scan_counts, below_threshold) if below)
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
        dust_rows=dust_rows,
        forfeited=Balance.from_rao(forfeited_rao),
        queued_new=admission.queued_new,
    )


@dataclass(frozen=True)
class _ClaimPreview:
    """Decoded ``BasketClaimPreview`` for one validator."""

    accrued: int
    redeemable: int
    forfeited: int
    rows: int
    rows_to_sell: int
    dust_rows: int
    swept: int
    flushed_credits: int


def _decode_claim_preview(raw: Any) -> _ClaimPreview:
    return _ClaimPreview(
        accrued=int(raw["accrued_tao"]),
        redeemable=int(raw["redeemable_tao"]),
        forfeited=int(raw["forfeited_tao_est"]),
        rows=int(raw["rows"]),
        rows_to_sell=int(raw["rows_to_sell"]),
        dust_rows=int(raw["dust_rows"]),
        swept=int(raw["swept"]),
        flushed_credits=int(raw["flushed_credits"]),
    )


async def _claim_previews(
    substrate: Any,
    claimant_address: str,
    hotkeys: list[str],
    *,
    coldkey_wide: bool,
) -> Optional[list[Optional[_ClaimPreview]]]:
    """Per selected hotkey, the runtime's dust-aware claim preview (``None`` for a
    validator on which the coldkey has no owed shares), or ``None`` when the runtime
    predates the preview API (spec 468) so the caller falls back to the full-entitlement
    payout views."""
    try:
        if coldkey_wide:
            raw = await substrate.runtime_call(
                *BetaBasketRuntimeApi.get_root_basket_claim_previews, [claimant_address]
            )
            if raw is None:
                return None
            by_hotkey = {str(entry["hotkey"]): _decode_claim_preview(entry) for entry in raw}
            return [by_hotkey.get(hotkey) for hotkey in hotkeys]
        raw = await substrate.runtime_call(
            *BetaBasketRuntimeApi.get_basket_claim_preview, [hotkeys[0], claimant_address]
        )
    except Exception:
        return None
    if raw is None:
        # Either no owed shares on this validator or a pre-468 runtime; the payout view
        # below distinguishes the two (it returns 0 for the former).
        return None
    return [_decode_claim_preview(raw)]


async def _existing_network_count(substrate: Any, block_hash: Optional[str] = None) -> int:
    rows = await substrate.query_map(*st.SubtensorModule.NetworksAdded, None, block_hash)
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
    share-pool state, so it is priced as a full hotkey unit. The flush work and, from
    runtime 495, the counts of the fund and the settlement of the claimant's
    deferred-dividend debts are left out, so ``spent`` is a floor. ``estimate_fee`` prices
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
