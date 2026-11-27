//! `pallet_lending` in the real runtime: the genesis and upgrade initialisation, the
//! registration hook, shorts end to end, the `Lending` proxy type and the runtime API.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use frame_support::{
    assert_noop, assert_ok,
    traits::{GetStorageVersion, OnRuntimeUpgrade, StorageVersion},
    weights::Weight,
};
use node_subtensor_runtime::{
    BuildStorage, Executive, Lending, Proxy, Runtime, RuntimeCall, RuntimeGenesisConfig,
    RuntimeOrigin, SubtensorModule, System, pallet_subtensor,
};
use pallet_lending::{
    LendingPoolInterface,
    migrations::MigrateToV1,
    params::{DeregParams, LendingParams, LongParams, ShortParams},
    types::{Leg, Side},
    views::ParamsView,
    weights::{SubstrateWeight, WeightInfo},
};
use pallet_lending_runtime_api::runtime_decl_for_lending_runtime_api::LendingRuntimeApiV1;
use pallet_subtensor::{
    Owner, SubnetAlphaIn, SubnetAlphaOut, SubnetMechanism, SubnetMovingPrice, SubnetTAO,
    SubtokenEnabled, TotalStake,
};
use pallet_subtensor_proxy::LastCallResult;
use pallet_subtensor_swap::{FeeRate, PalSwapInitialized, SwapBalancer, balancer::Balancer};
use sp_runtime::{DispatchError, Perquintill};
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, ProxyType, TaoBalance, Token};

type Api = Runtime;
type Pool = SubtensorModule;

const RAO: u64 = 1_000_000_000;
const POOL_TAO: u64 = 50_000 * RAO;
const POOL_ALPHA: u64 = 500_000 * RAO;
const START: u64 = 100_000 * RAO;

fn alice() -> AccountId {
    AccountId::new([0xA1; 32])
}

fn bob() -> AccountId {
    AccountId::new([0xB0; 32])
}

fn carol() -> AccountId {
    AccountId::new([0xC0; 32])
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis builds")
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn fund(who: &AccountId, tao: u64) {
    let credit = SubtensorModule::mint_tao(tao.into());
    let _ = SubtensorModule::spend_tao(who, credit, tao.into());
}

fn balance(who: &AccountId) -> u64 {
    SubtensorModule::get_coldkey_balance(who).to_u64()
}

fn live_subnets() -> Vec<NetUid> {
    <Pool as LendingPoolInterface<AccountId>>::live_subnets()
}

/// A dynamic subnet with a weighted pool of `tao` / `alpha` backed by the subnet account, and
/// an emission EMA price of `moving` (the pool's spot if `None`).
fn pool_subnet(netuid: u16, tao: u64, alpha: u64, moving: Option<f64>) -> NetUid {
    let netuid = NetUid::from(netuid);
    SubtensorModule::init_new_network(netuid, 100);
    SubnetMechanism::<Runtime>::insert(netuid, 1u16);
    SubtokenEnabled::<Runtime>::insert(netuid, true);
    SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(tao));
    SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(alpha));
    SubnetAlphaOut::<Runtime>::insert(netuid, AlphaBalance::from(1_000_000 * RAO));
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).expect("subnet account");
    fund(&subnet_account, tao);
    TotalStake::<Runtime>::mutate(|total| *total = total.saturating_add(tao.into()));
    SwapBalancer::<Runtime>::insert(
        netuid,
        Balancer::new(Perquintill::from_percent(50)).expect("valid weight"),
    );
    PalSwapInitialized::<Runtime>::insert(netuid, true);
    FeeRate::<Runtime>::insert(netuid, 33);
    let price = match moving {
        Some(price) => I96F32::saturating_from_num(price),
        None => I96F32::saturating_from_num(<Pool as LendingPoolInterface<AccountId>>::spot_price(
            netuid,
        )),
    };
    SubnetMovingPrice::<Runtime>::insert(netuid, price);
    netuid
}

/// The chain as it is before the upgrade that adds the pallet: no lending storage at all and
/// no pallet hotkey.
fn forget_lending() {
    let _ = frame_support::storage::unhashed::clear_prefix(
        &sp_io::hashing::twox_128(b"Lending"),
        None,
        None,
    );
    Owner::<Runtime>::remove(Lending::pallet_hotkey_account());
    assert_eq!(Lending::on_chain_storage_version(), StorageVersion::new(0));
    assert!(Api::market(NetUid::from(1)).is_none());
}

fn default_params() -> ParamsView {
    ParamsView {
        enabled: false,
        longs_enabled: false,
        lending: LendingParams::default(),
        short: ShortParams::default(),
        long: LongParams::default(),
        dereg: DeregParams::default(),
    }
}

/// Every live subnet has a market whose TWAP is the pool's spot and whose SMA is the
/// emission EMA price, the only seed the migration may read (SPEC I25).
fn assert_seeded(subnets: &[NetUid]) {
    assert!(!subnets.is_empty());
    for netuid in subnets.iter().copied() {
        let market = Api::market(netuid).expect("rings seeded");
        assert_eq!(
            market.twap,
            <Pool as LendingPoolInterface<AccountId>>::spot_price(netuid)
        );
        assert_eq!(
            market.sma,
            <Pool as LendingPoolInterface<AccountId>>::moving_alpha_price(netuid)
        );
        assert_eq!(market.spot, market.twap);
    }
}

/// A chain with lending enabled by root and one deep subnet; Alice holds 100,000 TAO.
fn enabled_market() -> NetUid {
    let netuid = pool_subnet(7, POOL_TAO, POOL_ALPHA, None);
    forget_lending();
    MigrateToV1::<Runtime>::on_runtime_upgrade();
    assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
    fund(&alice(), START);
    netuid
}

fn short_of(who: &AccountId, netuid: NetUid) -> Option<(u64, u64, u64)> {
    Api::position(who.clone(), netuid, Side::Short).map(|pos| match pos.leg {
        Leg::Short { minted, proceeds } => (minted, proceeds, pos.collateral),
        Leg::Long { .. } => panic!("short leg expected"),
    })
}

fn proxied(delegate: &AccountId, real: &AccountId, call: RuntimeCall) -> Result<(), DispatchError> {
    assert_ok!(Proxy::proxy(
        RuntimeOrigin::signed(delegate.clone()),
        real.clone().into(),
        Some(ProxyType::Lending),
        Box::new(call)
    ));
    LastCallResult::<Runtime>::get(real).expect("the proxy recorded a result")
}

#[test]
fn genesis_starts_with_lending_initialised_and_disabled() {
    new_test_ext().execute_with(|| {
        assert_eq!(Lending::on_chain_storage_version(), StorageVersion::new(1));
        assert_eq!(Api::params(), default_params());
        assert_seeded(&live_subnets());
        assert_eq!(
            Owner::<Runtime>::get(Lending::pallet_hotkey_account()),
            Lending::pallet_account()
        );
        // The migration has nothing to do on such a chain.
        assert_eq!(
            MigrateToV1::<Runtime>::on_runtime_upgrade(),
            <Runtime as frame_system::Config>::DbWeight::get().reads(1)
        );
    });
}

#[test]
fn migration_initialises_every_live_subnet_and_runs_once() {
    new_test_ext().execute_with(|| {
        pool_subnet(2, POOL_TAO, POOL_ALPHA, Some(0.25));
        pool_subnet(3, 7_000 * RAO, 90_000 * RAO, Some(0.05));
        pool_subnet(9, 120_000 * RAO, 40_000 * RAO, Some(2.5));
        forget_lending();
        let subnets = live_subnets();
        assert!(subnets.len() >= 4);

        #[cfg(feature = "try-runtime")]
        let state = MigrateToV1::<Runtime>::pre_upgrade().expect("pre_upgrade");
        let weight = MigrateToV1::<Runtime>::on_runtime_upgrade();
        #[cfg(feature = "try-runtime")]
        MigrateToV1::<Runtime>::post_upgrade(state).expect("post_upgrade");

        let db = <Runtime as frame_system::Config>::DbWeight::get();
        assert_eq!(
            weight,
            db.reads_writes(5, 5)
                .saturating_add(SubstrateWeight::<Runtime>::migrate_to_v1(
                    subnets.len() as u32
                ))
        );
        assert_eq!(Lending::on_chain_storage_version(), StorageVersion::new(1));
        assert_eq!(Api::params(), default_params());
        assert_seeded(&subnets);
        assert_eq!(
            Owner::<Runtime>::get(Lending::pallet_hotkey_account()),
            Lending::pallet_account()
        );

        // A second run reads the version and changes nothing: not the switch root has set
        // since, not the rings (the pools and their EMA prices have moved).
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        let before: Vec<_> = subnets.iter().map(|netuid| Api::market(*netuid)).collect();
        for netuid in subnets.iter().copied() {
            SubnetMovingPrice::<Runtime>::insert(netuid, I96F32::saturating_from_num(9));
        }
        #[cfg(feature = "try-runtime")]
        let state = MigrateToV1::<Runtime>::pre_upgrade().expect("pre_upgrade");
        assert_eq!(MigrateToV1::<Runtime>::on_runtime_upgrade(), db.reads(1));
        #[cfg(feature = "try-runtime")]
        MigrateToV1::<Runtime>::post_upgrade(state).expect("post_upgrade");
        let after: Vec<_> = subnets.iter().map(|netuid| Api::market(*netuid)).collect();
        assert_eq!(after, before);
        assert!(Api::params().enabled);
    });
}

/// The runtime's own upgrade path (the `Migrations` tuple and the pallet hooks) initialises
/// lending on a chain that has none.
#[test]
fn runtime_upgrade_initialises_lending() {
    new_test_ext().execute_with(|| {
        pool_subnet(2, POOL_TAO, POOL_ALPHA, Some(0.25));
        pool_subnet(3, 7_000 * RAO, 90_000 * RAO, Some(0.05));
        forget_lending();

        // FRAME stamps the storage version of a pallet with no storage before any migration
        // runs, so the initialisation cannot be guarded by the version.
        let weight: Weight = Executive::execute_on_runtime_upgrade();
        assert!(weight.any_gt(Weight::zero()));

        assert_eq!(Lending::on_chain_storage_version(), StorageVersion::new(1));
        assert!(pallet_lending::migrations::has_run::<Runtime>());
        assert_eq!(Api::params(), default_params());
        // The historic subtensor migrations that run after it on this genesis state reset the
        // test pools, so the rings are checked against what the migration read.
        for netuid in live_subnets() {
            assert!(Lending::price_refs(netuid).is_ok());
        }
        assert_eq!(
            Owner::<Runtime>::get(Lending::pallet_hotkey_account()),
            Lending::pallet_account()
        );
        assert_eq!(
            MigrateToV1::<Runtime>::on_runtime_upgrade(),
            <Runtime as frame_system::Config>::DbWeight::get().reads(1)
        );
    });
}

#[test]
fn registered_subnet_gets_price_rings() {
    new_test_ext().execute_with(|| {
        let netuid = SubtensorModule::get_next_netuid();
        assert!(Api::market(netuid).is_none());
        let lock = SubtensorModule::get_network_lock_cost().to_u64();
        fund(&alice(), lock + RAO);
        assert_ok!(SubtensorModule::register_network(
            RuntimeOrigin::signed(alice()),
            bob()
        ));
        assert!(live_subnets().contains(&netuid));
        let market = Api::market(netuid).expect("registration seeds the rings");
        // Both rings start at the new pool's own price.
        assert_eq!(market.twap, market.spot);
        assert_eq!(market.sma, market.spot);
        assert!(market.pool_tao > 0);
    });
}

#[test]
fn short_opens_and_closes_at_the_quoted_amounts() {
    new_test_ext().execute_with(|| {
        let netuid = pool_subnet(7, POOL_TAO, POOL_ALPHA, None);
        forget_lending();
        MigrateToV1::<Runtime>::on_runtime_upgrade();
        fund(&alice(), START);
        let q = 2_000 * RAO;

        // Disabled until root enables it; nobody else can.
        assert_noop!(
            Lending::open_short(RuntimeOrigin::signed(alice()), netuid, q, 5_000 * RAO, 0),
            pallet_lending::Error::<Runtime>::Disabled
        );
        assert_eq!(
            Api::quote_open_short(netuid, q),
            Err(pallet_lending::Error::<Runtime>::Disabled.into())
        );
        assert_noop!(
            Lending::set_enabled(RuntimeOrigin::signed(alice()), true),
            DispatchError::BadOrigin
        );
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));

        // Open at exactly the quote: the proceeds bound and the least collateral.
        let quote = Api::quote_open_short(netuid, q).expect("quotes");
        assert!(quote.proceeds > 0 && quote.min_collateral > 0);
        assert_eq!(quote.interest, 0);
        assert_noop!(
            Lending::open_short(
                RuntimeOrigin::signed(alice()),
                netuid,
                q,
                quote.min_collateral,
                quote.proceeds + 1
            ),
            pallet_lending::Error::<Runtime>::SlippageExceeded
        );
        assert_noop!(
            Lending::open_short(
                RuntimeOrigin::signed(alice()),
                netuid,
                q,
                quote.min_collateral - 1,
                quote.proceeds
            ),
            pallet_lending::Error::<Runtime>::CoverageNotMet
        );
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            quote.min_collateral,
            quote.proceeds
        ));
        assert_eq!(
            short_of(&alice(), netuid),
            Some((q, quote.proceeds, quote.min_collateral))
        );
        assert_eq!(
            balance(&alice()),
            START - quote.min_collateral - quote.deposit
        );
        let market = Api::market(netuid).expect("market");
        assert_eq!(market.synthetic_outstanding, q);
        assert_eq!(market.spot, quote.spot_after);
        let held = Api::positions_of(alice());
        assert_eq!(held.len(), 1);
        let first = held.first().expect("one position");
        assert_eq!((first.netuid, first.side), (netuid, Side::Short));
        assert_eq!(
            Some(first.position),
            Api::position(alice(), netuid, Side::Short)
        );

        // Close at exactly the quoted cost.
        let close = Api::quote_close_short(alice(), netuid, q).expect("quotes");
        assert!(close.full);
        assert_eq!(close.alpha, q);
        assert_eq!(close.cost, close.from_proceeds + close.from_collateral);
        assert_noop!(
            Lending::close_short(RuntimeOrigin::signed(alice()), netuid, q, close.cost - 1),
            pallet_lending::Error::<Runtime>::SlippageExceeded
        );
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            close.cost
        ));
        assert_eq!(short_of(&alice(), netuid), None);
        assert!(Api::positions_of(alice()).is_empty());
        assert_eq!(
            Api::market(netuid).expect("market").synthetic_outstanding,
            0
        );
        // Collateral, deposit and proceeds came back, less the cost of the buy-back.
        assert_eq!(balance(&alice()), START + quote.proceeds - close.cost);
    });
}

#[test]
fn longs_stay_disabled() {
    new_test_ext().execute_with(|| {
        let netuid = enabled_market();
        assert!(!Api::params().longs_enabled);
        assert_noop!(
            Lending::open_long(
                RuntimeOrigin::signed(alice()),
                netuid,
                1_000 * RAO,
                5_000 * RAO,
                0
            ),
            pallet_lending::Error::<Runtime>::LongsDisabled
        );
        assert_eq!(Api::position(alice(), netuid, Side::Long), None);
    });
}

#[test]
fn lending_proxy_manages_positions_and_nothing_else() {
    new_test_ext().execute_with(|| {
        let netuid = enabled_market();
        fund(&bob(), 10 * RAO);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(alice()),
            bob().into(),
            ProxyType::Lending,
            0
        ));
        let q = 2_000 * RAO;
        let quote = Api::quote_open_short(netuid, q).expect("quotes");
        let collateral = quote.min_collateral + 1_000 * RAO;

        // The delegate opens a short for Alice with Alice's funds.
        assert_eq!(
            proxied(
                &bob(),
                &alice(),
                RuntimeCall::Lending(pallet_lending::Call::open_short {
                    netuid,
                    q,
                    collateral,
                    min_tao_out: quote.proceeds,
                })
            ),
            Ok(())
        );
        assert_eq!(
            short_of(&alice(), netuid),
            Some((q, quote.proceeds, collateral))
        );
        assert_eq!(short_of(&bob(), netuid), None);

        // A batch of lending calls passes.
        let add = RuntimeCall::Lending(pallet_lending::Call::add_collateral {
            netuid,
            amount: RAO,
        });
        assert_eq!(
            proxied(
                &bob(),
                &alice(),
                RuntimeCall::Utility(pallet_subtensor_utility::Call::batch_all {
                    calls: vec![add.clone(), add.clone()],
                })
            ),
            Ok(())
        );
        assert_eq!(
            short_of(&alice(), netuid),
            Some((q, quote.proceeds, collateral + 2 * RAO))
        );

        // It cannot move Alice's TAO, alone or inside a batch, and a refused child undoes
        // the batch.
        let alice_before = balance(&alice());
        let carol_before = balance(&carol());
        let transfer = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
            dest: carol().into(),
            value: TaoBalance::from(RAO),
        });
        assert_eq!(
            proxied(&bob(), &alice(), transfer.clone()),
            Err(frame_system::Error::<Runtime>::CallFiltered.into())
        );
        assert_eq!(
            proxied(
                &bob(),
                &alice(),
                RuntimeCall::Utility(pallet_subtensor_utility::Call::batch_all {
                    calls: vec![add.clone(), transfer.clone()],
                })
            ),
            Err(frame_system::Error::<Runtime>::CallFiltered.into())
        );
        let _ = proxied(
            &bob(),
            &alice(),
            RuntimeCall::Utility(pallet_subtensor_utility::Call::force_batch {
                calls: vec![transfer.clone()],
            }),
        );
        let _ = proxied(
            &bob(),
            &alice(),
            RuntimeCall::Utility(pallet_subtensor_utility::Call::batch {
                calls: vec![transfer],
            }),
        );
        assert_eq!(balance(&alice()), alice_before);
        assert_eq!(balance(&carol()), carol_before);
        assert_eq!(
            short_of(&alice(), netuid),
            Some((q, quote.proceeds, collateral + 2 * RAO))
        );

        // Nor reach a root setter.
        assert!(
            proxied(
                &bob(),
                &alice(),
                RuntimeCall::Lending(pallet_lending::Call::set_enabled { enabled: false })
            )
            .is_err()
        );
        assert!(Api::params().enabled);

        // A staking proxy cannot touch the position.
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(alice()),
            carol().into(),
            ProxyType::Staking,
            0
        ));
        fund(&carol(), 10 * RAO);
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(carol()),
            alice().into(),
            Some(ProxyType::Staking),
            Box::new(RuntimeCall::Lending(pallet_lending::Call::close_short {
                netuid,
                alpha: q,
                max_tao_in: u64::MAX,
            }))
        ));
        assert_eq!(
            LastCallResult::<Runtime>::get(alice()),
            Some(Err(frame_system::Error::<Runtime>::CallFiltered.into()))
        );
        assert!(short_of(&alice(), netuid).is_some());
    });
}

#[test]
fn runtime_api_views_report_the_state() {
    new_test_ext().execute_with(|| {
        let netuid = enabled_market();
        let mut expected = default_params();
        expected.enabled = true;
        assert_eq!(Api::params(), expected);

        let market = Api::market(netuid).expect("market");
        assert_eq!((market.pool_tao, market.pool_alpha), (POOL_TAO, POOL_ALPHA));
        assert_eq!(market.quote_weight, Perquintill::from_percent(50));
        assert_eq!(market.swap_fee, 33);
        assert_eq!(
            (
                market.synthetic_outstanding,
                market.long_locked_alpha,
                market.long_lifted_tao
            ),
            (0, 0, 0)
        );
        assert_eq!(
            market.spot,
            <Pool as LendingPoolInterface<AccountId>>::spot_price(netuid)
        );
        assert_eq!(Api::market(NetUid::from(4_000)), None);

        // No position yet: nothing to read, increase or close.
        assert_eq!(Api::position(alice(), netuid, Side::Short), None);
        assert!(Api::positions_of(alice()).is_empty());
        assert_eq!(
            Api::quote_increase_short(alice(), netuid, RAO),
            Err(pallet_lending::Error::<Runtime>::NoPosition.into())
        );
        assert_eq!(
            Api::quote_close_short(alice(), netuid, RAO),
            Err(pallet_lending::Error::<Runtime>::NoPosition.into())
        );
        assert_eq!(
            Api::quote_open_short(netuid, 0),
            Err(pallet_lending::Error::<Runtime>::InvalidAmount.into())
        );
        assert_eq!(
            Api::quote_open_short(NetUid::from(4_000), RAO),
            Err(pallet_lending::Error::<Runtime>::PriceRingMissing.into())
        );

        // With a short open: an increase and a partial close are quoted and execute at the
        // quote.
        let q = 1_000 * RAO;
        let open = Api::quote_open_short(netuid, q).expect("quotes");
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            open.min_collateral,
            open.proceeds
        ));
        let grow = Api::quote_increase_short(alice(), netuid, q).expect("quotes");
        assert_eq!(grow.deposit, 0);
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q,
            grow.min_collateral,
            grow.proceeds
        ));
        assert_eq!(
            short_of(&alice(), netuid),
            Some((
                2 * q,
                open.proceeds + grow.proceeds,
                open.min_collateral + grow.min_collateral
            ))
        );
        let part = Api::quote_close_short(alice(), netuid, q / 2).expect("quotes");
        assert!(!part.full);
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(alice()),
            netuid,
            q / 2,
            part.cost
        ));
        let (minted, proceeds, collateral) = short_of(&alice(), netuid).expect("still open");
        assert_eq!(minted, 2 * q - q / 2);
        assert_eq!(proceeds, open.proceeds + grow.proceeds - part.from_proceeds);
        assert_eq!(
            collateral,
            open.min_collateral + grow.min_collateral - part.from_collateral
        );
        // A close of more than the position is quoted as the full close.
        let full = Api::quote_close_short(alice(), netuid, u64::MAX).expect("quotes");
        assert!(full.full);
        assert_eq!(full.alpha, minted);
    });
}

/// The API is part of the runtime's metadata, so clients can discover and decode it.
#[test]
fn runtime_api_is_in_the_metadata() {
    let metadata = Runtime::metadata_at_version(15).expect("v15 metadata");
    let frame_metadata::RuntimeMetadataPrefixed(_, frame_metadata::RuntimeMetadata::V15(v15)) =
        <frame_metadata::RuntimeMetadataPrefixed as codec::Decode>::decode(&mut &metadata[..])
            .expect("decodes")
    else {
        panic!("v15 metadata expected");
    };
    let api = v15
        .apis
        .iter()
        .find(|api| api.name == "LendingRuntimeApi")
        .expect("LendingRuntimeApi is declared");
    let methods: Vec<_> = api.methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        methods,
        [
            "params",
            "position",
            "positions_of",
            "market",
            "quote_open_short",
            "quote_increase_short",
            "quote_close_short"
        ]
    );
    let lending = v15
        .pallets
        .iter()
        .find(|pallet| pallet.name == "Lending")
        .expect("Lending is in the runtime");
    assert_eq!(lending.index, 34);
}
