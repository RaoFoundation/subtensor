#![allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::unwrap_used)]

use std::cell::RefCell;
use std::collections::HashMap;

use frame_support::traits::fungible::Mutate;
use frame_support::traits::tokens::Preservation;
use frame_support::{PalletId, derive_impl, parameter_types};
use frame_system as system;
use sp_core::H256;
use sp_runtime::traits::{BlakeTwo256, IdentityLookup};
use sp_runtime::{AccountId32, BuildStorage, DispatchError};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

use sp_core::H160;

use crate::{RailsOutbound, RailsStaking};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        UsdPsm: crate = 3,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BaseCallFilter = frame_support::traits::Everything;
    type BlockWeights = ();
    type BlockLength = ();
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<Self::AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = frame_support::traits::ConstU64<250>;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type SS58Prefix = frame_support::traits::ConstU16<42>;
    type OnSetCode = ();
    type MaxConsumers = frame_support::traits::ConstU32<16>;
    type Nonce = u64;
    type Block = Block;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type Balance = TaoBalance;
    type AccountStore = System;
    type ExistentialDeposit = ExistentialDeposit;
}

parameter_types! {
    pub const UsdPsmPalletId: PalletId = PalletId(*b"sn/rails");
    pub const ExistentialDeposit: TaoBalance = TaoBalance::new(1);
}

/// Deterministic mock staking engine: 1 TAO buys 2 alpha; unstaking pays
/// 1 TAO per 2 alpha. Netuid 99 always fails (fallback drills).
pub struct MockStaking;

thread_local! {
    static STAKES: RefCell<HashMap<(AccountId32, AccountId32, NetUid), u64>> =
        RefCell::new(HashMap::new());
}

pub fn staked_alpha(coldkey: &AccountId32, hotkey: &AccountId32, netuid: NetUid) -> u64 {
    STAKES.with(|s| {
        s.borrow()
            .get(&(coldkey.clone(), hotkey.clone(), netuid))
            .copied()
            .unwrap_or_default()
    })
}

pub const FAILING_NETUID: u16 = 99;

/// Seed stake directly (e.g. simulating emissions landing on the escrow).
pub fn set_staked_alpha(coldkey: &AccountId32, hotkey: &AccountId32, netuid: NetUid, alpha: u64) {
    STAKES.with(|s| {
        s.borrow_mut()
            .insert((coldkey.clone(), hotkey.clone(), netuid), alpha);
    });
}

impl RailsStaking<AccountId32> for MockStaking {
    fn stake(
        coldkey: &AccountId32,
        hotkey: &AccountId32,
        netuid: NetUid,
        tao: TaoBalance,
        min_alpha: AlphaBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        if netuid == NetUid::from(FAILING_NETUID) {
            return Err(DispatchError::Other("mock staking failure"));
        }
        let tao_u64: u64 = tao.into();
        let alpha = tao_u64 * 2;
        let min_alpha_u64: u64 = min_alpha.into();
        if alpha < min_alpha_u64 {
            return Err(DispatchError::Other("min alpha"));
        }
        // Burn the TAO from the coldkey (the real engine moves it to the
        // subnet account).
        Balances::burn_from(
            coldkey,
            tao,
            Preservation::Expendable,
            frame_support::traits::tokens::Precision::Exact,
            frame_support::traits::tokens::Fortitude::Polite,
        )?;
        STAKES.with(|s| {
            *s.borrow_mut()
                .entry((coldkey.clone(), hotkey.clone(), netuid))
                .or_default() += alpha;
        });
        Ok(AlphaBalance::from(alpha))
    }

    fn unstake(
        coldkey: &AccountId32,
        hotkey: &AccountId32,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        if netuid == NetUid::from(FAILING_NETUID) {
            return Err(DispatchError::Other("mock staking failure"));
        }
        let alpha_u64: u64 = alpha.into();
        let tao = alpha_u64 / 2;
        let min_tao_u64: u64 = min_tao.into();
        if tao < min_tao_u64 {
            return Err(DispatchError::Other("min tao"));
        }
        STAKES.with(|s| {
            let mut map = s.borrow_mut();
            let entry = map
                .get_mut(&(coldkey.clone(), hotkey.clone(), netuid))
                .ok_or(DispatchError::Other("no stake"))?;
            *entry = entry
                .checked_sub(alpha_u64)
                .ok_or(DispatchError::Other("insufficient stake"))?;
            Ok::<(), DispatchError>(())
        })?;
        Balances::mint_into(coldkey, TaoBalance::from(tao))?;
        Ok(TaoBalance::from(tao))
    }

    fn stake_of(hotkey: &AccountId32, coldkey: &AccountId32, netuid: NetUid) -> u64 {
        staked_alpha(coldkey, hotkey, netuid)
    }
}

/// Recording mock for the outbound bridge leg.
pub struct MockOutbound;

thread_local! {
    static DISPATCHES: RefCell<Vec<(H160, H160, u32, [u8; 32], Vec<u8>)>> =
        RefCell::new(Vec::new());
}

pub fn outbound_dispatches() -> Vec<(H160, H160, u32, [u8; 32], Vec<u8>)> {
    DISPATCHES.with(|d| d.borrow().clone())
}

impl RailsOutbound for MockOutbound {
    fn dispatch_mailbox(
        mailbox: H160,
        sender: H160,
        dest_domain: u32,
        recipient: [u8; 32],
        body: Vec<u8>,
    ) -> Result<(), DispatchError> {
        DISPATCHES.with(|d| {
            d.borrow_mut()
                .push((mailbox, sender, dest_domain, recipient, body))
        });
        Ok(())
    }
}

impl crate::pallet::Config for Test {
    type Currency = Balances;
    type Staking = MockStaking;
    type Outbound = MockOutbound;
    type AdminOrigin = frame_system::EnsureRoot<AccountId32>;
    type PalletId = UsdPsmPalletId;
}

pub fn account(byte: u8) -> AccountId32 {
    AccountId32::new([byte; 32])
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    STAKES.with(|s| s.borrow_mut().clear());
    DISPATCHES.with(|d| d.borrow_mut().clear());
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("frame_system storage should build");
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![
            (account(1), TaoBalance::from(1_000_000_000_000u64)),
            (account(2), TaoBalance::from(1_000_000_000_000u64)),
        ],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .expect("balances storage should build");
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}
