"""Short failures explain the position, quote or subnet book to inspect."""

DESCRIPTIONS: dict[str, str] = {
    "SubnetNotEligible": (
        "The subnet is absent, root, or not trading yet (the owner has not called start). "
        "Shorts need a live dynamic subnet."
    ),
    "BelowMinNotional": (
        "The position's minted alpha times the EMA price is below SynthParams.min_notional "
        "(10 TAO by default). Mint more alpha."
    ),
    "CoverageNotMet": (
        "Collateral plus sale proceeds miss the position's share of buying the whole subnet "
        "book back after a stressed pump, or the pumped pool cannot supply the book at all. "
        "Check lending_open_quote for the required collateral, add collateral, or mint less."
    ),
    "SlippageExceeded": (
        "The sale returned less than min_tao_out (zero is always refused), or the buy-back "
        "would cost more than max_tao_in. Re-quote with lending_open_quote / "
        "lending_close_quote and widen the tolerance, or reduce size."
    ),
    "OpenImpactTooHigh": (
        "This block's net minted alpha on the subnet would exceed SynthParams."
        "open_net_mint_bps of pool alpha (1% by default). Retry next block or mint less."
    ),
    "NoPosition": (
        "This coldkey has no short on the subnet. Check lending_position and the signing account."
    ),
    "PositionDeleveraging": (
        "The short is in a forced close. It cannot grow or withdraw collateral; you may "
        "still add collateral or close it."
    ),
    "WithdrawBreaksCoverage": (
        "After the withdrawal the remaining funds would miss the position's whole-book "
        "coverage. Withdraw less; lending_position shows coverage_cost and funds."
    ),
    "DissolutionInProgress": (
        "The subnet is deregistering. Its shorts settle at the stakers' rate and no short "
        "call is accepted."
    ),
    "InvalidParams": (
        "SynthParams are out of range. Check set_synth_params bounds against the live "
        "lending_params."
    ),
    "CustodyUnavailable": (
        "The lending custody hotkey is unavailable. Governance or runtime maintenance must "
        "restore it."
    ),
    "DilutionBreakerActive": (
        "Residual alpha from defaulted shorts exceeds the breaker threshold (2% of issuance "
        "by default), so new opens and increases are paused until it falls below 1%. Check "
        "lending_subnet."
    ),
    "PoolTooShallow": (
        "Pool TAO is below SynthParams.min_pool_depth. Wait for a deeper pool or pick "
        "another subnet."
    ),
    "WithdrawBufferBreached": (
        "The withdrawal would leave less collateral than SynthParams.withdraw_buffer_days "
        "of interest (7 days by default). Withdraw less."
    ),
    "PositionExists": (
        "This coldkey already shorts the subnet. Grow the position with increase_short."
    ),
    "TooManyPositions": (
        "The subnet or pallet has reached its bounded position count. Wait for another short "
        "to close."
    ),
    "ZeroAmount": "The call moves nothing. Pass a positive amount.",
    "InsufficientFunds": (
        "Custody cannot pay: the buy-back costs more than proceeds plus collateral, or the "
        "withdrawal exceeds the collateral. Close less alpha or add collateral first."
    ),
    "OwnerNotAllowed": (
        "The owner has a pending coldkey swap. Shorts cannot open or grow until it completes."
    ),
}
