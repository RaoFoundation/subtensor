"""Block-pinned positions, lending inventories and runtime loan quotes."""

from __future__ import annotations

import asyncio
from typing import Any, Optional, cast

from .._generated import runtime_apis as api
from .._generated.runtime_apis import Method
from .._generated.storage import Item
from ..balance import Balance
from ..intents._money import alpha_amount, tao_amount
from ..intents.lending import POSITIONS, check_side
from ..result import BittensorError, ChainError, chain_error_from_dispatch
from .base import read

LIFTS = Item("Lending", "Lifts", "LiftBook")
OPEN_BY_NETUID = Item("Lending", "OpenByNetuid")
DISSOLUTIONS = Item("Lending", "Dissolutions", "Dissolution")
MINER_COLLATERAL = Item("SubtensorModule", "MinerCollateral", "MinerCollateralState")
QUOTE_OPEN = Method("LendingRuntimeApi", "quote_open")
QUOTE_OPEN_FOR = Method("LendingRuntimeApi", "quote_open_for")
QUOTE_OPEN_EXACT_FOR = Method("LendingRuntimeApi", "quote_open_exact_for")
QUOTE_CLOSE = Method("LendingRuntimeApi", "quote_close")
QUOTE_RAISE = Method("LendingRuntimeApi", "quote_raise")


def _nonnegative(value: Any, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise BittensorError(f"invalid {label} in stake response")
    return value


async def _unlocked_alpha(view, coldkey_ss58: str, hotkey_ss58: str, netuid: int) -> int:
    """Unlocked alpha on ``hotkey_ss58`` usable as long collateral (rao).

    Mirrors the open-path stake gate: per-hotkey stake, coldkey availability,
    and miner-collateral lock on that hotkey.
    """
    info, availability, bond = await asyncio.gather(
        view.runtime(
            api.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
            [hotkey_ss58, coldkey_ss58, netuid],
        ),
        view.runtime(
            api.StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
            [[coldkey_ss58], [netuid]],
        ),
        view.query(MINER_COLLATERAL, [netuid, hotkey_ss58, coldkey_ss58]),
    )
    if info is not None and not isinstance(info, dict):
        raise BittensorError("wallet alpha stake is unavailable")
    if not isinstance(availability, dict):
        raise BittensorError("wallet alpha availability is unavailable")
    per_owner = availability.get(coldkey_ss58, {})
    if not isinstance(per_owner, dict):
        raise BittensorError("invalid wallet alpha availability")
    entry = per_owner.get(netuid, per_owner.get(str(netuid), {}))
    if not isinstance(entry, dict):
        raise BittensorError("invalid wallet alpha availability")
    stake = _nonnegative(info.get("stake") if info is not None else 0, "alpha stake")
    available = _nonnegative(entry.get("available", 0), "available alpha")
    if bond is not None and not isinstance(bond, dict):
        raise BittensorError("wallet miner collateral is unavailable")
    locked = _nonnegative(bond.get("locked") if bond is not None else 0, "miner collateral")
    return min(available, max(stake - locked, 0))


def _ok(raw: Any) -> dict:
    if not isinstance(raw, dict):
        raise BittensorError("lending quote unavailable; use a node with the lending runtime")
    if "Err" in raw:
        error = raw["Err"]
        if isinstance(error, dict) and "Module" in error and not isinstance(error["Module"], dict):
            raise BittensorError("invalid lending quote error response")
        raise chain_error_from_dispatch(error)
    result = raw.get("Ok", raw)
    if not isinstance(result, dict):
        raise BittensorError("invalid lending quote response")
    return result


async def _interest_clock(view, netuid: int) -> tuple[int, int]:
    year = int(await view.constant(("Lending", "BlocksPerYear")))
    if year <= 0:
        raise BittensorError("invalid lending interest year")
    dissolution = await view.query(DISSOLUTIONS, [netuid])
    now = min(view.block, int(dissolution["frozen_at"])) if dissolution else view.block
    return now, year


def _position_record(
    view, owner: str, netuid: int, raw: Any, clock: tuple[int, int]
) -> Optional[dict]:
    if not isinstance(raw, dict):
        return None
    variant = raw["side"]
    side = check_side(str(next(iter(variant)) if isinstance(variant, dict) else variant))
    collateral_unit = 0 if side == "Short" else netuid
    principal_unit = netuid if side == "Short" else 0
    annual = int(raw["annual_interest"])
    now, blocks_per_year = clock
    accrued_blocks = max(now - int(raw["last_accrued"]), 0)
    interest_due = (
        annual * accrued_blocks + int(raw.get("interest_remainder") or 0)
    ) // blocks_per_year
    collateral = int(raw["collateral"])
    return {
        "coldkey": owner,
        "netuid": netuid,
        "side": side,
        "principal": view.balance(int(raw["principal"]), principal_unit),
        "collateral": view.balance(collateral, collateral_unit),
        "proceeds": Balance.from_rao(int(raw["proceeds"])),
        "escrow_alpha": view.balance(int(raw.get("escrow_alpha") or 0), netuid),
        "lift_tao": view.balance(int(raw.get("lift_tao") or 0), 0),
        "lift_alpha": view.balance(int(raw.get("lift_alpha") or 0), netuid),
        "annual_interest": view.balance(annual, collateral_unit),
        "interest_due": view.balance(interest_due, collateral_unit),
        "last_accrued": int(raw["last_accrued"]),
        "due": int(raw["due"]),
        "runway_days": max(collateral - interest_due, 0) * 365 / annual if annual else None,
    }


@read(
    "lending_position",
    {"coldkey_ss58": "string", "netuid": "integer"},
    category="Prices & swaps",
    param_docs={"coldkey_ss58": "Position owner.", "netuid": "Subnet of the position."},
)
async def lending_position(view, coldkey_ss58: str, netuid: int) -> Optional[dict]:
    """A coldkey's fixed-principal loan on one subnet, or None.

    Principal increases when more is borrowed and never falls when interest is
    collected. This record reports the combined position. Amounts retain their
    currency: short collateral is TAO and short debt is alpha; long collateral
    is alpha and long debt is TAO. The retained ``proceeds`` compatibility field
    is zero for newly opened shorts; it is not a borrowed-alpha sale balance.
    Interest and
    remaining runway are estimates at the selected block, not a close quote.
    """
    view = await view.at()
    raw = await view.query(POSITIONS, [coldkey_ss58, netuid])
    if not isinstance(raw, dict):
        return None
    return _position_record(view, coldkey_ss58, netuid, raw, await _interest_clock(view, netuid))


@read(
    "lending_positions",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet whose open loans to list."},
)
async def lending_positions(view, netuid: int) -> list[dict]:
    """All open loans on a subnet, pinned to one block and sorted by owner."""
    view = await view.at()
    owners = await view.query_map(OPEN_BY_NETUID, [netuid])
    if not owners:
        return []
    clock = await _interest_clock(view, netuid)
    records = []
    for key, _ in owners:
        owner = str(key[0] if isinstance(key, (list, tuple)) else key)
        raw = await view.query(POSITIONS, [owner, netuid])
        record = _position_record(view, owner, netuid, raw, clock)
        if record is not None:
            records.append(record)
    return sorted(records, key=lambda record: record["coldkey"])


@read(
    "lending_reserves",
    {"netuid": "integer"},
    category="Prices & swaps",
    param_docs={"netuid": "Subnet whose lift book to inspect."},
)
async def lending_reserves(view, netuid: int) -> dict:
    """Lift-book balances for a subnet: held legs, escrow, and long collateral.

    Reports custody accounting for lift-based lending, not a separate vault.
    Outstanding short/long lift legs and escrowed proceeds are included.
    """
    raw = await view.query(LIFTS, [netuid])
    raw = raw if isinstance(raw, dict) else {}
    return {
        "netuid": netuid,
        "l_s": view.balance(int(raw.get("l_s") or 0), 0),
        "l_l": view.balance(int(raw.get("l_l") or 0), 0),
        "b_l": view.balance(int(raw.get("b_l") or 0), 0),
        "held_tao": view.balance(int(raw.get("held_tao") or 0), 0),
        "held_alpha": view.balance(int(raw.get("held_alpha") or 0), netuid),
        "escrow_tao": view.balance(int(raw.get("escrow_tao") or 0), 0),
        "escrow_alpha": view.balance(int(raw.get("escrow_alpha") or 0), netuid),
        "long_collateral_alpha": view.balance(int(raw.get("long_collateral_alpha") or 0), netuid),
        "g_ema": view.balance(int(raw.get("g_ema") or 0), 0),
        "last": int(raw.get("last") or 0),
    }


@read(
    "lending_raise_quote",
    {
        "netuid": "integer",
        "coldkey_ss58": "string",
        "hotkey_ss58": "string",
        "borrow_tao": "string",
        "collateral_alpha": "string",
        "sell_alpha": "string",
    },
    category="Prices & swaps",
    param_docs={
        "netuid": "Subnet to raise on.",
        "coldkey_ss58": "Owner of the raise / existing long.",
        "hotkey_ss58": "Hotkey holding collateral and sale alpha.",
        "borrow_tao": "Exact TAO principal; mutually exclusive with collateral_alpha.",
        "collateral_alpha": "Exact alpha collateral; mutually exclusive with borrow_tao.",
        "sell_alpha": "Unlocked alpha to sell; omit to match realised escrow alpha.",
    },
)
async def lending_raise_quote(
    view,
    netuid: int,
    coldkey_ss58: str,
    hotkey_ss58: str,
    borrow_tao: Optional[str] = "",
    collateral_alpha: Optional[str] = "",
    sell_alpha: Optional[str] = "",
) -> dict:
    """Quote an atomic raise: escrowed long sizing plus optional unlocked sale."""
    view = await view.at()
    # Empty-string defaults keep the generated CLI options optional; treat them
    # as unset so callers can pass exactly one of borrow_tao / collateral_alpha.
    borrow_tao = borrow_tao or None
    collateral_alpha = collateral_alpha or None
    sell_alpha = sell_alpha or None
    if (borrow_tao is None) == (collateral_alpha is None):
        raise BittensorError("provide borrow_tao or collateral_alpha, not both")
    amount = None if borrow_tao is None else cast(Balance, tao_amount(borrow_tao)).rao
    collateral = (
        None
        if collateral_alpha is None
        else cast(Balance, alpha_amount(collateral_alpha, netuid)).rao
    )
    sell = None if sell_alpha is None else cast(Balance, alpha_amount(sell_alpha, netuid)).rao
    raw = _ok(
        await view.runtime(
            QUOTE_RAISE,
            [coldkey_ss58, netuid, hotkey_ss58, amount, collateral, sell],
        )
    )
    return {
        "principal_tao": view.balance(int(raw["principal_tao"]), 0),
        "collateral_alpha": view.balance(int(raw["collateral_alpha"]), netuid),
        "escrow_alpha": view.balance(int(raw["escrow_alpha"]), netuid),
        "sell_alpha": view.balance(int(raw["sell_alpha"]), netuid),
        "free_tao_out": view.balance(int(raw["free_tao_out"]), 0),
        "price_before": raw.get("price_before"),
        "price_after_buy": raw.get("price_after_buy"),
        "price_after": raw.get("price_after"),
        "v_dyn": view.balance(int(raw["v_dyn"]), 0),
        "ltv_tier_bps": int(raw["ltv_tier_bps"]),
        "long_cap_headroom": view.balance(int(raw["long_cap_headroom"]), 0),
        "annual_interest_alpha": view.balance(int(raw["annual_interest_alpha"]), netuid),
    }


@read(
    "lending_open_quote",
    {
        "netuid": "integer",
        "side": "string",
        "collateral": "string",
        "coldkey_ss58": "string",
        "hotkey_ss58": "string",
        "amount": "string",
    },
    category="Prices & swaps",
    param_docs={
        "netuid": "Subnet to borrow against.",
        "side": "short or long.",
        "collateral": "Additional collateral amount: TAO for a short, alpha for a long.",
        "coldkey_ss58": "Owner to quote an opening or increase for.",
        "hotkey_ss58": (
            "Long only: collateral source hotkey. Omit for shorts (TAO-only; "
            "source_hotkey is None)."
        ),
        "amount": "Exact additional principal: alpha for a short, TAO for a long; optional.",
    },
)
async def lending_open_quote(
    view,
    netuid: int,
    side: str,
    collateral: str,
    coldkey_ss58: Optional[str] = None,
    hotkey_ss58: Optional[str] = None,
    amount: Optional[str] = "",
) -> dict:
    """Quote additional fixed debt, opening value and collateral coupon.

    Supply ``coldkey_ss58`` to quote a new position or increase an existing
    one on the same side (positions are keyed by coldkey and netuid). Longs
    also need ``hotkey_ss58`` as the collateral source; shorts must omit it
    (TAO-only). The runtime accrues old interest and checks the combined
    position. Alpha loans require conservative funded-claim coverage; TAO
    loans require conservative funded collateral backing. Both protect each
    other same-side loan after accrued interest. Returned principal,
    annual_interest and opening_value are additions, not totals. Short
    opening_value is the loan's TAO value at the lending EMA, rounded up,
    and fixes its annual TAO coupon. Omitting the owner quotes a fresh loan
    only. A refusal is an error; it never becomes a zero-protection quote.
    The result is indicative until the transaction executes. Supply
    ``amount`` with an owner to quote exactly that additional principal.
    """
    view = await view.at()
    side = check_side(side)
    amount = amount or None
    collateral_amount = cast(
        Balance, tao_amount(collateral) if side == "Short" else alpha_amount(collateral, netuid)
    )
    if side == "Short" and hotkey_ss58 is not None:
        raise BittensorError("short open quote is TAO-only; omit hotkey_ss58")
    if side == "Long" and coldkey_ss58 is not None and hotkey_ss58 is None:
        raise BittensorError("a long owner-aware quote requires hotkey_ss58")
    source_hotkey = None if side == "Short" else hotkey_ss58
    method = QUOTE_OPEN if coldkey_ss58 is None else QUOTE_OPEN_FOR
    params = (
        [netuid, side, collateral_amount.rao]
        if coldkey_ss58 is None
        else [coldkey_ss58, netuid, side, collateral_amount.rao, source_hotkey]
    )
    if amount is not None:
        if coldkey_ss58 is None:
            raise BittensorError("an exact lending quote requires coldkey_ss58")
        if side == "Long" and hotkey_ss58 is None:
            raise BittensorError("an exact long quote requires hotkey_ss58")
        principal = cast(
            Balance, alpha_amount(amount, netuid) if side == "Short" else tao_amount(amount)
        )
        if not 0 < principal.rao <= (1 << 64) - 1:
            raise BittensorError("borrow amount must be positive and fit the chain's token units")
        method = QUOTE_OPEN_EXACT_FOR
        params = [
            coldkey_ss58,
            netuid,
            side,
            collateral_amount.rao,
            source_hotkey,
            principal.rao,
        ]
    raw = _ok(await view.runtime(method, params))
    if amount is not None and int(raw["principal"]) != principal.rao:
        raise BittensorError("exact lending quote returned a different borrowed amount")
    collateral_unit = 0 if side == "Short" else netuid
    return {
        "principal": view.balance(int(raw["principal"]), netuid if side == "Short" else 0),
        "annual_interest": view.balance(int(raw["annual_interest"]), collateral_unit),
        "opening_value": Balance.from_rao(int(raw["opening_value"])),
    }


@read(
    "lending_required_collateral",
    {
        "netuid": "integer",
        "side": "string",
        "amount": "string",
        "coldkey_ss58": "string",
        "hotkey_ss58": "string",
    },
    category="Prices & swaps",
    param_docs={
        "netuid": "Subnet to borrow against.",
        "side": "short or long.",
        "amount": "Exact additional principal: alpha for a short, TAO for a long.",
        "coldkey_ss58": "Owner of the new or increased loan.",
        "hotkey_ss58": "Long only: collateral source hotkey. Omit for shorts.",
    },
)
async def lending_required_collateral(
    view,
    netuid: int,
    side: str,
    amount: str,
    coldkey_ss58: str,
    hotkey_ss58: Optional[str] = None,
) -> dict:
    """Minimum additional collateral for exactly the requested borrowed amount.

    The search uses exact-principal runtime quotes at one block. It includes
    existing principal, accrued interest, market depth and funded-redemption
    guards. Only the owner's ``InsufficientCollateral`` response means to
    search higher; unavailable references, inventory, borrowing caps and
    other positions' uncovered debt remain hard refusals. For longs, probes
    are capped by unlocked alpha on ``hotkey_ss58`` so the owner-aware stake
    gate cannot abort an otherwise affordable search. The requested opening
    value must meet the runtime's minimum, currently 0.1 TAO.

    Returns collateral and its opening quote. The exact-principal transaction
    rechecks admission without borrowing extra if state changes.
    """
    view = await view.at()
    side = check_side(side)
    principal = cast(
        Balance, alpha_amount(amount, netuid) if side == "Short" else tao_amount(amount)
    )
    if not 0 < principal.rao <= (1 << 64) - 1:
        raise BittensorError("borrow amount must be positive and fit the chain's token units")
    unit = 0 if side == "Short" else netuid
    maximum = (1 << 64) - 1
    existing = await lending_position(view, coldkey_ss58, netuid)
    if existing is not None:
        remaining = existing["collateral"].rao - existing["interest_due"].rao
        if remaining <= 0:
            raise BittensorError("the existing position's collateral is exhausted")
        maximum -= remaining
    if maximum <= 0:
        raise BittensorError("the position cannot accept additional collateral")

    available: Optional[int] = None
    if side == "Long":
        if hotkey_ss58 is None:
            raise BittensorError("a long required-collateral search needs hotkey_ss58")
        available = await _unlocked_alpha(view, coldkey_ss58, hotkey_ss58, netuid)
        if available <= 0:
            raise BittensorError("no unlocked alpha on the source hotkey for collateral")
        maximum = min(maximum, available)

    quotes: dict[int, Optional[dict]] = {}

    async def quote(collateral: int) -> Optional[dict]:
        if collateral not in quotes:
            try:
                quotes[collateral] = await lending_open_quote(
                    view,
                    netuid,
                    side,
                    view.balance(collateral, unit),
                    coldkey_ss58,
                    hotkey_ss58,
                    principal,
                )
            except ChainError as error:
                if error.name == "InsufficientCollateral":
                    quotes[collateral] = None
                elif error.name == "StakeUnavailable" and available is not None:
                    # Probe above unlocked stake — treat as out of search range.
                    if collateral > available:
                        quotes[collateral] = None
                    else:
                        raise
                else:
                    raise
        return quotes[collateral]

    # Bound the geometric probe by unlocked stake so the first probe cannot
    # overshoot available collateral and abort on StakeUnavailable.
    low, high = 0, min(1_000_000_000, maximum)
    for _ in range(64):
        if high <= 0:
            break
        if await quote(high) is not None:
            break
        if high == maximum:
            if available is not None:
                raise BittensorError(
                    "the required collateral exceeds unlocked alpha on the source hotkey"
                )
            raise BittensorError("the required collateral exceeds the chain's position limit")
        low, high = high, min(high * 2, maximum)
    else:
        raise BittensorError("could not bound the required collateral")

    for _ in range(64):
        if high - low <= 1:
            break
        middle = (low + high) // 2
        if await quote(middle) is None:
            low = middle
        else:
            high = middle
    result = await quote(high)
    if result is None:
        raise BittensorError("the requested loan has no admissible collateral quote")
    return {"collateral": view.balance(high, unit), **result}


@read(
    "lending_close_quote",
    {
        "coldkey_ss58": "string",
        "netuid": "integer",
        "hotkey_ss58": "string",
        "repay_from_wallet": "boolean",
    },
    category="Prices & swaps",
    param_docs={
        "coldkey_ss58": "Position owner.",
        "netuid": "Subnet of the position.",
        "hotkey_ss58": (
            "Optional per-call hotkey for alpha repay (short wallet path) or "
            "alpha refund (long close). Omit for short escrow buyback (TAO "
            "only). When omitted, the runtime skips the hotkey-existence check."
        ),
        "repay_from_wallet": (
            "False (default): quote the escrow AMM buyback for a short. "
            "True: repay short alpha from a call-supplied hotkey stake."
        ),
    },
)
async def lending_close_quote(
    view,
    coldkey_ss58: str,
    netuid: int,
    hotkey_ss58: Optional[str] = None,
    repay_from_wallet: bool = False,
) -> dict:
    """Runtime quote of the combined debt repayment and refund, after accrued interest.

    Payment is TAO except for a wallet-repaid short, which returns its fixed
    alpha principal. Refund is TAO for a short and alpha for a long. Default
    ``repay_from_wallet=False`` quotes the escrow AMM path: shorts buy P alpha
    from escrow+collateral (leftover TAO refund); when the buyback costs more
    than the pot, payment is the TAO needed, refund is 0, and ``max_payment``
    on close must cover the shortfall from free balance. Longs sell escrow A
    then only enough collateral to cover any shortfall to P (leftover alpha
    refund). Set True to quote a wallet repayment instead. Pass
    ``hotkey_ss58`` when alpha is repaid or refunded so quote and call share
    hotkey-existence checks; omit it on TAO-only short escrow buyback.
    """
    view = await view.at()
    position = await lending_position(view, coldkey_ss58, netuid)
    if position is None:
        raise BittensorError(f"no lending position on netuid {netuid}")
    raw = _ok(
        await view.runtime(QUOTE_CLOSE, [coldkey_ss58, netuid, hotkey_ss58, repay_from_wallet])
    )
    short = position["side"] == "Short"
    return {
        "payment": view.balance(int(raw["payment"]), netuid if short and repay_from_wallet else 0),
        "refund": view.balance(int(raw["refund"]), 0 if short else netuid),
    }
