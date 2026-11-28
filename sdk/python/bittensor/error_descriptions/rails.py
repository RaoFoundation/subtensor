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
    "RouteIsFixed": (
        "The bridge exists, and its route (chain, transport domain, portal and stablecoin) "
        "cannot change: what the bridge has outstanding is in the vault of its portal. "
        "`Tusd.set_bridge` changes only the mint cap and the mint window of an existing "
        "bridge. A new portal is a new bridge, with a new id."
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
        "messages from the outbox. This call is not for accounts. An exit, and a payment into "
        "a bridge's budget, also fail with this while no gateway account is set: there is "
        "nobody to pay."
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
        "The destination is the asset the payment is made with, the zero address, or one that "
        "cannot be paid for this way. An exit from Bittensor delivers the stablecoin, "
        "canonical TAO or an alpha token on the spoke."
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
        "This bridge's queue has no place left (`Rails.MaxOutbox`): its messages in line "
        "and its order slots that are out take all of them. Anyone can call the gateway's "
        "`flush` for the bridge: each message then leaves, or is deferred if nobody pays "
        "what the transport charges for it. Deferred messages take no place in the queue."
    ),
    "NotOutboxHead": (
        "The message is not the head of its bridge's queue (`Rails.NextOutbound`). The "
        "gateway takes a bridge's messages from the head, and can also take a deferred "
        "message: one below the head that is still in `Rails.Outbox`."
    ),
    "MessagesNotFunded": (
        "The hub's messages to this bridge are not paid for. Either the admin has not set "
        "what the hub sets aside for one (`Rails.MessageReserve`), or the bridge's account "
        "does not hold that reserve for every message that has not left, for every order "
        "slot that is out, and for one more message. Anyone can pay into the account with "
        "`Rails.fund_messages`."
    ),
    "FeeAboveLimit": (
        "The gateway asked the hub for more than the hub pays for the message: more than the "
        "bridge's ceiling (`Rails.MessageCeiling`), or more than its reserve "
        "(`Rails.MessageReserve`) while the bridge's account has nothing to spare. Anyone can "
        "pay into the account (`Rails.fund_messages`), who calls the gateway's `flush` or "
        "`send` can pay the difference, or the admin raises the ceiling. This call is not "
        "for accounts."
    ),
    "CeilingBelowReserve": (
        "The most the hub pays for one message of a bridge (the ceiling) cannot be less than "
        "what it sets aside for one (the reserve). Give a ceiling at or above the reserve."
    ),
    "GatewayNotFunded": (
        "The account named as the gateway does not hold the existential deposit. Send it "
        "that much first: the hub hands the gateway only what it pays on to the transport."
    ),
    "NoSlotsToGrant": (
        "The hub can give this bridge's portal no order slot in a message of its own now. "
        "More than half of the bridge's slots are out (`Rails.OrderSlots`, "
        "`Rails.MaxOrderSlots`): the rest go with the hub's payments. Or the bridge's queue "
        "or account has no room for one more slot: pay into the account "
        "(`Rails.fund_messages`), or have the gateway send what waits."
    ),
    "NotStale": (
        "The message has not been at the head of its bridge's queue for `Rails.StaleAfter` "
        "blocks. Until then only the gateway and the admin defer it. Retry when it has "
        "waited that long."
    ),
}
