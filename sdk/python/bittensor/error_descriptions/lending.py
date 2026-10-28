"""Lending failures explain the position, quote or inventory to inspect."""

DESCRIPTIONS: dict[str, str] = {
    "Disabled": (
        "New borrowing, including position increases, is disabled. Governance can enable "
        "borrowing; closes remain available."
    ),
    "SubnetUnavailable": (
        "The subnet is absent, is root, or is dissolving. Check its lifecycle before borrowing "
        "or closing."
    ),
    "ReferenceUnavailable": (
        "The subnet has no lending price reference. Submit warm_reference for the netuid, then "
        "wait until valid_after before borrowing."
    ),
    "ReferenceWarmingUp": (
        "The dedicated lending EMA is still warming up after warm_reference. Retry after its "
        "valid_after block."
    ),
    "PositionExists": (
        "This coldkey's existing subnet loan has a different side or hotkey. Use its saved side "
        "and hotkey to increase it, or repay and close before opening a different position."
    ),
    "PositionMissing": (
        "This coldkey has no open loan on the subnet. Check lending_position and the signing "
        "account."
    ),
    "AmountTooSmall": (
        "The collateral or loan opening value is below the runtime minimum. Borrow at least "
        "0.001 TAO of opening value and quote its collateral requirement."
    ),
    "SideCapacity": (
        "This loan would exceed the short or long side capacity for the subnet. Reduce size or "
        "wait for repayment; inspect lending_reserves for headroom."
    ),
    "DepthBelowThreshold": (
        "Pool depth after the lift would fall below the buy-depth threshold. Reduce principal "
        "or wait for deeper reserves."
    ),
    "DynamicCollateral": (
        "Collateral is below the Karl dynamic valuation requirement at the current LTV tier. "
        "Add collateral or reduce principal; quote lending_raise_quote / lending_open_quote."
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
        "Repayment exceeds the caller's payment ceiling. Requote the full close before "
        "resubmitting."
    ),
    "BelowMinimumRefund": (
        "The remaining collateral refund is below the caller's minimum. Requote after accrued "
        "interest."
    ),
    "InsufficientEscrow": (
        "Remaining escrow or collateral cannot cover this operation. Inspect accrued interest "
        "and the complete close quote."
    ),
    "InsufficientCollateral": (
        "The requested additional debt needs more collateral under V_dyn / LTV checks. Quote "
        "lending_required_collateral for this amount. The failed call moves no assets."
    ),
    "InsufficientSellStake": (
        "Unlocked alpha on the hotkey is below collateral plus the raise sale amount. Stake "
        "more alpha or reduce --sell / collateral."
    ),
    "SlippageExceeded": (
        "A forced trade missed the caller min-out / max-payment limit (open, raise, close, or "
        "repay). Requote and widen slippage or reduce size."
    ),
    "DeadlinePassed": (
        "The extrinsic's valid_until block has passed. Resubmit with a later deadline."
    ),
    "SubnetTooNew": (
        "The subnet has not cleared the new-subnet gate (start call, minimum age, warm_reference "
        "plus G_ema age, then capacity ramp)."
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
        "spot to mean-revert, open the other side, or close/repay an existing position "
        "(exits are never blocked by this check)."
    ),
    "ConflictingSizing": (
        "Provide exactly one of collateral or borrow amount for a raise, not both and not neither."
    ),
    "InvalidLiftParams": (
        "Lift parameters are out of range (for example floor/thin_floor above ceiling). Check "
        "set_lift_params bounds."
    ),
    "PositionSideMismatch": (
        "The existing position's side does not match this call. Close first or use the saved side."
    ),
    "HotkeyMismatch": (
        "The existing position's hotkey does not match this call. Close first or reuse the "
        "saved hotkey."
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
