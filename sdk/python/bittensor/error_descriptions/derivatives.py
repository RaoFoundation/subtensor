"""Chain error descriptions declared (first) by the `Derivatives` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "SideDisabled": (
        "Adding to this side is switched off by root, globally or on this subnet. Check "
        "`shorts_enabled` and `longs_enabled` in `deriv params --netuid N`; existing positions "
        "can always be reduced or closed."
    ),
    "SubnetNotDynamic": (
        "The subnet does not exist, is not AMM-priced, has its subtoken disabled, or has an "
        "empty reserve. Check the subnet exists and that `subnets show` reports a live pool."
    ),
    "NoPosition": (
        "That owner has no position on that subnet. Check the owner and netuid against "
        "`derivative-positions`; it may already have been closed or swept at expiry."
    ),
    "Expired": (
        "The position is past `expires_at`, so nothing can be added to it. Check `expired` in "
        "`derivative-positions`; reduce or close it, or wait for the sweep, then add again."
    ),
    "LeverageOutOfRange": (
        "The requested leverage is zero or above the side's maximum. Check "
        "`max_short_leverage_percent` / `max_long_leverage_percent` in `deriv params` "
        "(`100` = 1x) and pass a `--leverage` at or below it."
    ),
    "ExposureTooLarge": (
        "Leverage times the cushion would take the whole matching reserve, so no pool share "
        "can be lifted. Check the deposit against the pool reserves and use a smaller cushion."
    ),
    "ZeroExposure": (
        "Leverage times the cushion rounds to nothing against the pool reserve. Check the "
        "deposit is well above `min_deposit_tao` relative to the pool size."
    ),
    "PoolCapExceeded": (
        "Open positions of this side would together borrow more than `max_pool_share` of the "
        "lent reserve. Check `Footprint` for the netuid and side against the reserve; try a "
        "smaller cushion or wait for other positions to close."
    ),
    "NotExpired": (
        "Only the owner may close a position before `expires_at`. Check the position's "
        "`expires_at` block; anyone may close it after that."
    ),
    "ExpiryQueueFull": (
        "Too many positions already expire in the block this one would land in and the next "
        "few. Check `Expiring` around `now + lifetime_blocks` and retry in a later block."
    ),
    "InvalidParams": (
        "Root submitted parameters with a zero maximum leverage, `max_pool_share`, or "
        "`lifetime_blocks`, "
        "or a subnet override with a zero `max_pool_share`, which would brick adds or make "
        "positions closable at once. Pause a side with its enabled switch instead."
    ),
    "PalletHotkeyUnset": (
        "The pallet has not claimed its custody hotkey yet, so nothing can be added. Check "
        "`Derivatives.PalletHotkey`; it is set by `on_runtime_upgrade` in the upgrade block."
    ),
}
