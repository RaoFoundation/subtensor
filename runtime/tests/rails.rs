//! The canonical rails on the production runtime: its real weights, accounts, origins and
//! precompile set.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::indexing_slicing)]

use fp_evm::Context;
use frame_support::{BoundedVec, assert_noop, assert_ok, traits::PalletInfoAccess};
use node_subtensor_runtime::{
    Balances, BuildStorage, CanonicalPool, Rails, Runtime, RuntimeGenesisConfig, RuntimeOrigin,
    SubtensorModule, System, Tusd,
};
use pallet_evm::{AddressMapping, PrecompileSet};
use pallet_rails::{Destination, Order, SpokeAsset};
use precompile_utils::{
    prelude::UnboundedBytes, solidity::encode_with_selector, testing::MockHandle,
};
use sp_core::{H160, H256, U256};
use sp_runtime::{DispatchError, Permill};
use subtensor_precompiles::{PrecompileExt, Precompiles, RailsPrecompile, rails_wire};
use subtensor_runtime_common::{
    AlphaBalance, NetUid, TaoBalance, TusdBalance,
    rails::{OriginChain, Payer, StableAsset},
};
use subtensor_swap_interface::SwapHandler;

type AccountId = <Runtime as frame_system::Config>::AccountId;

const UNIT: u64 = 1_000_000_000;
const BASE: u32 = 8453;
const NETUID: u16 = 1;
/// A tenth of what a block has.
const GAS_LIMIT: u64 = 7_500_000;

fn portal() -> H256 {
    H256::repeat_byte(0xB0)
}

fn gateway() -> H160 {
    H160::from_low_u64_be(0x6A7E)
}

fn wallet() -> H160 {
    H160::repeat_byte(0xBB)
}

fn account(seed: u8) -> AccountId {
    AccountId::from([seed; 32])
}

fn mapped(address: H160) -> AccountId {
    <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address)
}

fn fund(who: &AccountId, tao: u64) {
    let tao = TaoBalance::from(tao);
    let credit = SubtensorModule::mint_tao(tao);
    let _ = SubtensorModule::spend_tao(who, credit, tao).unwrap();
}

fn selector(signature: &str) -> u32 {
    let hash = sp_io::hashing::keccak_256(signature.as_bytes());
    u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]])
}

/// The production runtime with the rails set up as governance would: a bridge to Base, the
/// gateway, a subnet whose service takes credits, a pool with a price history, canonical
/// TAO and a market.
fn rails_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        // Written directly: setting it through the pallet also checks the Aura slot.
        pallet_timestamp::Now::<Runtime>::put(1_000_000u64);
        assert_ok!(Tusd::set_bridge(
            RuntimeOrigin::root(),
            BASE,
            pallet_tusd::BridgeRoute {
                origin: OriginChain::Evm(8453),
                domain: BASE,
                portal: portal(),
                asset: StableAsset::Usdc,
            },
            TusdBalance::from(u64::MAX / 4),
            TusdBalance::from(u64::MAX / 4),
            1,
        ));
        assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), BASE, true));
        assert_ok!(Rails::set_gateway(
            RuntimeOrigin::root(),
            Some(mapped(gateway()))
        ));
        // A message to Base costs the gateway a thousandth of a TAO, and a hundred are
        // paid for.
        assert_ok!(Rails::set_message_cost(
            RuntimeOrigin::root(),
            BASE,
            (UNIT / 1_000).into()
        ));
        fund(&account(0x12), UNIT);
        assert_ok!(Rails::fund_messages(
            RuntimeOrigin::signed(account(0x12)),
            BASE,
            (UNIT / 10).into()
        ));

        let netuid = NetUid::from(NETUID);
        SubtensorModule::init_new_network(netuid, 100);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::FirstEmissionBlockNumber::<Runtime>::insert(netuid, 0);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(10_000 * UNIT));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            netuid,
            AlphaBalance::from(20_000 * UNIT),
        );
        fund(
            &SubtensorModule::get_subnet_account_id(netuid).unwrap(),
            10_000 * UNIT,
        );
        pallet_subtensor::GammaEnabled::<Runtime>::put(true);
        pallet_subtensor::SubnetGammaEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::TaoUsdPriceOracle::<Runtime>::put(account(0x0C));
        assert_ok!(SubtensorModule::feed_tao_usd_price(
            RuntimeOrigin::signed(account(0x0C)),
            400 * UNIT
        ));
        pallet_subtensor::SubnetGammaPrice::<Runtime>::insert(
            netuid,
            <pallet_subtensor_swap::Pallet<Runtime> as SwapHandler>::current_alpha_price(netuid),
        );
        pallet_subtensor::Services::<Runtime>::insert(
            netuid,
            account(0x5E),
            pallet_subtensor::ServiceInfo {
                url: BoundedVec::truncate_from(b"https://service.example".to_vec()),
            },
        );

        assert_ok!(CanonicalPool::set_enabled(RuntimeOrigin::root(), true));
        fund(&account(0x11), 100_001 * UNIT);
        assert_ok!(Tusd::mint(BASE, &account(0x11), (40_000_000 * UNIT).into()));
        assert_ok!(CanonicalPool::add_liquidity(
            RuntimeOrigin::signed(account(0x11)),
            (100_000 * UNIT).into(),
            (40_000_000 * UNIT).into(),
            0
        ));
        // Two periods of price history.
        let period = <Runtime as pallet_canonical_pool::Config>::TwapPeriod::get();
        System::set_block_number(1 + 2 * period);
        assert_ok!(SubtensorModule::feed_tao_usd_price(
            RuntimeOrigin::signed(account(0x0C)),
            400 * UNIT
        ));

        assert_ok!(Rails::register_token(
            RuntimeOrigin::root(),
            BASE,
            SpokeAsset::Tao,
            H160::repeat_byte(0x7A)
        ));
        assert_ok!(Rails::set_tao_cap(
            RuntimeOrigin::root(),
            BASE,
            (1_000 * UNIT).into()
        ));
        assert_ok!(Rails::open_market(
            RuntimeOrigin::root(),
            netuid,
            (1_000_000 * UNIT).into()
        ));
        assert_ok!(Rails::list_market(
            RuntimeOrigin::root(),
            netuid,
            BASE,
            H160::repeat_byte(0x64)
        ));
    });
    ext
}

fn order(nonce: u64, amount: u64, destination: Destination<AccountId>) -> Order<AccountId> {
    Order {
        nonce,
        payer: H160::repeat_byte(0xAA),
        source: SpokeAsset::Stable,
        amount,
        destination,
        min_out: 0,
        deadline: u64::MAX,
    }
}

/// The gateway delivers `order` through the runtime's precompile set with [`GAS_LIMIT`].
/// Returns the status, the amount delivered and the gas used.
fn deliver(order: &Order<AccountId>) -> (u8, u64, u64) {
    let rails = H160::from_low_u64_be(RailsPrecompile::<Runtime>::INDEX);
    let mut handle = MockHandle::new(
        rails,
        Context {
            address: rails,
            caller: gateway(),
            apparent_value: U256::zero(),
        },
    );
    handle.gas_limit = GAS_LIMIT;
    handle.input = encode_with_selector(
        selector("execute(uint32,bytes32,bytes)"),
        (
            BASE,
            portal(),
            UnboundedBytes::from(rails_wire::encode_order(order)),
        ),
    );
    let output = Precompiles::<Runtime>::new()
        .execute(&mut handle)
        .expect("the Rails precompile is registered")
        .expect("the order is delivered")
        .output;
    (
        U256::from_big_endian(&output[..32]).low_u64() as u8,
        U256::from_big_endian(&output[32..64]).low_u64(),
        handle.gas_used,
    )
}

#[test]
fn the_rails_pallets_keep_their_indices() {
    assert_eq!(<Tusd as PalletInfoAccess>::index(), 34);
    assert_eq!(<CanonicalPool as PalletInfoAccess>::index(), 35);
    assert_eq!(<Rails as PalletInfoAccess>::index(), 36);
    assert!(Precompiles::<Runtime>::used_addresses().contains(&H160::from_low_u64_be(0x0815)));
}

#[test]
fn the_hub_keeps_its_funds_in_separate_accounts() {
    let accounts = [
        Rails::hub_account(),
        Rails::reserve_account(),
        Rails::vault_account(NetUid::from(1)),
        Rails::vault_account(NetUid::from(2)),
        CanonicalPool::account_id(),
        SubtensorModule::index_hotkey(NetUid::from(1)),
    ];
    for (position, account) in accounts.iter().enumerate() {
        assert!(!accounts[position + 1..].contains(account));
    }
}

#[test]
fn an_order_for_alpha_tokens_settles_within_the_gas_limit() {
    rails_ext().execute_with(|| {
        let netuid = NetUid::from(NETUID);
        let (status, shares, gas) = deliver(&order(
            0,
            4_000 * UNIT,
            Destination::Alpha {
                netuid,
                to: wallet(),
            },
        ));

        assert_eq!(status, 1);
        assert!(shares > 9 * UNIT, "{shares}");
        // The vault holds the alpha at the subnet's index hotkey.
        assert_eq!(
            u64::from(SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &SubtensorModule::index_hotkey(netuid),
                &Rails::vault_account(netuid),
                netuid,
            )),
            shares
        );
        assert!(gas < GAS_LIMIT, "{gas}");
        assert_ok!(Rails::do_try_state());
        assert_ok!(CanonicalPool::do_try_state());
        assert_ok!(Tusd::do_try_state());
    });
}

#[test]
fn an_order_for_credits_spends_gamma_for_the_address() {
    rails_ext().execute_with(|| {
        let netuid = NetUid::from(NETUID);
        let (status, gamma, gas) = deliver(&order(
            0,
            100 * UNIT,
            Destination::Credits {
                netuid,
                service: account(0x5E),
                beneficiary: wallet(),
            },
        ));

        assert_eq!(status, 1);
        // A hundred dollars buy a little less than a hundred dollars of credit.
        assert!(gamma > 99 * UNIT && gamma < 100 * UNIT, "{gamma}");
        assert_eq!(
            u64::from(pallet_subtensor::GammaSpentByPayer::<Runtime>::get((
                netuid,
                account(0x5E),
                Payer::Evm(wallet()),
            ))),
            gamma
        );
        assert!(gas < GAS_LIMIT, "{gas}");
        assert_ok!(Rails::do_try_state());
    });
}

#[test]
fn the_guardian_pauses_every_rail_and_cannot_resume_one() {
    rails_ext().execute_with(|| {
        let guardian = account(0x6D);
        // Until root names it, the guardian is nobody.
        assert_noop!(
            Rails::set_paused(RuntimeOrigin::signed(guardian.clone()), true),
            DispatchError::BadOrigin
        );
        assert_ok!(Rails::set_guardian(
            RuntimeOrigin::root(),
            Some(guardian.clone())
        ));
        let signed = || RuntimeOrigin::signed(guardian.clone());

        assert_ok!(Rails::set_paused(signed(), true));
        assert_ok!(CanonicalPool::set_enabled(signed(), false));
        assert_ok!(Tusd::set_bridge_enabled(signed(), BASE, false));

        assert_noop!(Rails::set_paused(signed(), false), DispatchError::BadOrigin);
        assert_noop!(
            CanonicalPool::set_enabled(signed(), true),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Tusd::set_bridge_enabled(signed(), BASE, true),
            DispatchError::BadOrigin
        );
        // Nor can it do anything that only governance does.
        assert_noop!(
            Rails::set_gateway(signed(), Some(guardian.clone())),
            DispatchError::BadOrigin
        );
        assert_noop!(
            CanonicalPool::set_fee(signed(), Permill::from_parts(500)),
            DispatchError::BadOrigin
        );
        // Liquidity providers can still leave a paused pool.
        let shares = pallet_canonical_pool::Shares::<Runtime>::get(account(0x11));
        assert_ok!(CanonicalPool::remove_liquidity(
            RuntimeOrigin::signed(account(0x11)),
            shares / 2,
            0.into(),
            0.into()
        ));
        assert!(Balances::free_balance(account(0x11)) > TaoBalance::from(40_000 * UNIT));
    });
}
