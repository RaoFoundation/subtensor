"""Chain error descriptions declared (first) by the `EVM` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "BalanceLow": (
        "The sender's mapped account cannot cover the transaction's value plus maximum gas fee, "
        "detected during validation or when withdrawing the fee. Check the account balance "
        "against `value` plus `gas_limit` times the effective gas price."
    ),
    "CreateOriginNotAllowed": (
        "A CREATE, or a CALL that performs a nested CREATE, was attempted from an EVM address "
        "not permitted to deploy contracts. Check whether the deploying address is in the "
        "chain's allowed-deployers list."
    ),
    "FeeOverflow": (
        "Fee arithmetic overflowed, either multiplying the fee per gas by `gas_limit` or "
        "converting the EVM fee into Substrate balance decimals. Check for absurdly large "
        "`gas_limit` or fee-per-gas values in the transaction."
    ),
    "GasLimitTooHigh": (
        "The transaction's `gas_limit` exceeds the block gas limit configured for the EVM. "
        "Compare the `gas_limit` argument against the chain's block gas limit and lower it."
    ),
    "GasLimitTooLow": (
        "The transaction's `gas_limit` is below the intrinsic gas required, or too small to "
        "cover the weight and proof-size base cost. Raise the `gas_limit` argument, comparing "
        "against an `eth_estimateGas` result."
    ),
    "GasPriceTooLow": (
        "The offered fee cannot satisfy the current base fee: `max_fee_per_gas` is below the "
        "block base fee, the priority fee exceeds the max fee, or the fee inputs are "
        "inconsistent. Check `max_fee_per_gas` and `max_priority_fee_per_gas` against the "
        "chain's base fee."
    ),
    "InvalidChainId": (
        "The EIP-155 chain id encoded in the signed transaction does not match this chain's "
        "configured chain id. Compare the transaction's chain id with the value returned by "
        "`eth_chainId` and re-sign the transaction."
    ),
    "InvalidNonce": (
        "The transaction nonce does not match the sender's current account nonce, being either "
        "too low (already used) or too high. Compare the transaction's `nonce` with the "
        "sender's on-chain nonce from `eth_getTransactionCount`."
    ),
    "NotAllowed": (
        "Contract deployment was blocked because the source address is not in the "
        "`WhitelistedCreators` list while the whitelist check is enabled. Check the deployer "
        "address against `WhitelistedCreators` and the `DisableWhitelistCheck` storage value."
    ),
    "PaymentOverflow": (
        "Arithmetic overflowed while computing the total payment or refund for an EVM "
        "transaction, such as refunding remaining gas at the effective gas price. Check for "
        "extreme gas price or gas limit values in the transaction."
    ),
    "Reentrancy": (
        "EVM execution re-entered the pallet while another EVM execution was already in "
        "progress on the same thread, e.g. a precompile or runtime call dispatching back into "
        "the EVM. Inspect precompiles and runtime code that invoke the EVM from within an EVM "
        "call."
    ),
    "TransactionMustComeFromEOA": (
        "Rejected per EIP-3607: the sender address has contract code deployed, and transactions "
        "must originate from externally owned accounts. Check `eth_getCode` for the `source` "
        "address and sign with a plain EOA key instead."
    ),
    "Undefined": (
        "Catch-all EVM validation error for cases without a dedicated variant, such as a "
        "malformed EIP-7702 authorization list or an unknown validation failure. Inspect the "
        "raw transaction for unsupported fields and check the node logs."
    ),
    "WithdrawFailed": (
        "Withdrawing the transaction fee from the sender's mapped account failed even though "
        "the balance check passed, e.g. due to locks, holds, or existential deposit "
        "constraints. Check the account's locks and its free versus withdrawable balance."
    ),
}
