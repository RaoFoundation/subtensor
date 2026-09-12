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
        "The requested leverage is zero or above the side's maximum: 1x for shorts, 2x for "
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
}
