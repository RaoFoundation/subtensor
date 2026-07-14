"""Chain error descriptions declared (first) by the `AdminUtils` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "BondsMovingAverageMaxReached": (
        "A subnet owner called `sudo_set_bonds_moving_average` with a value above 975000, the "
        "cap for owner-set values; root is exempt. Lower the `bonds_moving_average` argument or "
        "submit the call as root."
    ),
    "MaxAllowedUIdsLessThanCurrentUIds": (
        "`sudo_set_max_allowed_uids` was given a value below the number of neurons already "
        "registered on the subnet. Compare the `max_allowed_uids` argument against "
        "`SubnetworkN` for that netuid."
    ),
    "MaxAllowedUidsGreaterThanDefaultMaxAllowedUids": (
        "`sudo_set_max_allowed_uids` was given a value above the chain-wide ceiling. Compare "
        "the `max_allowed_uids` argument against the `DefaultMaxAllowedUids` storage value."
    ),
    "MaxAllowedUidsLessThanMinAllowedUids": (
        "`sudo_set_max_allowed_uids` was given a value below the subnet's configured minimum. "
        "Compare the `max_allowed_uids` argument against `MinAllowedUids` for that netuid."
    ),
    "MaxValidatorsLargerThanMaxUIds": (
        "`sudo_set_max_allowed_validators` was given a value exceeding the subnet's UID "
        "capacity. Compare the `max_allowed_validators` argument against `MaxAllowedUids` for "
        "that netuid."
    ),
    "MinAllowedUidsGreaterThanCurrentUids": (
        "`sudo_set_min_allowed_uids` was given a value not strictly below the number of neurons "
        "currently registered on the subnet. Compare the `min_allowed_uids` argument against "
        "`SubnetworkN` for that netuid."
    ),
    "MinAllowedUidsGreaterThanMaxAllowedUids": (
        "`sudo_set_min_allowed_uids` was given a value not strictly below the subnet's maximum "
        "UID capacity. Compare the `min_allowed_uids` argument against `MaxAllowedUids` for "
        "that netuid."
    ),
    "NegativeSigmoidSteepness": (
        "A non-root caller (subnet owner) passed a negative value to "
        "`sudo_set_alpha_sigmoid_steepness`; negative steepness values are reserved for the "
        "root origin. Use a non-negative `steepness` or submit the call as root."
    ),
    "NotPermittedOnRootSubnet": (
        "An admin-utils call that only applies to regular subnets (burn half-life, burn "
        "increase multiplier, owner-cut flags, or the subnet emission toggle) was targeted at "
        "the root network. Check that the `netuid` argument is not the root netuid 0."
    ),
    "POWRegistrationDisabled": (
        "`sudo_set_network_pow_registration_allowed` unconditionally fails because "
        "proof-of-work registration is deprecated and its toggle can no longer be changed. "
        "Nothing to check; the call is permanently disabled."
    ),
    "SubnetDoesNotExist": (
        "The admin-utils call targets a netuid with no registered subnet. Verify the `netuid` "
        "argument against `NetworksAdded` (the set of existing subnets) before setting "
        "hyperparameters."
    ),
    "ValueNotInBounds": (
        "An admin-utils argument fell outside its allowed range: `min_burn` must be below "
        "`MinBurnUpperBound` and the subnet's max burn, `max_burn` above `MaxBurnLowerBound` "
        "and the min burn, and `max_epochs_per_block` at least 1. Check the argument against "
        "those bounds."
    ),
}
