#![cfg_attr(not(feature = "std"), no_std, no_main)]

use parity_scale_codec::Compact;

#[derive(Debug, Clone)]
pub struct CustomEnvironment;

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

#[ink::chain_extension(extension = 0x1000)]
pub trait RuntimeReadWrite {
    type ErrorCode = ReadWriteErrorCode;

    #[ink(function = 0)]
    fn get_stake_info_for_hotkey_coldkey_netuid(
        hotkey: ink::primitives::AccountId,
        coldkey: ink::primitives::AccountId,
        netuid: u16,
    ) -> Option<StakeInfo<ink::primitives::AccountId>>;

    #[ink(function = 1)]
    fn add_stake(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    );

    #[ink(function = 2)]
    fn remove_stake(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    );

    #[ink(function = 3)]
    fn unstake_all(hotkey: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 4)]
    fn unstake_all_alpha(hotkey: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 5)]
    fn move_stake(
        origin_hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        destination_hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
    );

    #[ink(function = 6)]
    fn transfer_stake(
        destination_coldkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
    );

    #[ink(function = 7)]
    fn swap_stake(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
    );

    #[ink(function = 8)]
    fn add_stake_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
        limit_price: u64,
        allow_partial: bool,
    );

    #[ink(function = 9)]
    fn remove_stake_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
        limit_price: u64,
        allow_partial: bool,
    );

    #[ink(function = 10)]
    fn swap_stake_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
        limit_price: u64,
        allow_partial: bool,
    );

    #[ink(function = 11)]
    fn remove_stake_full_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        limit_price: Option<u64>,
    );

    #[ink(function = 12)]
    fn set_coldkey_auto_stake_hotkey(
        netuid: u16,
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
    );

    #[ink(function = 13)]
    fn add_proxy(delegate: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 14)]
    fn remove_proxy(delegate: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 15)]
    fn get_alpha_price(netuid: u16) -> u64;

    #[ink(function = 16)]
    fn recycle_alpha(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    ) -> u64;

    #[ink(function = 17)]
    fn burn_alpha(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    ) -> u64;

    #[ink(function = 18)]
    fn add_stake_recycle(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    ) -> u64;

    #[ink(function = 19)]
    fn add_stake_burn(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    ) -> u64;

    #[ink(function = 20)]
    fn caller_add_stake(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    );

    #[ink(function = 21)]
    fn caller_remove_stake(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
    );

    #[ink(function = 22)]
    fn caller_unstake_all(hotkey: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 23)]
    fn caller_unstake_all_alpha(hotkey: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 24)]
    fn caller_move_stake(
        origin_hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        destination_hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
    );

    #[ink(function = 25)]
    fn caller_transfer_stake(
        destination_coldkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
    );

    #[ink(function = 26)]
    fn caller_swap_stake(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
    );

    #[ink(function = 27)]
    fn caller_add_stake_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
        limit_price: u64,
        allow_partial: bool,
    );

    #[ink(function = 28)]
    fn caller_remove_stake_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        amount: u64,
        limit_price: u64,
        allow_partial: bool,
    );

    #[ink(function = 29)]
    fn caller_swap_stake_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        amount: u64,
        limit_price: u64,
        allow_partial: bool,
    );

    #[ink(function = 30)]
    fn caller_remove_stake_full_limit(
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
        limit_price: Option<u64>,
    );

    #[ink(function = 31)]
    fn caller_set_coldkey_auto_stake_hotkey(
        netuid: u16,
        hotkey: <CustomEnvironment as ink::env::Environment>::AccountId,
    );

    #[ink(function = 32)]
    fn caller_add_proxy(delegate: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 33)]
    fn caller_remove_proxy(delegate: <CustomEnvironment as ink::env::Environment>::AccountId);

    #[ink(function = 34)]
    fn get_subnet_registration_state(netuid: u16) -> SubnetRegistrationState;

    #[ink(function = 35)]
    fn get_coldkey_lock(
        coldkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
    ) -> Option<ColdkeyLock>;

    #[ink(function = 36)]
    fn get_stake_availability(
        coldkey: <CustomEnvironment as ink::env::Environment>::AccountId,
        netuid: u16,
    ) -> StakeAvailability;
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
pub enum ReadWriteErrorCode {
    ReadFailed,
    WriteFailed,
}

impl ink::env::chain_extension::FromStatusCode for ReadWriteErrorCode {
    fn from_status_code(status_code: u32) -> Result<(), Self> {
        match status_code {
            0 => Ok(()),
            1 => Err(ReadWriteErrorCode::ReadFailed),
            2 => Err(ReadWriteErrorCode::WriteFailed),
            _ => Err(ReadWriteErrorCode::ReadFailed),
        }
    }
}

impl ink::env::Environment for CustomEnvironment {
    const MAX_EVENT_TOPICS: usize = 4;

    type AccountId = ink::primitives::AccountId;
    type Balance = u64;
    type Hash = ink::primitives::Hash;
    type BlockNumber = u32;
    type Timestamp = u64;

    type ChainExtension = RuntimeReadWrite;
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
pub struct StakeInfo<AccountId> {
    hotkey: AccountId,
    coldkey: AccountId,
    netuid: Compact<u16>,
    stake: Compact<u64>,
    locked: Compact<u64>,
    emission: Compact<u64>,
    tao_emission: Compact<u64>,
    drain: Compact<u64>,
    is_registered: bool,
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
pub struct SubnetRegistrationState {
    netuid: u16,
    exists: bool,
    registered_subnet_counter: u64,
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
pub struct ColdkeyLock {
    locked_mass: u64,
    conviction_bits: u128,
    last_update: u64,
}

#[ink::scale_derive(Encode, Decode, TypeInfo)]
pub struct StakeAvailability {
    netuid: u16,
    total: u64,
    locked: u64,
    available: u64,
}

#[ink::contract(env = crate::CustomEnvironment)]
mod bittensor {
    use super::*;

    /// Defines the storage of your contract.
    /// Add new fields to the below struct in order
    /// to add new static storage fields to your contract.
    #[ink(storage)]
    pub struct Bittensor {}

    impl Bittensor {
        /// Constructor
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {}
        }

        /// Constructor
        #[ink(constructor)]
        pub fn default() -> Self {
            Self::new()
        }

        #[ink(message)]
        pub fn get_stake_info_for_hotkey_coldkey_netuid(
            &self,
            hotkey: [u8; 32],
            coldkey: [u8; 32],
            netuid: u16,
        ) -> Result<Option<StakeInfo<ink::primitives::AccountId>>, ReadWriteErrorCode> {
            self.env()
                .extension()
                .get_stake_info_for_hotkey_coldkey_netuid(hotkey.into(), coldkey.into(), netuid)
                .map_err(|_e| ReadWriteErrorCode::ReadFailed)
        }

        #[ink(message)]
        pub fn add_stake(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .add_stake(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn remove_stake(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .remove_stake(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn unstake_all(&self, hotkey: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .unstake_all(hotkey.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn unstake_all_alpha(&self, hotkey: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .unstake_all_alpha(hotkey.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn move_stake(
            &self,
            origin_hotkey: [u8; 32],
            destination_hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .move_stake(
                    origin_hotkey.into(),
                    destination_hotkey.into(),
                    origin_netuid,
                    destination_netuid,
                    amount,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn transfer_stake(
            &self,
            destination_coldkey: [u8; 32],
            hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .transfer_stake(
                    destination_coldkey.into(),
                    hotkey.into(),
                    origin_netuid,
                    destination_netuid,
                    amount,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn swap_stake(
            &self,
            hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .swap_stake(hotkey.into(), origin_netuid, destination_netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn add_stake_limit(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
            limit_price: u64,
            allow_partial: bool,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .add_stake_limit(hotkey.into(), netuid, amount, limit_price, allow_partial)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn remove_stake_limit(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
            limit_price: u64,
            allow_partial: bool,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env().extension().remove_stake_limit(
                hotkey.into(),
                netuid,
                amount,
                limit_price,
                allow_partial,
            )
        }

        #[ink(message)]
        pub fn swap_stake_limit(
            &self,
            hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
            limit_price: u64,
            allow_partial: bool,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .swap_stake_limit(
                    hotkey.into(),
                    origin_netuid,
                    destination_netuid,
                    amount,
                    limit_price,
                    allow_partial,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn remove_stake_full_limit(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            limit_price: Option<u64>,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .remove_stake_full_limit(hotkey.into(), netuid, limit_price)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn set_coldkey_auto_stake_hotkey(
            &self,
            netuid: u16,
            hotkey: [u8; 32],
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .set_coldkey_auto_stake_hotkey(netuid, hotkey.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn add_proxy(&self, delegate: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .add_proxy(delegate.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn remove_proxy(&self, delegate: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .remove_proxy(delegate.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn get_alpha_price(&self, netuid: u16) -> Result<u64, ReadWriteErrorCode> {
            self.env()
                .extension()
                .get_alpha_price(netuid)
                .map_err(|_e| ReadWriteErrorCode::ReadFailed)
        }

        #[ink(message)]
        pub fn recycle_alpha(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<u64, ReadWriteErrorCode> {
            self.env()
                .extension()
                .recycle_alpha(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn burn_alpha(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<u64, ReadWriteErrorCode> {
            self.env()
                .extension()
                .burn_alpha(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn add_stake_recycle(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<u64, ReadWriteErrorCode> {
            self.env()
                .extension()
                .add_stake_recycle(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn add_stake_burn(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<u64, ReadWriteErrorCode> {
            self.env()
                .extension()
                .add_stake_burn(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_add_stake(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_add_stake(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_remove_stake(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_remove_stake(hotkey.into(), netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_unstake_all(&self, hotkey: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_unstake_all(hotkey.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_unstake_all_alpha(&self, hotkey: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_unstake_all_alpha(hotkey.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_move_stake(
            &self,
            origin_hotkey: [u8; 32],
            destination_hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_move_stake(
                    origin_hotkey.into(),
                    destination_hotkey.into(),
                    origin_netuid,
                    destination_netuid,
                    amount,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_transfer_stake(
            &self,
            destination_coldkey: [u8; 32],
            hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_transfer_stake(
                    destination_coldkey.into(),
                    hotkey.into(),
                    origin_netuid,
                    destination_netuid,
                    amount,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_swap_stake(
            &self,
            hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_swap_stake(hotkey.into(), origin_netuid, destination_netuid, amount)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_add_stake_limit(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
            limit_price: u64,
            allow_partial: bool,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_add_stake_limit(hotkey.into(), netuid, amount, limit_price, allow_partial)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_remove_stake_limit(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            amount: u64,
            limit_price: u64,
            allow_partial: bool,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_remove_stake_limit(
                    hotkey.into(),
                    netuid,
                    amount,
                    limit_price,
                    allow_partial,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_swap_stake_limit(
            &self,
            hotkey: [u8; 32],
            origin_netuid: u16,
            destination_netuid: u16,
            amount: u64,
            limit_price: u64,
            allow_partial: bool,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_swap_stake_limit(
                    hotkey.into(),
                    origin_netuid,
                    destination_netuid,
                    amount,
                    limit_price,
                    allow_partial,
                )
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_remove_stake_full_limit(
            &self,
            hotkey: [u8; 32],
            netuid: u16,
            limit_price: Option<u64>,
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_remove_stake_full_limit(hotkey.into(), netuid, limit_price)
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_set_coldkey_auto_stake_hotkey(
            &self,
            netuid: u16,
            hotkey: [u8; 32],
        ) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_set_coldkey_auto_stake_hotkey(netuid, hotkey.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_add_proxy(&self, delegate: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_add_proxy(delegate.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn caller_remove_proxy(&self, delegate: [u8; 32]) -> Result<(), ReadWriteErrorCode> {
            self.env()
                .extension()
                .caller_remove_proxy(delegate.into())
                .map_err(|_e| ReadWriteErrorCode::WriteFailed)
        }

        #[ink(message)]
        pub fn get_subnet_registration_state(
            &self,
            netuid: u16,
        ) -> Result<SubnetRegistrationState, ReadWriteErrorCode> {
            self.env()
                .extension()
                .get_subnet_registration_state(netuid)
                .map_err(|_e| ReadWriteErrorCode::ReadFailed)
        }

        #[ink(message)]
        pub fn get_coldkey_lock(
            &self,
            coldkey: [u8; 32],
            netuid: u16,
        ) -> Result<Option<ColdkeyLock>, ReadWriteErrorCode> {
            self.env()
                .extension()
                .get_coldkey_lock(coldkey.into(), netuid)
                .map_err(|_e| ReadWriteErrorCode::ReadFailed)
        }

        #[ink(message)]
        pub fn get_stake_availability(
            &self,
            coldkey: [u8; 32],
            netuid: u16,
        ) -> Result<StakeAvailability, ReadWriteErrorCode> {
            self.env()
                .extension()
                .get_stake_availability(coldkey.into(), netuid)
                .map_err(|_e| ReadWriteErrorCode::ReadFailed)
        }
    }
}
