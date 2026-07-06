# Crowdloan Pallet

## Overview

A pallet that enables the creation and management of generic crowdloans for transferring funds or executing an arbitrary call.

Users of this pallet can create a crowdloan by providing a deposit, a cap, an end block, and exactly one of a target address or a call.

Users can contribute to a crowdloan by providing funds to the crowdloan they choose to support. The contribution can be withdrawn while the crowdloan is not finalized.

Once the crowdloan is finalized, it follows the single configured finalization route. If a target address was provided, the raised funds are transferred to that account. If a call was provided, the call is dispatched with the creator origin and the current crowdloan ID stored as a temporary item.

Crowdloans cannot be created with both a target address and a call, or with neither. Finalization also checks this invariant before transferring funds or dispatching a call, which protects any invalid legacy or manually-written storage state.

If the crowdloan fails to reach the cap, the creator can decide to refund all contributors and dissolve the crowdloan. The initial deposit will be refunded.

*The call or target address provided when creating the crowdloan is guaranteed to never change. Only the minimum contribution, maximum contribution, end block and cap can be updated from the crowdloan creator.*

## Interface

- `create`: Create a crowdloan that will raise funds up to a maximum cap and if successful, will transfer funds to the target address or dispatch the call (using creator origin). Exactly one of target address or call must be provided; both and neither are invalid. The initial deposit will be transferred to the crowdloan account and will be refunded in case the crowdloan fails to raise the cap. Additionally, the creator will pay for the execution of the call.

- `contribute`: Contribute to an active crowdloan. The contribution will be transferred to the crowdloan account and will be refunded if the crowdloan fails to raise the cap. If the contribution would raise the amount above the cap, the contribution will be set to the amount that is left to be raised.

- `withdraw`: Withdraw a contribution from an active (not yet finalized or dissolved) crowdloan. Only contributions over the deposit can be withdrawn by the creator.

- `refund`: Try to refund all contributors (excluding the creator) up to the limit defined by a runtime parameter *RefundContributorsLimit* (currently set to 5). If the limit is reached, the call will stop and the crowdloan will be marked as partially refunded. It may be needed to dispatch this call multiple times to refund all contributors.

The following functions are only callable by the creator of the crowdloan:

- `finalize`: Finalize a successful crowdloan. Finalization uses exactly one route: it transfers the raised amount to the target address if one was configured, otherwise it dispatches the configured call using the creator origin. Invalid configurations with both or neither fail before side effects.

- `dissolve`: Dissolve a crowdloan. The crowdloan will be removed from the storage. All contributions must have been refunded before the crowdloan can be dissolved (except the creator's one).

- `update_min_contribution`: Update the minimum contribution of a non-finalized crowdloan. If a maximum contribution is configured, the new minimum contribution must not exceed it.

- `update_end`: Update the end block of a non-finalized crowdloan.

- `update_cap`: Update the cap of a non-finalized crowdloan.

- `set_max_contribution`: Set or clear the maximum cumulative contribution allowed per contributor for a non-finalized crowdloan.

## Integration with subnet leasing (from the subtensor pallet)

The `crowdloan` pallet can be used to create a crowdloan that will be used to register a new leased network through a crowdloan using the `register_leased_network` extrinsic from the `subtensor` pallet as a call parameter to the crowdloan pallet `create` extrinsic. A new subnet will be registered paying the lock cost using the crowdloan funds and a proxy will be created for the beneficiary to operate the subnet.

When active, the lease will distribute dividends to the contributors according to their contribution to the crowdloan and the lease can be operated by the beneficiary using the proxy created `SubnetLeaseBeneficiary`.

If the lease is perpetual, the lease will never be terminated and emissions will continue to be distributed to the contributors.

If the lease has an end block, the lease can be terminated when end block has passed and the subnet ownership will be transferred to the beneficiary.

License: Apache-2.0
