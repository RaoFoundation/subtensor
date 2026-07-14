"""Chain error descriptions declared (first) by the `Balances` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "DeadAccount": (
        "The beneficiary account does not exist and this operation is not allowed to create it. "
        "Check `System.Account` for the destination: it must already hold at least the "
        "existential deposit before the operation runs."
    ),
    "DeltaZero": (
        "The issuance adjustment was called with a delta of zero, which is meaningless. Check "
        "the `delta` argument to `force_adjust_total_issuance` and pass a strictly positive "
        "amount."
    ),
    "ExistentialDeposit": (
        "The amount is too small to create the destination account: its resulting balance would "
        "sit below the existential deposit. Compare the transfer value plus the destination's "
        "current free balance against the `ExistentialDeposit` constant."
    ),
    "ExistingVestingSchedule": (
        "A vesting schedule already exists for the target account and this call cannot add "
        "another. Check the account's `Vesting` storage entry before attempting to set a new "
        "vested transfer or schedule."
    ),
    "Expendability": (
        "The transfer or payment would drop the sender below the existential deposit and kill "
        "the account while keep-alive semantics are required. Compare the sender's free balance "
        "minus the amount and fees against `ExistentialDeposit`, or use `transfer_allow_death` "
        "if reaping is acceptable."
    ),
    "InsufficientBalance": (
        "The caller's spendable balance is below what the operation needs, whether a plain "
        "transfer, a crowdloan deposit or contribution, or a swap. Compare the account's free "
        "balance in `System.Account` (net of holds, freezes, and fees) against the amount "
        "being moved."
    ),
    "IssuanceDeactivated": (
        "Total issuance cannot be adjusted because issuance has already been deactivated. Check "
        "the Balances `InactiveIssuance` state before calling `force_adjust_total_issuance` "
        "again."
    ),
    "LiquidityRestrictions": (
        "The withdrawal is blocked by locks or freezes on the account even though the raw "
        "balance looks sufficient. Check the account's `Locks` and `Freezes` entries and "
        "compare the frozen amount against what would remain after the withdrawal."
    ),
    "TooManyFreezes": (
        "The account already has the maximum number of balance freezes, so a new freeze cannot "
        "be added. Check the account's `Freezes` entry against the `MaxFreezes` constant; an "
        "existing freeze must be thawed first."
    ),
    "TooManyHolds": (
        "The account already carries the maximum number of balance holds, one per hold reason "
        "variant. Check the account's `Holds` entry; an existing hold must be released before a "
        "new reason can place another."
    ),
    "TooManyReserves": (
        "The account already has the maximum number of named reserves. Check the account's "
        "`Reserves` entry against the `MaxReserves` constant; a named reserve must be "
        "unreserved before adding another."
    ),
    "VestingBalance": (
        "The account's balance is locked under a vesting schedule, leaving too little usable "
        "balance to send the requested value. Check the account's `Vesting` schedules and its "
        "lock in `Locks`, and compare the unvested (still locked) amount against what the "
        "transfer needs."
    ),
}
