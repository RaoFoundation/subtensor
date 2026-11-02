"""Lending failures explain the position, quote or inventory to inspect."""

DESCRIPTIONS: dict[str, str] = {
    "Disabled": (
        "New borrowing, including position increases, is disabled. Governance can enable "
        "borrowing; closes and repays remain available."
    ),
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
        "The loan opening value (or remaining dust after a partial repay) is below the runtime "
        "minimum (~0.001 TAO / Lending MinimumLoanValue), and side capacity is not the binder. "
        "Increase collateral or principal, or close the remainder fully. When the side cap is "
        "what blocks a minimum loan, the call returns SideCapacity instead."
    ),
    "PrincipalZero": (
        "Principal, principal_part, or borrow amount is zero. Pass a positive amount; a young "
        "or disabled subnet reports its gate error before this one."
    ),
    "CollateralZero": (
        "Collateral is zero. Pass a positive collateral amount (alpha for longs, TAO for shorts)."
    ),
    "SideCapacity": (
        "Side-cap headroom cannot fit this borrow (or any MinimumLoanValue-sized borrow) even "
        "at full Cap. This is capacity headroom, not AMM reserve depth — check the quote's "
        "long_cap_headroom / lending_reserves lift book, reduce size, or wait for repayment. "
        "When only the new-subnet Cap ramp is binding, the call returns SideCapacityRamping."
    ),
    "SideCapacityRamping": (
        "Side capacity is binding only because this subnet is still inside the Cap ramp after "
        "the new-subnet gate (LiftParams.new_subnet_ramp; default ~30 days after "
        "new_subnet_min_age). Full Cap would allow the borrow — check live LiftParams, try "
        "again later as the ramp rises, or reduce size to fit current ramped headroom."
    ),
    "DepthBelowThreshold": (
        "Legacy depth refusal. Prefer EmptyPool / PrincipalExceedsAlphaIn / "
        "PrincipalExceedsTaoIn for the specific reserve failure."
    ),
    "EmptyPool": (
        "The subnet AMM has zero TAO or zero alpha in reserve, so a lift cannot be sized. "
        "Wait for liquidity or pick another subnet."
    ),
    "PrincipalExceedsAlphaIn": (
        "Short principal must be strictly less than pool alpha in. Reduce principal or wait "
        "for deeper alpha reserves."
    ),
    "PrincipalExceedsTaoIn": (
        "Long principal must be strictly less than pool TAO in. Reduce principal or wait for "
        "deeper TAO reserves."
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
    "AboveMaximumPayment": (
        "Repayment exceeds the caller's payment ceiling. Requote the close or repay before "
        "resubmitting (stale quote or tighten --max-slippage carefully)."
    ),
    "BelowMinimumRefund": (
        "The remaining collateral refund is below the caller's minimum. Requote after accrued "
        "interest, or lower the refund floor."
    ),
    "InsufficientEscrow": (
        "Escrow plus the slice's collateral cannot cover this repay/close path. Check "
        "lending_close_quote / lending_repay_quote for payment, refund, and shortfall; for an "
        "underwater short, fund free TAO for the top-up or use --repay-from-wallet with alpha "
        "on --hotkey. Accrued interest reduces usable collateral."
    ),
    "InsufficientCollateral": (
        "The requested additional debt needs more collateral under V_dyn / LTV checks. Quote "
        "lending_required_collateral for this amount (or pass more --collateral). The failed "
        "call moves no assets."
    ),
    "InsufficientSellStake": (
        "Unlocked alpha on the hotkey is below collateral plus the raise sale amount. Stake "
        "more alpha or reduce --sell / collateral."
    ),
    "SlippageExceeded": (
        "A forced trade missed the caller min-out / max-payment limit (open, raise, close, or "
        "repay). The quote is likely stale — re-quote and raise --max-slippage, or reduce size. "
        "Do not leave --max-slippage at 0."
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
        "reference and wait LiftParams.settle_ema_blocks before opening."
    ),
    "CustodyHotkey": ("Third-party stake onto the lending custody hotkey is rejected."),
    "EscrowInUse": (
        "This coldkey's derived lending escrow is still assigned to another open loan "
        "(usually after a coldkey swap that kept the original custody account). Close or "
        "wait for that loan to settle before opening a new position on this subnet."
    ),
    "PriceTooFarFromEma": (
        "The post-trade spot would leave the directional EMA band on the exploitable side "
        "(longs above EMA·(1+band), shorts below EMA·(1−band); default band 15%). Wait for "
        "spot to mean-revert, open the other side, reduce size, or close/repay an existing "
        "position (exits are never blocked by this check). Confirm free balance / unlocked "
        "stake before retrying a large collateral quote."
    ),
    "ConflictingSizing": (
        "Provide exactly one of collateral or borrow amount for a raise, not both and not neither."
    ),
    "InvalidLiftParams": (
        "Lift parameters are out of range (for example floor/thin_floor above ceiling). Check "
        "set_lift_params bounds against the live LiftParams storage."
    ),
    "PositionSideMismatch": (
        "The existing position's side does not match this call. Close first or use the saved side."
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
}
