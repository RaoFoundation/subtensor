"""Lending failures explain the position, quote or inventory to inspect."""

DESCRIPTIONS: dict[str, str] = {
    "SubnetUnavailable": (
        "The subnet is absent, is root, or is dissolving. Check its lifecycle before borrowing "
        "or closing."
    ),
    "ReferenceUnavailable": (
        "The subnet has no lending price reference. Submit warm_reference for the netuid, then "
        "wait until valid_after before borrowing. Inspect lending_reserves / the reference EMA."
    ),
    "ReferenceWarmingUp": (
        "The dedicated lending EMA is still warming up after warm_reference. Retry after its "
        "valid_after block."
    ),
    "PositionExists": (
        "This coldkey already has a subnet loan (or a coldkey-swap target already holds one). "
        "Use the same side to increase it, or repay and close before opening a different "
        "position. Positions are keyed by (coldkey, netuid) only."
    ),
    "PositionMissing": (
        "This coldkey has no open loan on the subnet. Check lending_position and the signing "
        "account."
    ),
    "AmountTooSmall": (
        "Borrow, repay slice, or remaining dust is below LiftParams.minimum_loan_value "
        "(default 1 τ). Increase collateral or principal, or close the remainder fully. "
        "If the side cap is what blocks a minimum loan, the chain returns SideCapacity instead."
    ),
    "PrincipalZero": (
        "Principal, principal_part, or borrow amount is zero. Pass a positive amount; a young "
        "or disabled subnet reports its gate error before this one."
    ),
    "CollateralZero": (
        "Collateral is zero. Pass a positive collateral amount (alpha for longs, TAO for shorts)."
    ),
    "SideCapacity": (
        "This subnet's lend/borrow side cap has no headroom for this size (even at full Cap). "
        "Reduce the borrow, wait for other loans to repay, or check lending_reserves headroom. "
        "This is capacity, not AMM depth — SideCapacityRamping means only the ramp binds."
    ),
    "SideCapacityRamping": (
        "Side capacity is limited by the new-subnet Cap ramp (not full Cap yet). "
        "Reduce size to fit current headroom, or retry later as the ramp rises."
    ),
    "EmptyPool": (
        "The subnet AMM has zero TAO or zero alpha in reserve, so a lift cannot be sized. "
        "Wait for liquidity or pick another subnet."
    ),
    "PrincipalExceedsAlphaIn": (
        "Short principal exceeds pool capacity (must be strictly less than SubnetAlphaIn). "
        "Reduce --amount below ~pool alpha in, or wait for deeper alpha reserves. This is "
        "not an insufficient-collateral failure."
    ),
    "PrincipalExceedsTaoIn": (
        "Long principal exceeds pool capacity (must be strictly less than SubnetTAO). "
        "Reduce --amount below ~pool TAO in, or wait for deeper TAO reserves. This is not "
        "an insufficient-collateral failure."
    ),
    "CollateralExceedsMax": (
        "Collateral exceeds the caller's max_collateral_alpha (or that max is zero). Raise "
        "the cap or lower collateral."
    ),
    "DynamicCollateral": (
        "Collateral is below the dynamic valuation requirement at the current LTV tier. "
        "Add collateral or reduce principal; re-quote with lending_raise_quote / "
        "lending_open_quote (or btcli lending show)."
    ),
    "TooManyPositions": (
        "The subnet or pallet has reached its bounded position count. Wait for another position "
        "to close."
    ),
    "BelowMinimumBorrow": (
        "Additional principal is below the caller's minimum. Requote; the failed call moves "
        "no assets."
    ),
    "BelowMinimumRefund": (
        "The remaining collateral refund is below the caller's minimum. Requote after accrued "
        "interest, or lower the refund floor."
    ),
    "InsufficientEscrow": (
        "Escrow plus collateral cannot cover this repay/close path (or free TAO cannot cover "
        "an underwater-short top-up). Check lending_close_quote / lending_repay_quote for "
        "payment, leftover refund, and shortfall; fund free TAO for the top-up or use "
        "--from-wallet (alias: --repay-from-wallet). Insolvent positions forfeit at expiry "
        "(buyback through the pool; leftover collateral burned; no refund) once spot is "
        "inside the EMA band. Accrued interest reduces usable collateral."
    ),
    "InsufficientCollateral": (
        "The requested additional debt needs more collateral under V_dyn / LTV checks. Use "
        "lending_required_collateral (or btcli loan open --amount) for the minimum size, then "
        "pass at least that much --collateral, or lower --amount. The failed call moves no "
        "assets."
    ),
    "InsufficientSellStake": (
        "Unlocked alpha on the hotkey is below collateral plus the raise sale amount. Stake "
        "more alpha or reduce --sell / collateral."
    ),
    "SlippageExceeded": (
        "A forced trade missed your min-out / max-payment limit (open, raise, close, or repay). "
        "Re-quote (btcli loan show / the same command), then raise --max-slippage above the "
        "default 1% or reduce size. Zero --max-slippage always fails once the next block moves "
        "the price. The CLI help line shows quoted payment/refund vs your bounds when available."
    ),
    "DeadlinePassed": (
        "The extrinsic's valid_until block has passed. Resubmit with a later deadline "
        "(raise defaults to head+50 when --valid-until is omitted; open/close default to none)."
    ),
    "SubnetTooNew": (
        "Subnet age is below LiftParams.new_subnet_min_age (live value; default ~7 days / "
        "50_400 blocks). Wait until now − NetworkRegisteredAt clears that floor. Owner start "
        "and warm_reference are separate (SubnetNotStarted / Reference* / GemaWarmingUp)."
    ),
    "SubnetNotStarted": (
        "Subtoken trading is not enabled on this subnet — the subnet owner must call start "
        "first. Opens and quotes refuse until SubtokenEnabled is true."
    ),
    "GemaWarmingUp": (
        "Lending G_ema is missing or has not aged one full EMA window after seeding. Warm the "
        "reference and wait LiftParams.total_tao_ema_blocks before opening."
    ),
    "EscrowInUse": (
        "This coldkey's derived lending escrow is still assigned to another open loan "
        "(usually after a coldkey swap that kept the original custody account). Close or "
        "wait for that loan to settle before opening a new position on this subnet."
    ),
    "ConflictingSizing": (
        "Provide collateral_alpha and/or borrow_tao for a raise (at least one). "
        "Both together sizes an exact lower-LTV C+P raise."
    ),
    "PriceTooFarFromEma": (
        "Post-trade spot would leave the directional EMA band on the exploitable side "
        "(longs above EMA·(1+band), shorts below EMA·(1−band); default band 15%). Wait for "
        "spot to mean-revert, open the other side, reduce size, or close/repay an existing "
        "position (manual exits are never blocked). Confirm free balance / unlocked stake "
        "before retrying a large collateral quote."
    ),
    "SettlementPriceOutsideEmaBand": (
        "Permissionless / protocol-forced settle saw realized AMM execution (or per-block "
        "cumulative impact) outside expiry_max_slippage_bps of the lending EMA (default 2%), "
        "or the wider 15% directional spot band on interest-exhaustion forfeit pre-checks. The "
        "claim is deferred and retried. After max_settlement_deferral (default 50_400 blocks / "
        "~7 days) the next expiry pass force-settles at market (skips the per-trade band) but "
        "still respects the per-block ProtocolForcedImpact cap. Increases and rolls stay "
        "frozen while deferred. Close or repay when realized execution is inside the band "
        "(wallet repay is unconstrained)."
    ),
    "SettlementFrozen": (
        "This position has an active settlement deferral (BandDeferralSince). Increases and "
        "rolls are refused until permissionless settle succeeds (inside the realized EMA band, "
        "or force-at-market after max_settlement_deferral). You may still close or repay."
    ),
    "InvalidLiftParams": (
        "Lift parameters are out of range (for example floor/thin_floor above ceiling). Check "
        "set_lift_params bounds against the live LiftParams storage."
    ),
    "PositionSideMismatch": (
        "Existing loan side does not match this call. Close first, or use the saved side."
    ),
    "InvalidSourceHotkey": (
        "Long open / increase / raise requires a source_hotkey (alpha is locked from that "
        "stake). Short open / top-up is TAO-only and must omit source_hotkey (pass None). "
        "Close and repay take a per-call hotkey only when alpha moves to or from the "
        "caller's stake."
    ),
    "InvalidQuote": (
        "A required executable-depth quote or repayment swap is unavailable. Check curve "
        "capacity and retry with a fresh quote."
    ),
    "Arithmetic": (
        "Lending arithmetic could not represent the requested amount safely. Reduce the amount "
        "and report persistent failures."
    ),
    "CustodyUnavailable": (
        "The lending custody hotkey is unavailable. Governance or runtime maintenance must "
        "restore it."
    ),
    "AlreadyDissolving": (
        "Terminal settlement has already started for this subnet. Its frozen settlement must "
        "finish before reuse."
    ),
    "UnsupportedTermsVersion": (
        "This loan's terms_version is not implemented by the running runtime. Positions keep "
        "the rules they opened under; upgrade the node or wait for a runtime that supports "
        "that version before closing, repaying, or settling it."
    ),
    "ExpiryScheduleCongested": (
        "Too many distinct loan expiry deadlines sit ahead of this open or roll for a "
        "fixed-weight insert. Wait for earlier loans to expire, or use a later term so the "
        "new deadline appends instead of walking mid-list."
    ),
}
