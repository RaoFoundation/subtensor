"""Chain error descriptions declared (first) by the `Derivatives` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "SideDisabled": (
        "Opening a position of this side is switched off by root. Check `shorts_enabled` and "
        "`longs_enabled` in `derivatives-params`; existing positions can always be closed."
    ),
    "SubnetNotDynamic": (
        "The subnet does not exist, is not AMM-priced, has its subtoken disabled, or has an "
        "empty reserve. Check the subnet exists and that `subnets show` reports a live pool."
    ),
    "PositionExists": (
        "The coldkey already holds a position of this side on this subnet; there is one per "
        "coldkey, subnet, and side. Check `derivative-positions` and close or roll the "
        "existing one first."
    ),
    "NoPosition": (
        "No position of this side exists for that owner on that subnet. Check the owner, "
        "netuid, and side against `derivative-positions`; it may already have been closed or "
        "swept at expiry."
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
        "Root submitted parameters with a zero `leverage_percent`, `max_pool_share`, or "
        "`lifetime_blocks`, which would brick opens or make positions closable at once. Check "
        "each field of the submitted `DerivativesParams` is non-zero."
    ),
    "TopUpMismatch": (
        "A roll top-up must be in the token the settled cushion comes back in, and for alpha "
        "on the same hotkey. Check the position's cushion asset and hotkey and match them."
    ),
    "PalletHotkeyUnset": (
        "The pallet has not claimed its custody hotkey yet, so nothing can be opened. Check "
        "`Derivatives.PalletHotkey`; it is set by `on_runtime_upgrade` in the upgrade block."
    ),
}
