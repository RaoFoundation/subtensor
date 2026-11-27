"""Canonical rails diagnostics: tUSD, the tUSD/TAO pool, and the hub that settles bridge orders."""

DESCRIPTIONS: dict[str, str] = {
    # ── Tusd ────────────────────────────────────────────────────────────
    "BelowMinimumBalance": (
        "The transfer would leave an account holding some tUSD but less than the minimum "
        "balance (`Tusd.MinBalance`, 0.01 tUSD). Send the whole balance, or leave at least the "
        "minimum on both sides."
    ),
    "BridgeUnknown": (
        "No bridge has this id. List the bridges with `btcli bridge tokens` or read `Tusd.Bridges`."
    ),
    "BridgeDisabled": (
        "The bridge is unknown or disabled: it mints nothing, releases nothing and takes no "
        "order until the admin origin enables it. Check `Tusd.Bridges` for its `enabled` flag."
    ),
    "MintCapExceeded": (
        "The mint would put the bridge over its cap on tUSD outstanding. Check "
        "`Tusd.BridgeOutstanding` against the bridge's `mint_cap`; a smaller deposit may fit."
    ),
    "MintRateLimited": (
        "The bridge has minted as much tUSD as its window allows for now. The window refills "
        "evenly over its period; retry later or deposit less."
    ),
    "RedeemExceedsOutstanding": (
        "The exit asks the bridge to release more of the stablecoin than it brought in. A bridge "
        "releases only against its own `Tusd.BridgeOutstanding`; exit less, or through another "
        "bridge."
    ),
    "PortalAlreadyRegistered": (
        "Another bridge already uses this portal address on this transport domain. Each portal "
        "belongs to one bridge; check `Tusd.BridgeByPortal`."
    ),
    # ── CanonicalPool ───────────────────────────────────────────────────
    "PoolDisabled": (
        "The tUSD/TAO pool is disabled: swaps and deposits wait until the admin origin enables "
        "it. Withdrawals stay open. Check `btcli pool show`."
    ),
    "NoLiquidity": (
        "The tUSD/TAO pool holds no liquidity yet, so it cannot swap. Someone must make the "
        "first deposit with `btcli pool add`."
    ),
    "SlippageExceeded": (
        "The result is worse than the minimum the caller set: fewer shares, less of an asset, "
        "or less of an order's destination. Quote again and resubmit, or raise the tolerance."
    ),
    "SwapTooLarge": (
        "The swap pays in more than the pool allows in one swap, a fraction of the reserve "
        "(`CanonicalPool.Caps.per_swap`). Split the amount into smaller swaps."
    ),
    "SwapRateLimited": (
        "Swaps in this direction have used up the pool's window "
        "(`CanonicalPool.Caps.per_window`). It refills evenly over the window; retry later."
    ),
    "ReserveTooLow": (
        "The operation would leave one of the pool's reserves below the minimum reserve "
        "(`CanonicalPool.MinReserve`). Swap or withdraw less."
    ),
    "InsufficientShares": (
        "The account does not hold that many shares of the pool. Check `btcli pool show` for "
        "its shares."
    ),
    "OracleUnavailable": (
        "The first deposit into the pool is checked against the TAO/USD price, and there is no "
        "fresh price. Check `tao_usd_price` and retry once the oracle has fed."
    ),
    "InitialPriceOutOfRange": (
        "The first deposit sets a price too far from the TAO/USD oracle price "
        "(`CanonicalPool.MaxInitialPriceDeviation`). Deposit TAO and tUSD in the ratio of the "
        "oracle price."
    ),
    "FeeOutOfRange": (
        "The swap fee is outside the allowed range, 0.05% to 1%. Set a fee within it."
    ),
    "InvalidCaps": (
        "The swap caps name a fraction of zero or a window of zero blocks. Every cap must be "
        "positive."
    ),
    # ── Rails ───────────────────────────────────────────────────────────
    "NotGateway": (
        "Only the gateway contract's account (`Rails.Gateway`) delivers orders and takes "
        "messages from the outbox. This call is not for accounts."
    ),
    "Paused": (
        "The hub of the rails is paused: it takes no order and no exit until the admin origin "
        "resumes it. Check `Rails.Paused`."
    ),
    "UnknownPortal": (
        "No bridge has this portal on this transport domain, so the hub does not take its "
        "orders. Check `Tusd.BridgeByPortal`."
    ),
    "OrderOutOfSequence": (
        "The order is not the next of its bridge: it was settled before, or an earlier order "
        "has not arrived. Check `Rails.NextOrder` for the bridge."
    ),
    "InvalidRoute": (
        "The destination is the asset the payment is made with, or one that cannot be paid for "
        "this way. An exit from Bittensor delivers the stablecoin, canonical TAO or an alpha "
        "token on the spoke."
    ),
    "Expired": (
        "The order's deadline passed before the hub could settle it, so it was refunded. "
        "Resubmit with a later deadline."
    ),
    "Unbacked": (
        "The spoke reports a burn of more canonical tokens than the hub let it mint. The hub "
        "releases nothing for it. Check `Rails.TaoSupply` or `Rails.AlphaSupply`."
    ),
    "TokenNotRegistered": (
        "The destination token is not in the registry for this spoke. List what a spoke has "
        "with `btcli bridge tokens`."
    ),
    "TaoCapExceeded": (
        "The spoke would hold more canonical TAO than its cap (`Rails.TaoCap`). Buy less, or "
        "wait until the cap is raised."
    ),
    "ReserveDust": (
        "The exit would leave the reserve behind canonical TAO with less than an account can "
        "hold. Unwrap a little less, or the whole supply."
    ),
    "MarketUnknown": (
        "The subnet has no market, so it has no alpha token. List the markets with "
        "`btcli bridge tokens`."
    ),
    "MarketExists": (
        "The subnet already has a market, or its token is already listed on this spoke. A "
        "netuid has one market."
    ),
    "MarketClosed": (
        "The market's subnet is gone. Its token can still be sold, for a share of the TAO its "
        "stake was paid out in, but not bought."
    ),
    "MarketSettling": (
        "The market's subnet is being dissolved and its stake is not paid out yet. Its token "
        "can be sold once the dissolution has finished."
    ),
    "MarketCapExceeded": (
        "The purchase would put the market's vault over its cap of alpha (`Rails.Markets`). "
        "Buy less, or wait until the cap is raised."
    ),
    "SubnetUnknown": (
        "There is no live subnet at this netuid to open a market for. The root network has "
        "no alpha token."
    ),
    "TwapUnavailable": (
        "Credits are valued at the pool's time-weighted price, and the pool does not have a "
        "full period of price history yet. Retry after `CanonicalPool.TwapPeriod` blocks."
    ),
    "OutboxFull": (
        "Too many of the hub's messages wait for the gateway (`Rails.MaxOutbox`). Anyone can "
        "call the gateway's `flush` to hand them to the transport."
    ),
    "NotOutboxHead": (
        "The position is not the head of the hub's outbox. Messages leave first in, first out; "
        "read `Rails.OutboxBounds`."
    ),
}
