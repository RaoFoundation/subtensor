"""Chain error descriptions declared (first) by the `Derivatives` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "SubnetNotDynamic": (
        "The subnet does not exist, is not AMM-priced, has its subtoken disabled, or has an "
        "empty reserve. Check the subnet exists and that `subnets show` reports a live pool."
    ),
    "NoPosition": (
        "That owner has no position on that subnet. Check the owner and netuid against "
        "`derivative-positions`; it may already have been closed, or forfeited once its "
        "cushion could no longer pay its interest."
    ),
    "LeverageOutOfRange": (
        "The requested leverage is zero or above the side's maximum: 1x for shorts, 1.5x for "
        "longs (`max_short_leverage` / `max_long_leverage` in `deriv params`). Pass a "
        "`--leverage` at or below it."
    ),
    "ZeroExposure": (
        "Leverage times the deposit rounds to nothing against the pool reserve, or the pool "
        "would swap the lifted slice for nothing. Use a larger deposit relative to the pool."
    ),
    "PoolCapExceeded": (
        "Open positions of this side would together borrow more than `pool_share` of the lent "
        "reserve, or the deposit alone would take the whole pool. A `pool_share` of zero means "
        "root has paused new positions. Check `deriv params`; try a smaller deposit or wait "
        "for other positions to close."
    ),
    "PalletHotkeyUnset": (
        "The pallet has not claimed its custody hotkey yet, so nothing can be added. Check "
        "`Derivatives.PalletHotkey`; it is set by `on_runtime_upgrade` in the upgrade block."
    ),
    "ZeroInterestRate": (
        "`sudo_set_params` was given a `short_interest_rate` or `long_interest_rate` of zero. "
        "Both rates must be above zero; to pause new positions set `pool_share` to zero instead."
    ),
    "DerivativesDisabled": (
        "Derivatives are switched off network-wide (`Derivatives.DerivativesEnabled` is false, "
        "shown as `enabled` in `deriv params`), so nothing can be added: no open, grow, reduce, "
        "or flip. Governance turns it on with `Derivatives.sudo_set_derivatives_enabled`. "
        "Existing positions can still be closed with `deriv close`, and the chain keeps "
        "collecting their interest."
    ),
    "LongsDisabled": (
        "Longs are switched off (`Derivatives.LongsEnabled` is false, shown as `longs_enabled` "
        "in `deriv params`): shorts are the launch product, and longs wait on a decision about "
        "their collateral and leverage. No add may open a long, grow one, or flip a short "
        "through zero into one. Shorts work as usual with `deriv short`; a `deriv long` that "
        "only reduces or closes a short goes through; an open long can still be closed with "
        "`deriv close`. Governance turns longs on with `Derivatives.sudo_set_longs_enabled`."
    ),
    "SettlementBelowMinimum": (
        "The settlement would pay you less TAO than the `min_amount_out` floor you set, after "
        "interest, so nothing moved and the position is as it was. Either the pool price moved "
        "against you since the quote (someone may have pushed it in the same block), or the "
        "position is underwater and a close would pay nothing. Re-quote and retry; widen "
        "`--max-slippage` (btcli) or lower `min_amount_out` (SDK) to accept less; a floor of 0 "
        "disables the check. Also raised when an add that only opens or grows a position is "
        "given a floor above zero: such an add pays nothing out, so leave the floor at 0."
    ),
}
