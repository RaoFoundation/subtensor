"""Chain error descriptions declared (first) by the `Crowdloan` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AlreadyFinalized": (
        "The crowdloan's `finalized` flag is already true, so withdraw, finalize, refund, "
        "dissolve, and the update calls are all rejected. Check the `finalized` field of the "
        "`Crowdloans` entry for the given `crowdloan_id`."
    ),
    "AlreadyFinalizing": (
        "A `finalize` call was made while another finalization is still in progress, i.e. the "
        "dispatched call from a previous finalize has not cleared. Check that the "
        "`CurrentCrowdloanId` storage value is empty before retrying."
    ),
    "BlockDurationTooLong": (
        "The requested `end` block is more than `MaximumBlockDuration` blocks after the current "
        "block. Compare `end` minus the current block number against the `MaximumBlockDuration` "
        "pallet constant when calling `create` or `update_end`."
    ),
    "BlockDurationTooShort": (
        "The requested `end` block is fewer than `MinimumBlockDuration` blocks after the "
        "current block. Compare `end` minus the current block number against the "
        "`MinimumBlockDuration` pallet constant when calling `create` or `update_end`."
    ),
    "CallUnavailable": (
        "During `finalize` the crowdloan's stored call could not be fetched from preimage "
        "storage, so nothing was dispatched. Check that the preimage referenced by the `call` "
        "field of the `Crowdloans` entry still exists in the preimage pallet."
    ),
    "CannotEndInPast": (
        "The `end` block passed to `create` or `update_end` is not after the current block. "
        "Compare the `end` argument against the current block number; it must be strictly "
        "greater."
    ),
    "CapNotRaised": (
        "`finalize` was called before the crowdloan's `raised` amount equals its `cap`. Compare "
        "the `raised` and `cap` fields of the `Crowdloans` entry; contribute the remainder or "
        "lower the cap with `update_cap` before finalizing."
    ),
    "CapRaised": (
        "A contribution was attempted on a crowdloan whose `raised` amount has already reached "
        "its `cap`, so no further contributions are accepted. Compare the `raised` and `cap` "
        "fields of the `Crowdloans` entry for the `crowdloan_id`."
    ),
    "CapTooLow": (
        "On `create` the `cap` is not strictly greater than the initial `deposit`, or on "
        "`update_cap` the new cap is below the amount already raised. Compare the cap argument "
        "against the `deposit` or the `raised` field of the `Crowdloans` entry."
    ),
    "ContributionPeriodEnded": (
        "A contribution was made at or after the crowdloan's `end` block. Compare the `end` "
        "field of the `Crowdloans` entry with the current block number; the creator can extend "
        "it with `update_end` while the crowdloan is not finalized."
    ),
    "ContributionPeriodNotEnded": (
        "The operation requires the crowdloan's contribution period to be over, but the current "
        "block is still before the crowdloan's `end` block. Compare the `end` field of the "
        "`Crowdloans` entry for the `crowdloan_id` with the current block number."
    ),
    "ContributionTooLow": (
        "The `amount` passed to `contribute` is below the crowdloan's configured minimum "
        "contribution. Check the `min_contribution` field of the `Crowdloans` entry for the "
        "`crowdloan_id` and contribute at least that amount."
    ),
    "DepositCannotBeWithdrawn": (
        "The creator called `withdraw` but holds nothing above the initial deposit, which stays "
        "locked until the crowdloan is dissolved. Compare the creator's `Contributions` entry "
        "with the `deposit` field of the `Crowdloans` entry."
    ),
    "DepositTooLow": (
        "The `deposit` argument to `create` is below the pallet's required minimum. Check the "
        "`MinimumDeposit` pallet constant and create the crowdloan with at least that initial "
        "deposit."
    ),
    "InvalidCrowdloanId": (
        "No crowdloan exists in the `Crowdloans` storage map for the given `crowdloan_id`; it "
        "was never created or has been dissolved. Check `Crowdloans` for the id and "
        "`NextCrowdloanId` for the range of ids ever issued."
    ),
    "InvalidFinalizationConfig": (
        "Exactly one of `call` or `target_address` must be set, but both or neither were "
        "provided to `create`, or the stored crowdloan holds an inconsistent pair at `finalize` "
        "time. Check those two fields on the `create` arguments or the `Crowdloans` entry."
    ),
    "InvalidOrigin": (
        "The caller is not the crowdloan's creator, which is required for `finalize`, `refund`, "
        "`dissolve`, and the update calls. Compare the signing account with the `creator` field "
        "of the `Crowdloans` entry for the `crowdloan_id`."
    ),
    "MaxContributionReached": (
        "The contributor's cumulative contribution already equals the crowdloan's "
        "per-contributor cap, so further contributions are rejected. Compare their "
        "`Contributions` entry with the `MaxContributions` value for the `crowdloan_id`."
    ),
    "MaxContributorsReached": (
        "The crowdloan already has the maximum number of distinct contributors, so accounts "
        "without an existing contribution are rejected. Compare the `contributors_count` field "
        "of the `Crowdloans` entry with the `MaxContributors` pallet constant."
    ),
    "MaximumContributionTooLow": (
        "The `new_max_contribution` passed to `set_max_contribution` is below the crowdloan's "
        "`min_contribution` or below the creator's existing contribution. Check both against "
        "the `Crowdloans` entry and the creator's `Contributions` entry."
    ),
    "MinimumContributionTooHigh": (
        "The `new_min_contribution` passed to `update_min_contribution` exceeds the crowdloan's "
        "configured per-contributor maximum. Compare it with the `MaxContributions` entry for "
        "the `crowdloan_id`."
    ),
    "MinimumContributionTooLow": (
        "The minimum contribution given to `create` or `update_min_contribution` is below the "
        "chain-wide floor. Check the `AbsoluteMinimumContribution` pallet constant and raise "
        "the `min_contribution` argument to at least that value."
    ),
    "NoContribution": (
        "The account has no contribution recorded for this crowdloan, so `withdraw` has nothing "
        "to pay out (or `dissolve` finds no creator contribution). Check the `Contributions` "
        "double map under the `crowdloan_id` for the account in question."
    ),
    "NotReadyToDissolve": (
        "`dissolve` was called while outside contributions remain: the crowdloan's `raised` "
        "amount still exceeds the creator's own contribution. Call `refund` until only the "
        "creator's `Contributions` entry remains and equals `raised` in the `Crowdloans` "
        "record."
    ),
    "Underflow": (
        "A checked subtraction underflowed in crowdloan accounting, e.g. `raised` exceeding "
        "`cap` when computing remaining room, or a contributor count decrement, indicating "
        "inconsistent state. Inspect the `Crowdloans` and `Contributions` entries for the "
        "`crowdloan_id`."
    ),
}
