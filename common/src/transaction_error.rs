//! Custom `InvalidTransaction::Custom(u8)` codes shared across Subtensor signed extensions.
//!
//! The `u8` values in [`From<CustomTransactionError> for u8`] are part of the client-facing
//! validity surface — do not renumber existing variants. Variant order in the enum is not the
//! wire encoding; the explicit match arms are.

use sp_runtime::transaction_validity::{InvalidTransaction, TransactionValidityError};

/// Custom transaction-validity error codes returned by Subtensor signed extensions / checks.
///
/// Converted to `InvalidTransaction::Custom(u8)` via [`From`]. Prefer matching on this enum
/// in runtime code; explorers and SDKs decode the raw `u8`.
#[derive(Debug, PartialEq)]
pub enum CustomTransactionError {
    /// Deprecated: coldkey swap now uses announcements; check moved to DispatchGuard.
    #[deprecated]
    ColdkeyInSwapSchedule,
    /// Stake amount below the extrinsic minimum.
    StakeAmountTooLow,
    /// Free balance too low for the requested operation.
    BalanceTooLow,
    /// Target subnet (netuid) does not exist.
    SubnetNotExists,
    /// Hotkey account is not known / not registered where required.
    HotkeyAccountDoesntExist,
    /// Stake balance insufficient for withdraw / unstake.
    NotEnoughStakeToWithdraw,
    /// Caller exceeded a rate limit.
    RateLimitExceeded,
    /// AMM / swap pool lacks liquidity for the trade.
    InsufficientLiquidity,
    /// Swap slippage exceeds the caller's limit.
    SlippageTooHigh,
    /// Transfer path is disabled for this account or subnet.
    TransferDisallowed,
    /// Hotkey is not registered on the target subnet.
    HotKeyNotRegisteredInNetwork,
    /// Axon / serve endpoint IP is invalid.
    InvalidIpAddress,
    /// Axon serve rate limit exceeded.
    ServingRateLimitExceeded,
    /// Axon / serve endpoint port is invalid.
    InvalidPort,
    /// Generic malformed request (maps to custom code `255`).
    BadRequest,
    /// `max_amount` / similar bound was zero when a positive limit is required.
    ZeroMaxAmount,
    /// Commit-reveal round is invalid for the current window.
    InvalidRevealRound,
    /// Expected commit was not found in storage.
    CommitNotFound,
    /// Commit block is outside the allowed reveal range.
    CommitBlockNotInRevealRange,
    /// Parallel input vectors have unequal lengths.
    InputLengthsUnequal,
    /// Neuron uid not found on the subnet.
    UidNotFound,
    /// EVM↔coldkey association rate limit exceeded.
    EvmKeyAssociateRateLimitExceeded,
    /// Coldkey swap is blocked by an active dispute.
    ColdkeySwapDisputed,
    /// Proxy / nested origin real account is invalid.
    InvalidRealAccount,
    /// Shielded transaction bytes failed to parse.
    FailedShieldedTxParsing,
    /// Shielded transaction public-key hash is invalid.
    InvalidShieldedTxPubKeyHash,
    /// Coldkey is not associated with the required hotkey / EVM key.
    NonAssociatedColdKey,
    /// Delegate take is below the allowed minimum.
    DelegateTakeTooLow,
    /// Delegate take is above the allowed maximum.
    DelegateTakeTooHigh,
}

impl From<CustomTransactionError> for u8 {
    fn from(variant: CustomTransactionError) -> u8 {
        match variant {
            #[allow(deprecated)]
            CustomTransactionError::ColdkeyInSwapSchedule => 0,
            CustomTransactionError::StakeAmountTooLow => 1,
            CustomTransactionError::BalanceTooLow => 2,
            CustomTransactionError::SubnetNotExists => 3,
            CustomTransactionError::HotkeyAccountDoesntExist => 4,
            CustomTransactionError::NotEnoughStakeToWithdraw => 5,
            CustomTransactionError::RateLimitExceeded => 6,
            CustomTransactionError::InsufficientLiquidity => 7,
            CustomTransactionError::SlippageTooHigh => 8,
            CustomTransactionError::TransferDisallowed => 9,
            CustomTransactionError::HotKeyNotRegisteredInNetwork => 10,
            CustomTransactionError::InvalidIpAddress => 11,
            CustomTransactionError::ServingRateLimitExceeded => 12,
            CustomTransactionError::InvalidPort => 13,
            CustomTransactionError::BadRequest => 255,
            CustomTransactionError::ZeroMaxAmount => 14,
            CustomTransactionError::InvalidRevealRound => 15,
            CustomTransactionError::CommitNotFound => 16,
            CustomTransactionError::CommitBlockNotInRevealRange => 17,
            CustomTransactionError::InputLengthsUnequal => 18,
            CustomTransactionError::UidNotFound => 19,
            CustomTransactionError::EvmKeyAssociateRateLimitExceeded => 20,
            CustomTransactionError::ColdkeySwapDisputed => 21,
            CustomTransactionError::InvalidRealAccount => 22,
            CustomTransactionError::FailedShieldedTxParsing => 23,
            CustomTransactionError::InvalidShieldedTxPubKeyHash => 24,
            CustomTransactionError::NonAssociatedColdKey => 25,
            CustomTransactionError::DelegateTakeTooLow => 26,
            CustomTransactionError::DelegateTakeTooHigh => 27,
        }
    }
}

impl From<CustomTransactionError> for TransactionValidityError {
    fn from(variant: CustomTransactionError) -> Self {
        TransactionValidityError::Invalid(InvalidTransaction::Custom(variant.into()))
    }
}
