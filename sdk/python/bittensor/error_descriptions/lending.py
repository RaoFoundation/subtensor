"""Chain error descriptions declared (first) by the `Lending` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AlphaSupplyCap": (
        "Opening or increasing the short would mint alpha past the subnet's 21,000,000 alpha "
        "supply cap. Compare the subnet's alpha issuance plus the requested `q` with the cap "
        "and reduce `q`."
    ),
    "BelowMinNotional": (
        "The position's notional, its alpha size valued at the subnet's TWAP, would be below "
        "`min_notional` in `Lending.LendingParamsStore`. Open a larger position, or close the "
        "position in full instead of leaving a remainder below the minimum."
    ),
    "CloseInsolvent": (
        "Closing the long would not repay its debt: the TAO from selling its alpha plus its "
        "collateral is less than the TAO it lifted from the pool. Add collateral with "
        "`add_long_collateral` before closing."
    ),
    "CoverageNotMet": (
        "The short's collateral and sale proceeds together are below its coverage need, on "
        "an open or increase, or for the part that a partial close leaves. Read the least "
        "collateral from `LendingRuntimeApi.quote_open_short` or `quote_increase_short` "
        "(`min_collateral`) and bring at least that much, or reduce `q`."
    ),
    "InsufficientFunds": (
        "The short's proceeds and collateral together cannot pay for buying back the alpha "
        "being closed. Compare `cost` from `LendingRuntimeApi.quote_close_short` with the "
        "position's `collateral` and proceeds in `Lending.Positions`; add collateral with "
        "`add_collateral` or close a smaller amount."
    ),
    "InvalidAmount": (
        "A size, amount or quoted value is zero, or a close asks for more than the position "
        "holds. Pass a non-zero amount, and for a close no more than the position's alpha in "
        "`Lending.Positions`."
    ),
    "InvalidParams": (
        "A root parameter update (`set_lending_params`, `set_short_params`, "
        "`set_long_params` or `set_dereg_params`) violates a bound or a cross-check between "
        "the parameter groups, so nothing was written. Compare each field with its allowed "
        "range and with the related fields of the live groups from "
        "`LendingRuntimeApi.params`."
    ),
    "LiftCapExceeded": (
        "The long would take this block's total lifted TAO past `lift_cap_bps` of the "
        "subnet's pool. Reduce `lift` or retry in a later block."
    ),
    "LongCoverageNotMet": (
        "The long's collateral is below its coverage need, on an open or increase, or for "
        "the part that a partial close leaves. Bring more collateral or reduce `lift`."
    ),
    "LongOpenLimit": (
        "The long would take this block's net lifted TAO on the subnet past `open_limit_bps` "
        "of its pool. Reduce `lift` or retry in a later block."
    ),
    "LongsDisabled": (
        "Long positions are switched off: `Lending.LongsEnabled` is false, so `open_long` "
        "and `increase_long` are refused. Existing positions can still be closed. Check "
        "`longs_enabled` in `LendingRuntimeApi.params`."
    ),
    "NoPosition": (
        "The coldkey holds no position on this side of the subnet. Check `Lending.Positions` "
        "for (coldkey, netuid, side), or list the coldkey's positions with "
        "`LendingRuntimeApi.positions_of`; when signing through a proxy, the position "
        "belongs to the real account."
    ),
    "OpenImpactTooHigh": (
        "The short would take this block's net minted alpha on the subnet past "
        "`open_limit_bps` of its pool. Reduce `q` or retry in a later block."
    ),
    "OppositeSideOpen": (
        "The coldkey already holds a position on the other side of this subnet, and a "
        "holder may have only one side per subnet. Close the existing position first."
    ),
    "PoolTooShallow": (
        "The subnet's TAO reserve is below `min_pool_tao` in `Lending.LendingParamsStore`, "
        "now or after the trade, so positions cannot be opened or increased there. Compare "
        "`pool_tao` from `LendingRuntimeApi.market` with `min_pool_tao`."
    ),
    "PositionExists": (
        "The coldkey already holds a position on this side of the subnet. Use "
        "`increase_short` or `increase_long` to grow it instead of opening another."
    ),
    "PriceRingMissing": (
        "The subnet has no lending price history (TWAP ring or rank ring), so lending cannot "
        "price a position there; there is no fallback price. Check that the netuid is a live "
        "subnet: `LendingRuntimeApi.market` returns nothing for a subnet without rings."
    ),
    "QuoteMismatch": (
        "The pool moved a different amount than the quote the lending pallet checked its "
        "gates against, so the call was rolled back. This is an internal consistency check; "
        "retry the call."
    ),
    "SlippageExceeded": (
        "The trade's price moved past the caller's bound: an open or increase would pay out "
        "less than `min_tao_out` (or `min_alpha_out` for a long), or a close would cost more "
        "than `max_tao_in`. Re-quote with `LendingRuntimeApi.quote_open_short`, "
        "`quote_increase_short` or `quote_close_short` and resubmit with a bound that "
        "matches the current price."
    ),
    "SubnetNotInEmission": (
        "The subnet is not in the emission set (it currently receives no share of the block "
        "emission), so a short cannot be opened or increased there. Closing a short and "
        "adding collateral stay available. Use a subnet that receives emission, or retry "
        "once this one does."
    ),
    "SwappedOutThisBlock": (
        "A coldkey swap moved away a position this coldkey opened in the current block, so "
        "the coldkey cannot open another position until the next block. Retry in the next "
        "block."
    ),
    "TooManyLongPositions": (
        "The subnet already has `max_long_positions_per_subnet` long positions "
        "(`Lending.LongPositionCount`). Wait for a long to close, or use another subnet."
    ),
    "TooManyPositions": (
        "The coldkey already holds the maximum of 32 lending positions "
        "(`Lending.HolderPositions`). Close a position before opening another."
    ),
}
