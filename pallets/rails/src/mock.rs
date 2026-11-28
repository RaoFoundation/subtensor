#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use frame_support::{
    Blake2_128Concat, PalletId, assert_ok, derive_impl, parameter_types, storage_alias,
    traits::{
        EitherOfDiverse,
        fungible::{Inspect, Mutate},
        tokens::{Fortitude, Preservation},
    },
    weights::Weight,
};
use frame_system::EnsureRoot;
use sp_core::{H160, H256};
use sp_runtime::{
    AccountId32, BuildStorage, DispatchError, Permill, TokenError, traits::IdentityLookup,
};
use subtensor_runtime_common::{
    AlphaBalance, GammaBalance, NetUid, TaoBalance, TusdBalance,
    rails::{BridgedPayment, OriginChain, Payer, StableAsset},
};

use crate::{EnsureGuardian, RailsStaking, SubnetStatus};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Timestamp: pallet_timestamp = 3,
        Tusd: pallet_tusd = 4,
        Pool: pallet_canonical_pool = 5,
        Rails: crate = 6,
    }
);

/// As wide as the runtime's: the hub derives several accounts from one pallet id, and they
/// are distinct only in an account id with room for the suffix.
pub type AccountId = AccountId32;

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<AccountId>;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
}

pub const fn account(seed: u8) -> AccountId {
    AccountId32::new([seed; 32])
}

/// One TAO, one tUSD, one alpha, one share or one gamma.
pub const UNIT: u64 = 1_000_000_000;
pub const EXISTENTIAL_DEPOSIT: u64 = 500;
pub const TWAP_PERIOD: u32 = 10;
pub const MAX_OUTBOX: u32 = 8;
/// A bridge with this many deferred messages is congested.
pub const CONGESTED_AT: u32 = 3;
pub const RECEIPTS_KEPT: u64 = 4;

/// The account of the gateway contract.
pub const GATEWAY: AccountId = account(50);
/// The account that may only pause.
pub const GUARDIAN: AccountId = account(51);
/// The pool's first liquidity provider.
pub const PROVIDER: AccountId = account(52);
/// Where the mock staking keeps the TAO that is staked or spent.
pub const SUBNET_POOL: AccountId = account(53);

/// What the mock staking answers for a netuid without a live subnet.
pub const NOT_LIVE: DispatchError = DispatchError::Unavailable;
/// What it answers for a service that takes no credits.
pub const NO_SERVICE: DispatchError = DispatchError::CannotLookup;

/// The bridge to Base: Hyperlane domain 8453, with the portal below.
pub const BASE: u32 = 8453;
/// A second bridge, to Ethereum.
pub const ETHEREUM: u32 = 1;
/// A bridge that only funds test accounts with tUSD.
pub const FAUCET: u32 = 999;

/// The alpha that the mock staking pays for one TAO, on every subnet.
pub const ALPHA_PER_TAO: u64 = 2;
/// The oracle's TAO price in the mock: 400 dollars.
pub const ORACLE_PRICE: u64 = 400 * UNIT;

pub fn portal(bridge: u32) -> H256 {
    H256::repeat_byte(bridge as u8)
}

parameter_types! {
    pub const ExistentialDeposit: TaoBalance = TaoBalance::new(EXISTENTIAL_DEPOSIT);
    pub const MinBalance: TusdBalance = TusdBalance::new(10_000_000);
    pub const PoolPalletId: PalletId = PalletId(*b"sn/cpool");
    pub const RailsPalletId: PalletId = PalletId(*b"sn/rails");
    pub const MaxInitialPriceDeviation: Permill = Permill::from_percent(2);
    pub const MinReserve: u64 = 1_000_000;
    pub const TwapPeriod: u32 = TWAP_PERIOD;
    pub const MaxOutbox: u32 = MAX_OUTBOX;
    pub const CongestedAt: u32 = CONGESTED_AT;
    pub const ReceiptsKept: u64 = RECEIPTS_KEPT;
}

// The mock staking keeps its state in storage, as the subtensor pallet does: an order that
// is refunded must take its staking back with everything else.

/// The subnets the mock staking knows: whether each lives, and the name it published.
#[storage_alias]
pub type Subnets = StorageMap<MockStaking, Blake2_128Concat, NetUid, (u8, u64, Vec<u8>)>;
/// The alpha each vault has staked.
#[storage_alias]
pub type Staked = StorageMap<
    MockStaking,
    Blake2_128Concat,
    (AccountId, NetUid),
    u64,
    frame_support::pallet_prelude::ValueQuery,
>;
/// The index hotkeys opened, with their owner.
#[storage_alias]
pub type OpenedIndexes = StorageMap<MockStaking, Blake2_128Concat, NetUid, AccountId>;
/// The services that take credits.
#[storage_alias]
pub type Services = StorageMap<MockStaking, Blake2_128Concat, (NetUid, AccountId), ()>;
/// The gamma spent for each payer, as the subtensor pallet would keep it.
#[storage_alias]
pub type Spent = StorageMap<
    MockStaking,
    Blake2_128Concat,
    (NetUid, AccountId, H160),
    u64,
    frame_support::pallet_prelude::ValueQuery,
>;
/// The payments the mock staking saw: payer, chain and nonce.
#[storage_alias]
pub type Payments = StorageValue<
    MockStaking,
    Vec<(H160, OriginChain, u64)>,
    frame_support::pallet_prelude::ValueQuery,
>;

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type AccountStore = System;
    type Balance = TaoBalance;
    type ExistentialDeposit = ExistentialDeposit;
}

#[derive_impl(pallet_timestamp::config_preludes::TestDefaultConfig)]
impl pallet_timestamp::Config for Test {}

pub type Pauser = EitherOfDiverse<EnsureRoot<AccountId>, EnsureGuardian<Test>>;

impl pallet_tusd::Config for Test {
    type AdminOrigin = EnsureRoot<AccountId>;
    type PauseOrigin = Pauser;
    type MinBalance = MinBalance;
    type WeightInfo = ();
}

pub struct MockOracle;
impl pallet_canonical_pool::TaoUsdOracle for MockOracle {
    fn tao_usd_price() -> Option<u64> {
        Some(ORACLE_PRICE)
    }
}

impl pallet_canonical_pool::Config for Test {
    type Currency = Balances;
    type AdminOrigin = EnsureRoot<AccountId>;
    type PauseOrigin = Pauser;
    type PriceOracle = MockOracle;
    type PalletId = PoolPalletId;
    type MaxInitialPriceDeviation = MaxInitialPriceDeviation;
    type MinReserve = MinReserve;
    type TwapPeriod = TwapPeriod;
    type WeightInfo = ();
}

/// Staking without a subnet AMM: every subnet pays [`ALPHA_PER_TAO`] alpha for a TAO, both
/// ways, and holds the TAO at [`SUBNET_POOL`]. Tests add emissions with [`accrue`].
///
/// Like the subtensor pallet, it leaves the existential deposit in the account that pays.
pub struct MockStaking;

impl MockStaking {
    fn live(netuid: NetUid) -> Result<(), DispatchError> {
        match Self::subnet_status(netuid) {
            SubnetStatus::Live { .. } => Ok(()),
            _ => Err(NOT_LIVE),
        }
    }

    /// Moves `tao` of `payer`'s into the subnet, less what keeps the payer's account alive.
    /// Returns what moved.
    fn pay_in(payer: &AccountId, tao: TaoBalance) -> Result<TaoBalance, DispatchError> {
        let tao = tao.min(Balances::reducible_balance(
            payer,
            Preservation::Preserve,
            Fortitude::Polite,
        ));
        if tao == TaoBalance::from(0) {
            return Err(DispatchError::Token(TokenError::FundsUnavailable));
        }
        Balances::transfer(payer, &SUBNET_POOL, tao, Preservation::Preserve)?;
        Ok(tao)
    }
}

impl RailsStaking<AccountId> for MockStaking {
    fn subnet_status(netuid: NetUid) -> SubnetStatus {
        match Subnets::get(netuid) {
            Some((0, generation, _)) => SubnetStatus::Live { generation },
            Some((1, ..)) => SubnetStatus::Dissolving,
            _ => SubnetStatus::Absent,
        }
    }

    fn subnet_name(netuid: NetUid) -> Option<Vec<u8>> {
        Subnets::get(netuid)
            .map(|(.., name)| name)
            .filter(|name| !name.is_empty())
    }

    fn open_index(netuid: NetUid, vault: &AccountId) -> Result<(), DispatchError> {
        Self::live(netuid)?;
        OpenedIndexes::insert(netuid, vault);
        Ok(())
    }

    fn staked(vault: &AccountId, netuid: NetUid) -> AlphaBalance {
        Staked::get((vault.clone(), netuid)).into()
    }

    fn stake(
        vault: &AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        Self::live(netuid)?;
        let tao = Self::pay_in(vault, tao)?;
        let alpha = u64::from(tao) * ALPHA_PER_TAO;
        Staked::mutate((vault.clone(), netuid), |staked| *staked += alpha);
        Ok(alpha.into())
    }

    fn unstake(
        vault: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::live(netuid)?;
        let alpha = u64::from(alpha);
        let left = Staked::get((vault.clone(), netuid))
            .checked_sub(alpha)
            .ok_or(DispatchError::Token(
                sp_runtime::TokenError::FundsUnavailable,
            ))?;
        let tao = TaoBalance::from(alpha / ALPHA_PER_TAO);
        Balances::transfer(&SUBNET_POOL, vault, tao, Preservation::Expendable)?;
        Staked::insert((vault.clone(), netuid), left);
        Ok(tao)
    }

    fn spend_at_service(
        funder: &AccountId,
        netuid: NetUid,
        service: &AccountId,
        tao: TaoBalance,
        tao_usd_price_cap: u64,
        payment: BridgedPayment<AccountId>,
    ) -> Result<(AlphaBalance, GammaBalance), DispatchError> {
        Self::live(netuid)?;
        if !Services::contains_key((netuid, service.clone())) {
            return Err(NO_SERVICE);
        }
        let Payer::Evm(payer) = payment.payer else {
            return Err(DispatchError::Other("the hub pays for addresses only"));
        };
        let tao = Self::pay_in(funder, tao)?;
        let price = u128::from(ORACLE_PRICE.min(tao_usd_price_cap));
        let gamma = (u128::from(u64::from(tao)) * price / u128::from(UNIT)) as u64;
        Spent::mutate((netuid, service.clone(), payer), |spent| *spent += gamma);
        Payments::append((payer, payment.origin_chain, payment.origin_nonce));
        Ok(((u64::from(tao) * ALPHA_PER_TAO).into(), gamma.into()))
    }

    fn weight() -> Weight {
        Weight::from_parts(1_000, 0)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_subnet() -> NetUid {
        add_subnet(1, "Benchmark");
        NetUid::from(1)
    }
}

impl crate::Config for Test {
    type Staking = MockStaking;
    type Time = Timestamp;
    type AdminOrigin = EnsureRoot<AccountId>;
    type PauseOrigin = Pauser;
    type PalletId = RailsPalletId;
    type MaxOutbox = MaxOutbox;
    type CongestedAt = CongestedAt;
    type ReceiptsKept = ReceiptsKept;
    type WeightInfo = ();
}

/// Adds a live subnet that published `name`.
pub fn add_subnet(netuid: u16, name: &str) {
    Subnets::insert(NetUid::from(netuid), (0, 1, name.as_bytes().to_vec()));
}

pub fn set_subnet_status(netuid: u16, status: SubnetStatus) {
    Subnets::mutate(NetUid::from(netuid), |subnet| {
        let name = subnet.take().map(|(.., name)| name).unwrap_or_default();
        *subnet = match status {
            SubnetStatus::Live { generation } => Some((0, generation, name)),
            SubnetStatus::Dissolving => Some((1, 0, name)),
            SubnetStatus::Absent => None,
        };
    });
}

/// Lets `service` take credits on `netuid`.
pub fn add_service(netuid: u16, service: AccountId) {
    Services::insert((NetUid::from(netuid), service), ());
}

/// Emissions: the vault of `netuid` earns `alpha`, and the TAO to pay it out exists.
pub fn accrue(netuid: u16, alpha: u64) {
    let vault = Rails::vault_account(NetUid::from(netuid));
    Staked::mutate((vault, NetUid::from(netuid)), |staked| *staked += alpha);
    fund_tao(SUBNET_POOL, alpha / ALPHA_PER_TAO);
}

pub fn fund_tao(who: AccountId, tao: u64) {
    let held = Balances::free_balance(&who);
    Balances::set_balance(&who, held + TaoBalance::from(tao));
}

pub fn fund_tusd(who: AccountId, tusd: u64) {
    assert_ok!(Tusd::mint(FAUCET, &who, tusd.into()));
}

fn open_bridge(id: u32, origin: OriginChain) {
    assert_ok!(Tusd::set_bridge(
        RuntimeOrigin::root(),
        id,
        pallet_tusd::BridgeRoute {
            origin,
            domain: id,
            portal: portal(id),
            asset: StableAsset::Usdc,
        },
        TusdBalance::from(u64::MAX / 4),
        TusdBalance::from(u64::MAX / 4),
        1,
    ));
    assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), id, true));
}

/// A chain with the rails ready: bridges to Base and Ethereum, a gateway and a guardian, a
/// pool of 10,000 TAO and 4,000,000 tUSD whose time-weighted price is established, and
/// canonical TAO registered on Base.
pub fn new_test_ext() -> sp_io::TestExternalities {
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("frame_system storage should build");
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        Timestamp::set_timestamp(1_000_000);
        open_bridge(BASE, OriginChain::Evm(8453));
        open_bridge(ETHEREUM, OriginChain::Evm(1));
        open_bridge(FAUCET, OriginChain::Bittensor);
        // The gateway account exists and holds nothing more: the hub hands it what it pays
        // the transport.
        fund_tao(GATEWAY, EXISTENTIAL_DEPOSIT);
        assert_ok!(Rails::set_gateway(RuntimeOrigin::root(), Some(GATEWAY)));
        assert_ok!(Rails::set_guardian(RuntimeOrigin::root(), Some(GUARDIAN)));
        // A transport that carries the hub's messages for nothing. The tests of who pays
        // for a message set a reserve.
        for bridge in [BASE, ETHEREUM] {
            assert_ok!(Rails::set_message_reserve(
                RuntimeOrigin::root(),
                bridge,
                0.into(),
                0.into()
            ));
        }

        assert_ok!(Pool::set_enabled(RuntimeOrigin::root(), true));
        assert_ok!(Pool::set_swap_caps(
            RuntimeOrigin::root(),
            pallet_canonical_pool::SwapCaps {
                per_swap: Permill::from_percent(50),
                per_window: Permill::one(),
                window: 1,
            }
        ));
        fund_tao(PROVIDER, 10_001 * UNIT);
        fund_tusd(PROVIDER, 4_000_000 * UNIT);
        assert_ok!(Pool::add_liquidity(
            RuntimeOrigin::signed(PROVIDER),
            (10_000 * UNIT).into(),
            (4_000_000 * UNIT).into(),
            0
        ));
        System::set_block_number(1 + u64::from(TWAP_PERIOD));

        assert_ok!(Rails::register_token(
            RuntimeOrigin::root(),
            BASE,
            crate::SpokeAsset::Tao,
            H160::repeat_byte(0x7A)
        ));
        assert_ok!(Rails::set_tao_cap(
            RuntimeOrigin::root(),
            BASE,
            (1_000 * UNIT).into()
        ));
        fund_tao(SUBNET_POOL, UNIT);
    });
    ext
}
