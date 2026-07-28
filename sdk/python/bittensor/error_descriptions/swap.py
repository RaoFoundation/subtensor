"""Chain error descriptions declared (first) by the `Swap` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "FeeRateTooHigh": (
        "`set_fee_rate` was called with a rate above the swap pallet's `MaxFeeRate` config "
        "constant. Compare the `rate` argument (u16-scaled fraction) against `MaxFeeRate` "
        "before submitting."
    ),
    "InsufficientInputAmount": (
        "Declared for swap inputs too small to execute, but no current code path raises it "
        "since the user-liquidity code was removed. If seen on an older runtime, check that the "
        "swap input amount is nonzero and large enough to produce output."
    ),
    "InvalidLiquidityValue": (
        "Legacy error from the removed V3 user-liquidity code, raised when an added or removed "
        "liquidity amount was below the pallet's `MinimumLiquidity`. Not raised on current "
        "runtimes; on older ones compare the `liquidity` argument to `MinimumLiquidity`."
    ),
    "InvalidTickRange": (
        "Legacy error from the removed V3 user-liquidity code, raised when `tick_low` was not "
        "below `tick_high` or a tick failed to convert to a sqrt price. Not raised on current "
        "runtimes; check the tick range arguments on older ones."
    ),
    "PriceLimitExceeded": (
        "The `limit_price` given to a swap is not beyond the current pool price in the trade's "
        "direction, so the swap would immediately breach it. Compare the limit price argument "
        "against the subnet's current alpha price before submitting."
    ),
    "ReservesOutOfBalance": (
        "Swap balancer initialization failed because the subnet's TAO and alpha reserves "
        "produce an invalid ratio, for example both reserves are zero when an initial price is "
        "supplied. Inspect the subnet's TAO and alpha reserves and the `SwapBalancer` entry."
    ),
    "ReservesTooLow": (
        "The output-side reserve is below the swap pallet's `MinimumReserve`, or a swap step "
        "produced zero output for a nonzero input. Check the subnet's TAO and alpha reserves "
        "against `MinimumReserve` and reduce the trade size."
    ),
    "SwapInputTooLarge": (
        "The swap's net input after fees exceeds 1000 times the input-side reserve, the "
        "pallet's hard per-trade cap. Compare the input amount against the subnet's input-side "
        "reserve (TAO or alpha) and split the trade if needed."
    ),
}
