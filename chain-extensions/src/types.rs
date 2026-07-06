use codec::{Decode, Encode};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use sp_runtime::{DispatchError, ModuleError};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid};

#[repr(u16)]
#[derive(TryFromPrimitive, IntoPrimitive, Decode, Encode)]
pub enum FunctionId {
    GetStakeInfoForHotkeyColdkeyNetuidV1 = 0,
    AddStakeV1 = 1,
    RemoveStakeV1 = 2,
    UnstakeAllV1 = 3,
    UnstakeAllAlphaV1 = 4,
    MoveStakeV1 = 5,
    TransferStakeV1 = 6,
    SwapStakeV1 = 7,
    AddStakeLimitV1 = 8,
    RemoveStakeLimitV1 = 9,
    SwapStakeLimitV1 = 10,
    RemoveStakeFullLimitV1 = 11,
    SetColdkeyAutoStakeHotkeyV1 = 12,
    AddProxyV1 = 13,
    RemoveProxyV1 = 14,
    GetAlphaPriceV1 = 15,
    RecycleAlphaV1 = 16,
    BurnAlphaV1 = 17,
    AddStakeRecycleV1 = 18,
    AddStakeBurnV1 = 19,
    CallerAddStakeV1 = 20,
    CallerRemoveStakeV1 = 21,
    CallerUnstakeAllV1 = 22,
    CallerUnstakeAllAlphaV1 = 23,
    CallerMoveStakeV1 = 24,
    CallerTransferStakeV1 = 25,
    CallerSwapStakeV1 = 26,
    CallerAddStakeLimitV1 = 27,
    CallerRemoveStakeLimitV1 = 28,
    CallerSwapStakeLimitV1 = 29,
    CallerRemoveStakeFullLimitV1 = 30,
    CallerSetColdkeyAutoStakeHotkeyV1 = 31,
    CallerAddProxyV1 = 32,
    CallerRemoveProxyV1 = 33,
    GetSubnetRegistrationStateV1 = 34,
    GetColdkeyLockV1 = 35,
    GetStakeAvailabilityV1 = 36,
}

#[freeze_struct("5dc33d60abed5c08")]
#[derive(PartialEq, Eq, Copy, Clone, Encode, Decode, Debug, scale_info::TypeInfo)]
pub struct SubnetRegistrationState {
    pub netuid: NetUid,
    pub exists: bool,
    pub registered_subnet_counter: u64,
}

#[freeze_struct("bf4c1e249109618")]
#[derive(PartialEq, Eq, Copy, Clone, Encode, Decode, Debug, scale_info::TypeInfo)]
pub struct ColdkeyLock {
    pub locked_mass: AlphaBalance,
    pub conviction_bits: u128,
    pub last_update: u64,
}

#[freeze_struct("fb12f00479cf6990")]
#[derive(PartialEq, Eq, Copy, Clone, Encode, Decode, Debug, scale_info::TypeInfo)]
pub struct StakeAvailability {
    pub netuid: NetUid,
    pub total: AlphaBalance,
    pub locked: AlphaBalance,
    pub available: AlphaBalance,
}

#[derive(PartialEq, Eq, Copy, Clone, Encode, Decode, Debug)]
#[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
pub enum Output {
    /// Success
    Success = 0,
    /// Unknown error
    RuntimeError = 1,
    /// Not enough balance to stake
    NotEnoughBalanceToStake = 2,
    /// Coldkey is not associated with the hotkey
    NonAssociatedColdKey = 3,
    /// Error withdrawing balance
    BalanceWithdrawalError = 4,
    /// Hotkey is not registered
    NotRegistered = 5,
    /// Not enough stake to withdraw
    NotEnoughStakeToWithdraw = 6,
    /// Transaction rate limit exceeded
    TxRateLimitExceeded = 7,
    /// Slippage is too high for the transaction
    SlippageTooHigh = 8,
    /// Subnet does not exist
    SubnetNotExists = 9,
    /// Hotkey is not registered in subnet
    HotKeyNotRegisteredInSubNet = 10,
    /// Same auto stake hotkey already set
    SameAutoStakeHotkeyAlreadySet = 11,
    /// Insufficient balance
    InsufficientBalance = 12,
    /// Amount is too low
    AmountTooLow = 13,
    /// Insufficient liquidity
    InsufficientLiquidity = 14,
    /// Same netuid
    SameNetuid = 15,
    /// Too many proxies registered
    ProxyTooMany = 16,
    /// Proxy already exists
    ProxyDuplicate = 17,
    /// Cannot add self as proxy
    ProxyNoSelfProxy = 18,
    /// Proxy relationship not found
    ProxyNotFound = 19,
    /// A system account cannot be used in this operation
    CannotUseSystemAccount = 20,
    /// Cannot burn or recycle on root subnet
    CannotBurnOrRecycleOnRootSubnet = 21,
    /// Subtoken is disabled for this subnet
    SubtokenDisabled = 22,
}

impl From<DispatchError> for Output {
    fn from(input: DispatchError) -> Self {
        let error_text = match input {
            DispatchError::Module(ModuleError { message, .. }) => message,
            _ => Some("No module error Info"),
        };
        match error_text {
            Some("NotEnoughBalanceToStake") => Output::NotEnoughBalanceToStake,
            Some("NonAssociatedColdKey") => Output::NonAssociatedColdKey,
            Some("CannotUseSystemAccount") => Output::CannotUseSystemAccount,
            Some("BalanceWithdrawalError") => Output::BalanceWithdrawalError,
            Some("HotKeyNotRegisteredInSubNet") => Output::NotRegistered,
            Some("HotKeyAccountNotExists") => Output::NotRegistered,
            Some("NotEnoughStakeToWithdraw") => Output::NotEnoughStakeToWithdraw,
            Some("TxRateLimitExceeded") => Output::TxRateLimitExceeded,
            Some("SlippageTooHigh") => Output::SlippageTooHigh,
            Some("SubnetNotExists") => Output::SubnetNotExists,
            Some("SameAutoStakeHotkeyAlreadySet") => Output::SameAutoStakeHotkeyAlreadySet,
            Some("InsufficientBalance") => Output::InsufficientBalance,
            Some("AmountTooLow") => Output::AmountTooLow,
            Some("InsufficientLiquidity") => Output::InsufficientLiquidity,
            Some("SameNetuid") => Output::SameNetuid,
            Some("TooMany") => Output::ProxyTooMany,
            Some("Duplicate") => Output::ProxyDuplicate,
            Some("NoSelfProxy") => Output::ProxyNoSelfProxy,
            Some("NotFound") => Output::ProxyNotFound,
            Some("CannotBurnOrRecycleOnRootSubnet") => Output::CannotBurnOrRecycleOnRootSubnet,
            Some("SubtokenDisabled") => Output::SubtokenDisabled,
            _ => Output::RuntimeError,
        }
    }
}

#[cfg(test)]
mod function_id_tests {
    use super::FunctionId;
    use num_enum::TryFromPrimitive;

    #[test]
    fn caller_variants_have_stable_discriminants() {
        assert_eq!(FunctionId::GetAlphaPriceV1 as u16, 15);
        assert_eq!(FunctionId::RecycleAlphaV1 as u16, 16);
        assert_eq!(FunctionId::BurnAlphaV1 as u16, 17);
        assert_eq!(FunctionId::AddStakeRecycleV1 as u16, 18);
        assert_eq!(FunctionId::AddStakeBurnV1 as u16, 19);
        assert_eq!(FunctionId::CallerAddStakeV1 as u16, 20);
        assert_eq!(FunctionId::CallerRemoveStakeV1 as u16, 21);
        assert_eq!(FunctionId::CallerUnstakeAllV1 as u16, 22);
        assert_eq!(FunctionId::CallerUnstakeAllAlphaV1 as u16, 23);
        assert_eq!(FunctionId::CallerMoveStakeV1 as u16, 24);
        assert_eq!(FunctionId::CallerTransferStakeV1 as u16, 25);
        assert_eq!(FunctionId::CallerSwapStakeV1 as u16, 26);
        assert_eq!(FunctionId::CallerAddStakeLimitV1 as u16, 27);
        assert_eq!(FunctionId::CallerRemoveStakeLimitV1 as u16, 28);
        assert_eq!(FunctionId::CallerSwapStakeLimitV1 as u16, 29);
        assert_eq!(FunctionId::CallerRemoveStakeFullLimitV1 as u16, 30);
        assert_eq!(FunctionId::CallerSetColdkeyAutoStakeHotkeyV1 as u16, 31);
        assert_eq!(FunctionId::CallerAddProxyV1 as u16, 32);
        assert_eq!(FunctionId::CallerRemoveProxyV1 as u16, 33);
        assert_eq!(FunctionId::GetSubnetRegistrationStateV1 as u16, 34);
        assert_eq!(FunctionId::GetColdkeyLockV1 as u16, 35);
        assert_eq!(FunctionId::GetStakeAvailabilityV1 as u16, 36);
    }

    #[test]
    fn caller_ids_roundtrip_try_from_primitive() {
        for id in 16u16..=36u16 {
            let v = FunctionId::try_from_primitive(id)
                .unwrap_or_else(|_| panic!("try_from_primitive failed for {id}"));
            assert_eq!(v as u16, id);
        }
    }
}
