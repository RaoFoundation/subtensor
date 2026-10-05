"""Lending failures explain the position, quote or inventory to inspect."""

DESCRIPTIONS: dict[str, str] = {
    "Disabled": (
        "New lending positions are disabled. Governance can enable borrowing; closes remain "
        "available."
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
        "This coldkey already has a loan on the subnet. Repay and close it before opening another."
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
        "Opening principal is below the caller's minimum. Requote; the failed call moves no assets."
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
        "Remaining collateral or locked short TAO cannot cover this operation. Inspect interest "
        "and the complete close quote."
    ),
    "InvalidQuote": (
        "The full swap cannot execute or its result differs from the loan quote. Check curve "
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
