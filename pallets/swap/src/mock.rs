#![allow(clippy::unwrap_used)]

use core::num::NonZeroU64;

use frame_support::pallet_prelude::*;
use frame_support::{
    PalletId, parameter_types,
    traits::{ConstU32, Everything},
};
use frame_support::{construct_runtime, derive_impl};
use frame_system::{self as system};
use sp_core::H256;
use sp_runtime::{
    BuildStorage, Vec,
    traits::{BlakeTwo256, IdentityLookup},
};
use std::{cell::RefCell, collections::HashMap};
use subtensor_runtime_common::{
    AlphaBalance, BalanceOps, NetUid, SubnetInfo, TaoBalance, TokenReserve,
};
use subtensor_swap_interface::Order;

construct_runtime!(
    pub enum Test {
        System: frame_system = 0,
        Swap: crate::pallet = 1,
    }
);

pub type Block = frame_system::mocking::MockBlock<Test>;
pub type AccountId = u32;
pub const OK_COLDKEY_ACCOUNT_ID: AccountId = 1;
pub const OK_HOTKEY_ACCOUNT_ID: AccountId = 1000;
pub const OK_COLDKEY_ACCOUNT_ID_2: AccountId = 2;
pub const OK_HOTKEY_ACCOUNT_ID_2: AccountId = 1001;
pub const OK_COLDKEY_ACCOUNT_ID_RICH: AccountId = 5;
pub const OK_HOTKEY_ACCOUNT_ID_RICH: AccountId = 1005;
pub const NOT_SUBNET_OWNER: AccountId = 666;
pub const NON_EXISTENT_NETUID: u16 = 999;
pub const WRAPPING_FEES_NETUID: u16 = 124;
pub const SUBTOKEN_DISABLED_NETUID: u16 = 13579;
parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BaseCallFilter = Everything;
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = BlockHashCount;
    type PalletInfo = PalletInfo;
    type SS58Prefix = SS58Prefix;
    type MaxConsumers = ConstU32<16>;
    type Nonce = u64;
    type Block = Block;
}

parameter_types! {
    pub const SwapProtocolId: PalletId = PalletId(*b"ten/swap");
    pub const MaxFeeRate: u16 = 10000; // 15.26%
    pub const MinimumLiquidity: u64 = 1_000;
    pub const MinimumReserves: NonZeroU64 = NonZeroU64::new(1).unwrap();
}

thread_local! {
    // maps netuid -> mocked tao reserve
    static MOCK_TAO_RESERVES: RefCell<HashMap<NetUid, TaoBalance>> =
        RefCell::new(HashMap::new());
    // maps netuid -> mocked alpha reserve
    static MOCK_ALPHA_RESERVES: RefCell<HashMap<NetUid, AlphaBalance>> =
        RefCell::new(HashMap::new());
}

#[derive(Clone)]
pub struct TaoReserve;

impl TaoReserve {
    pub fn set_mock_reserve(netuid: NetUid, value: TaoBalance) {
        MOCK_TAO_RESERVES.with(|m| {
            m.borrow_mut().insert(netuid, value);
        });
    }
}

impl TokenReserve<TaoBalance> for TaoReserve {
    fn reserve(netuid: NetUid) -> TaoBalance {
        // If test has set an override, use it
        if let Some(val) = MOCK_TAO_RESERVES.with(|m| m.borrow().get(&netuid).cloned()) {
            return val;
        }

        // Otherwise, fall back to our defaults
        match netuid.into() {
            123u16 => 10_000,
            WRAPPING_FEES_NETUID => 100_000_000_000_u64,
            _ => 1_000_000_000_000_u64,
        }
        .into()
    }

    fn increase_provided(_: NetUid, _: TaoBalance) {}
    fn decrease_provided(_: NetUid, _: TaoBalance) {}
}

#[derive(Clone)]
pub struct AlphaReserve;

impl AlphaReserve {
    pub fn set_mock_reserve(netuid: NetUid, value: AlphaBalance) {
        MOCK_ALPHA_RESERVES.with(|m| {
            m.borrow_mut().insert(netuid, value);
        });
    }
}

impl TokenReserve<AlphaBalance> for AlphaReserve {
    fn reserve(netuid: NetUid) -> AlphaBalance {
        // If test has set an override, use it
        if let Some(val) = MOCK_ALPHA_RESERVES.with(|m| m.borrow().get(&netuid).cloned()) {
            return val;
        }

        // Otherwise, fall back to our defaults
        match netuid.into() {
            123u16 => 10_000.into(),
            WRAPPING_FEES_NETUID => 400_000_000_000_u64.into(),
            _ => 4_000_000_000_000_u64.into(),
        }
    }

    fn increase_provided(_: NetUid, _: AlphaBalance) {}
    fn decrease_provided(_: NetUid, _: AlphaBalance) {}
}

pub type GetAlphaForTao = subtensor_swap_interface::GetAlphaForTao<TaoReserve, AlphaReserve>;
pub type GetTaoForAlpha = subtensor_swap_interface::GetTaoForAlpha<AlphaReserve, TaoReserve>;

#[allow(dead_code)]
pub(crate) trait TestExt<O: Order> {
    fn approx_expected_swap_output(
        sqrt_current_price: f64,
        liquidity_before: f64,
        order_liquidity: f64,
    ) -> f64;
}

impl TestExt<GetAlphaForTao> for Test {
    fn approx_expected_swap_output(
        sqrt_current_price: f64,
        liquidity_before: f64,
        order_liquidity: f64,
    ) -> f64 {
        let denom = sqrt_current_price * (sqrt_current_price * liquidity_before + order_liquidity);
        let per_order_liq = liquidity_before / denom;
        per_order_liq * order_liquidity
    }
}

impl TestExt<GetTaoForAlpha> for Test {
    fn approx_expected_swap_output(
        sqrt_current_price: f64,
        liquidity_before: f64,
        order_liquidity: f64,
    ) -> f64 {
        let denom = liquidity_before / sqrt_current_price + order_liquidity;
        let per_order_liq = sqrt_current_price * liquidity_before / denom;
        per_order_liq * order_liquidity
    }
}

// Mock implementor of SubnetInfo trait
pub struct MockLiquidityProvider;

impl SubnetInfo<AccountId> for MockLiquidityProvider {
    fn exists(netuid: NetUid) -> bool {
        netuid != NON_EXISTENT_NETUID.into()
    }

    fn mechanism(netuid: NetUid) -> u16 {
        if netuid == NetUid::from(0) { 0 } else { 1 }
    }

    fn is_owner(account_id: &AccountId, _netuid: NetUid) -> bool {
        *account_id != NOT_SUBNET_OWNER
    }

    // Only disable one subnet for testing
    fn is_subtoken_enabled(netuid: NetUid) -> bool {
        netuid.inner() != SUBTOKEN_DISABLED_NETUID
    }

    fn get_validator_trust(netuid: NetUid) -> Vec<u16> {
        match netuid.into() {
            123u16 => vec![4000, 3000, 2000, 1000],
            WRAPPING_FEES_NETUID => vec![8000, 7000, 6000, 5000],
            _ => vec![1000, 800, 600, 400],
        }
    }

    fn get_validator_permit(netuid: NetUid) -> Vec<bool> {
        match netuid.into() {
            123u16 => vec![true, true, false, true],
            WRAPPING_FEES_NETUID => vec![true, true, true, true],
            _ => vec![true, true, true, true],
        }
    }

    fn hotkey_of_uid(_netuid: NetUid, uid: u16) -> Option<AccountId> {
        Some(uid as AccountId)
    }
}

pub struct MockBalanceOps;

impl BalanceOps<AccountId> for MockBalanceOps {
    fn tao_balance(account_id: &AccountId) -> TaoBalance {
        match *account_id {
            OK_COLDKEY_ACCOUNT_ID => 100_000_000_000_000,
            OK_COLDKEY_ACCOUNT_ID_2 => 100_000_000_000_000,
            OK_COLDKEY_ACCOUNT_ID_RICH => 900_000_000_000_000_000_u64,
            _ => 1_000_000_000,
        }
        .into()
    }

    fn alpha_balance(
        _: NetUid,
        coldkey_account_id: &AccountId,
        hotkey_account_id: &AccountId,
    ) -> AlphaBalance {
        match (coldkey_account_id, hotkey_account_id) {
            (&OK_COLDKEY_ACCOUNT_ID, &OK_HOTKEY_ACCOUNT_ID) => 100_000_000_000_000,
            (&OK_COLDKEY_ACCOUNT_ID_2, &OK_HOTKEY_ACCOUNT_ID_2) => 100_000_000_000_000,
            (&OK_COLDKEY_ACCOUNT_ID_RICH, &OK_HOTKEY_ACCOUNT_ID_RICH) => {
                900_000_000_000_000_000_u64
            }
            _ => 1_000_000_000,
        }
        .into()
    }

    fn increase_stake(
        _coldkey: &AccountId,
        _hotkey: &AccountId,
        _netuid: NetUid,
        _alpha: AlphaBalance,
    ) -> Result<(), DispatchError> {
        Ok(())
    }

    fn decrease_stake(
        _coldkey: &AccountId,
        _hotkey: &AccountId,
        _netuid: NetUid,
        _alpha: AlphaBalance,
    ) -> Result<(), DispatchError> {
        Ok(())
    }
}

impl crate::pallet::Config for Test {
    type SubnetInfo = MockLiquidityProvider;
    type TaoReserve = TaoReserve;
    type AlphaReserve = AlphaReserve;
    type BalanceOps = MockBalanceOps;
    type ProtocolId = SwapProtocolId;
    type MaxFeeRate = MaxFeeRate;
    type MinimumLiquidity = MinimumLiquidity;
    type MinimumReserve = MinimumReserves;
    type WeightInfo = ();
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let storage = system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
    });
    ext
}
