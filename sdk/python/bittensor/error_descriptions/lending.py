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
        "The subnet has no valid lending price reference. Wait for a valid reference before "
        "borrowing."
    ),
    "ReferenceWarmingUp": (
        "The dedicated lending EMA is still warming up. Retry after its valid_after block."
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
        "The collateral or loan value is below the minimum. Increase collateral and quote the "
        "complete opening again."
    ),
    "InsufficientReserves": (
        "The separate lending vault cannot fund this loan. Inspect lending_reserves or reduce "
        "its size."
    ),
    "BorrowingLimit": (
        "This loan would exceed the aggregate per-asset borrowing cap. Reduce its size or wait "
        "for repayment."
    ),
    "TooManyPositions": (
        "The subnet has reached its bounded position count. Wait for another position to close."
    ),
    "TooManySubnets": (
        "The lending pallet has reached its funded-subnet limit. This needs governance or "
        "runtime maintenance."
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
        "Remaining collateral cannot cover this operation. Inspect accrued interest "
        "and the complete close quote."
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
    "BelowMinimumProceeds": (
        "The additional short's opening value is below the caller's minimum. Requote the "
        "opening or increase; the failed call moves no assets."
    ),
    "RedemptionUnavailable": (
        "A required conservative funded-redemption bound is unavailable. TAO borrowing also "
        "waits while the actual-stake aggregate is being migrated. Requote when the required "
        "bounds and counters are available."
    ),
    "InsufficientRedemptionBacking": (
        "The loan would exceed conservative funded-redemption coverage after accrued interest. "
        "Alpha debt needs TAO collateral against its funded claim; TAO debt needs backed alpha "
        "collateral. Each other same-side loan must remain individually covered. A zero protected "
        "alpha-claim count with a positive payout pot also refuses alpha borrowing."
    ),
}
