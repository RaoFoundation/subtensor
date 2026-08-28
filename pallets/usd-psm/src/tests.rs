#![allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::unwrap_used)]

use frame_support::traits::Hooks;
use frame_support::{assert_err, assert_ok};
use sp_core::H160;
use sp_runtime::AccountId32;
use subtensor_runtime_common::NetUid;
use subtensor_runtime_common::rails::{
    AssetId, FallbackReason, GatewayAction, GatewayEnvelope,
};

use crate::mock::*;
use crate::{
    Error, NextNonce, PoolTUsdReserve, PoolTaoReserve, ProcessedNonces, SharesOutstanding,
    TUsdBalances, pallet,
};

type UsdPsmError = Error<Test>;

const USDC: u32 = 0;
const BASE_DOMAIN: u32 = 8453;
const NETUID: u16 = 1;

fn gateway_h160() -> H160 {
    H160::from_low_u64_be(0xbeef)
}

fn buyer_h160() -> H160 {
    H160::from_low_u64_be(0xd00d)
}

fn chutes_recipient() -> [u8; 32] {
    let mut r = [0u8; 32];
    r[31] = 0x77;
    r
}

fn portal_recipient() -> [u8; 32] {
    let mut r = [0u8; 32];
    r[31] = 0x88;
    r
}

fn escrow_hotkey() -> AccountId32 {
    account(9)
}

fn setup_psm_and_pool() {
    assert_ok!(UsdPsm::register_usd_asset(
        RuntimeOrigin::root(),
        USDC,
        H160::from_low_u64_be(0xcafe),
        1_000_000_000_000, // cap
        0,                 // no refill
        0,                 // no haircut
    ));
    assert_ok!(UsdPsm::set_gateway(RuntimeOrigin::root(), gateway_h160()));
    // 1000 TAO : 100_000 tUSD => 1 TAO = 100 tUSD.
    assert_ok!(UsdPsm::init_pool(
        RuntimeOrigin::root(),
        account(1),
        1_000_000_000_000,   // 1000 TAO
        100_000_000_000_000, // 100k tUSD
        0,                   // no fee for easy math
    ));
}

fn setup_outbound() {
    assert_ok!(UsdPsm::set_hub_mailbox(
        RuntimeOrigin::root(),
        H160::from_low_u64_be(0x1111)
    ));
    assert_ok!(UsdPsm::set_outbound_route(
        RuntimeOrigin::root(),
        BASE_DOMAIN,
        NetUid::from(NETUID),
        chutes_recipient(),
    ));
    assert_ok!(UsdPsm::set_usd_route(
        RuntimeOrigin::root(),
        BASE_DOMAIN,
        portal_recipient(),
    ));
    assert_ok!(UsdPsm::set_escrow_hotkey(
        RuntimeOrigin::root(),
        NetUid::from(NETUID),
        escrow_hotkey(),
    ));
}

fn envelope(action: GatewayAction, amount: u64, nonce: u64) -> Vec<u8> {
    GatewayEnvelope {
        asset: AssetId::Usd(USDC),
        amount,
        dest: account(7),
        action,
        nonce,
    }
    .to_wire()
}

fn buy_envelope(amount: u64, nonce: u64) -> Vec<u8> {
    GatewayEnvelope {
        asset: AssetId::Usd(USDC),
        amount,
        dest: AccountId32::new([0u8; 32]),
        action: GatewayAction::BuyShares {
            netuid: NetUid::from(NETUID),
            recipient: buyer_h160().0,
            min_alpha: 0u64.into(),
            domain: BASE_DOMAIN,
        },
        nonce,
    }
    .to_wire()
}

fn sell_envelope(shares: u64, nonce: u64) -> Vec<u8> {
    GatewayEnvelope {
        asset: AssetId::Alpha(NetUid::from(NETUID)),
        amount: shares,
        dest: AccountId32::new([0u8; 32]),
        action: GatewayAction::SellShares {
            netuid: NetUid::from(NETUID),
            recipient: buyer_h160().0,
            usd_asset: USDC,
            min_usd: 0,
            domain: BASE_DOMAIN,
        },
        nonce,
    }
    .to_wire()
}

fn execute(amount: u64, env: &[u8]) {
    assert_ok!(pallet::Pallet::<Test>::do_gateway_execute(
        gateway_h160(),
        amount,
        env
    ));
    // Outbound messages are queued (inline dispatch would re-enter the EVM);
    // flush them the way the runtime does, via `on_idle` on an off-heartbeat
    // block.
    UsdPsm::on_idle(1, frame_support::weights::Weight::MAX);
}

fn hub_escrow_alpha() -> u64 {
    staked_alpha(
        &pallet::Pallet::<Test>::hub_account(),
        &escrow_hotkey(),
        NetUid::from(NETUID),
    )
}

#[test]
fn init_pool_moves_tao_and_sets_reserves() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        assert_eq!(PoolTaoReserve::<Test>::get(), 1_000_000_000_000);
        assert_eq!(PoolTUsdReserve::<Test>::get(), 100_000_000_000_000);
        assert_err!(
            UsdPsm::init_pool(RuntimeOrigin::root(), account(1), 1, 1, 0),
            UsdPsmError::PoolAlreadyInitialized
        );
    });
}

#[test]
fn internal_pool_swaps_round_trip() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        // Buy tUSD with 10 TAO: out = 100000e9 * 10e9 / (1000e9 + 10e9) ≈ 990.09 tUSD.
        let tusd =
            pallet::Pallet::<Test>::do_swap_tao_for_tusd(&account(2), 10_000_000_000, 0)
                .expect("swap works");
        assert_eq!(tusd, 990_099_009_900);
        assert_eq!(TUsdBalances::<Test>::get(account(2)), tusd);

        // Swap it back; with zero fees we get ~10 TAO back (minus rounding).
        let tao = pallet::Pallet::<Test>::do_swap_tusd_for_tao(&account(2), tusd, 9_990_000_000)
            .expect("swap back works");
        assert!(tao >= 9_990_000_000);
        assert_eq!(TUsdBalances::<Test>::get(account(2)), 0);

        // Slippage guard still applies internally.
        assert_err!(
            pallet::Pallet::<Test>::do_swap_tao_for_tusd(&account(2), 10_000_000_000, u64::MAX),
            UsdPsmError::SlippageExceeded
        );
    });
}

#[test]
fn gateway_execute_rejects_bad_callers_and_envelopes() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        let env = envelope(GatewayAction::CreditTUsd, 100, 0);

        // Wrong caller.
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(H160::zero(), 100, &env),
            UsdPsmError::NotGateway
        );
        // Amount mismatch.
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(gateway_h160(), 99, &env),
            UsdPsmError::AmountMismatch
        );
        // Garbage envelope.
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(gateway_h160(), 0, &[1, 2, 3]),
            UsdPsmError::BadEnvelope
        );
    });
}

#[test]
fn nonces_are_strictly_sequential() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();

        // Nonce 1 before nonce 0: out of order, delivery reverts.
        let early = envelope(GatewayAction::CreditTUsd, 100, 1);
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(gateway_h160(), 100, &early),
            UsdPsmError::NonceOutOfOrder
        );

        // Nonce 0 executes and advances the counter.
        let first = envelope(GatewayAction::CreditTUsd, 100, 0);
        execute(100, &first);
        assert_eq!(NextNonce::<Test>::get(), 1);

        // Replaying nonce 0 is rejected; nothing further is credited.
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(gateway_h160(), 100, &first),
            UsdPsmError::NonceReplayed
        );
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 100);

        // Now nonce 1 fills the gap and executes.
        execute(100, &early);
        assert_eq!(NextNonce::<Test>::get(), 2);
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 200);
    });
}

#[test]
fn gateway_execute_credit_records_receipt() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        let env = envelope(GatewayAction::CreditTUsd, 1_000_000_000, 0);
        execute(1_000_000_000, &env);
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 1_000_000_000);
        let receipt = ProcessedNonces::<Test>::get(0).expect("receipt stored");
        assert_eq!(receipt.fallback, None);
    });
}

#[test]
fn gateway_execute_stake_action_stakes() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        let hotkey = account(9);
        let env = envelope(
            GatewayAction::Stake {
                netuid: NetUid::from(NETUID),
                hotkey: AccountId32::new([9u8; 32]),
                min_alpha: 0u64.into(),
            },
            1_000_000_000,
            0,
        );
        execute(1_000_000_000, &env);
        // tUSD fully converted and staked.
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 0);
        assert!(staked_alpha(&account(7), &hotkey, NetUid::from(NETUID)) > 0);
        assert_eq!(ProcessedNonces::<Test>::get(0).unwrap().fallback, None);
    });
}

#[test]
fn gateway_execute_falls_back_to_tusd_on_action_failure() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        // Staking into the failing netuid: deposit must still land as tUSD.
        let env = envelope(
            GatewayAction::Stake {
                netuid: NetUid::from(FAILING_NETUID),
                hotkey: AccountId32::new([9u8; 32]),
                min_alpha: 0u64.into(),
            },
            1_000_000_000,
            0,
        );
        execute(1_000_000_000, &env);
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 1_000_000_000);
        assert!(ProcessedNonces::<Test>::get(0).unwrap().fallback.is_some());
    });
}

#[test]
fn gateway_execute_enforces_cap() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        let env = envelope(GatewayAction::CreditTUsd, 2_000_000_000_000, 0);
        // Cap is 1_000_000_000_000; delivery must revert (relayer retries).
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(gateway_h160(), 2_000_000_000_000, &env),
            UsdPsmError::CapExceeded
        );
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 0);
        assert!(ProcessedNonces::<Test>::get(0).is_none());
        assert_eq!(NextNonce::<Test>::get(), 0);
    });
}

#[test]
fn buy_shares_stakes_escrow_and_dispatches_mint() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        setup_outbound();

        let usd_in = 100_000_000_000; // 100 USD
        execute(usd_in, &buy_envelope(usd_in, 0));

        // First buy at index 1.0: shares == alpha staked in the escrow.
        let escrowed = hub_escrow_alpha();
        assert!(escrowed > 0);
        assert_eq!(SharesOutstanding::<Test>::get(NetUid::from(NETUID)), escrowed);
        assert_eq!(
            pallet::Pallet::<Test>::share_index_e9(NetUid::from(NETUID)),
            1_000_000_000
        );

        // No stray tUSD left on the buyer's mirror account.
        let mirror = pallet::Pallet::<Test>::evm_account(&buyer_h160());
        assert_eq!(TUsdBalances::<Test>::get(&mirror), 0);
        assert_eq!(ProcessedNonces::<Test>::get(0).unwrap().fallback, None);

        // Outbound mint: chutes route, share message with recipient/shares/index.
        let dispatches = outbound_dispatches();
        assert_eq!(dispatches.len(), 1);
        let (mailbox, sender, domain, recipient, body) = dispatches[0].clone();
        assert_eq!(mailbox, H160::from_low_u64_be(0x1111));
        assert_eq!(sender, pallet::Pallet::<Test>::hub_evm_address());
        assert_eq!(domain, BASE_DOMAIN);
        assert_eq!(recipient, chutes_recipient());
        // abi.encode(address, uint64, uint64): 3 words.
        assert_eq!(body.len(), 96);
        assert_eq!(&body[12..32], buyer_h160().as_bytes());
        assert_eq!(body[24 + 32..32 + 32], escrowed.to_be_bytes());
        assert_eq!(body[88..96], 1_000_000_000u64.to_be_bytes());
    });
}

#[test]
fn index_rises_with_emissions_and_prices_later_buys() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        setup_outbound();

        let usd_in = 100_000_000_000;
        execute(usd_in, &buy_envelope(usd_in, 0));
        let shares_before = SharesOutstanding::<Test>::get(NetUid::from(NETUID));

        // Emissions land on the escrow: +50% alpha.
        let hub = pallet::Pallet::<Test>::hub_account();
        set_staked_alpha(
            &hub,
            &escrow_hotkey(),
            NetUid::from(NETUID),
            hub_escrow_alpha() * 3 / 2,
        );
        let index = pallet::Pallet::<Test>::share_index_e9(NetUid::from(NETUID));
        assert_eq!(index, 1_500_000_000);
        let escrow_before_second_buy = hub_escrow_alpha();

        // A second identical buy mints alpha / 1.5 shares: the index prices
        // in the emissions earned by existing holders.
        execute(usd_in, &buy_envelope(usd_in, 1));
        let alpha_gained = hub_escrow_alpha() - escrow_before_second_buy;
        let minted = SharesOutstanding::<Test>::get(NetUid::from(NETUID)) - shares_before;
        assert!(minted > 0 && minted < alpha_gained);
        assert_eq!(
            minted,
            (u128::from(alpha_gained) * 1_000_000_000 / u128::from(index)) as u64
        );

        // The dispatched mint body carries exactly the minted shares.
        let dispatches = outbound_dispatches();
        let (_, _, _, _, body) = dispatches.last().unwrap().clone();
        let mut shares_bytes = [0u8; 8];
        shares_bytes.copy_from_slice(&body[56..64]);
        assert_eq!(u64::from_be_bytes(shares_bytes), minted);
    });
}

#[test]
fn buy_shares_failure_falls_back_to_mirror_credit() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        setup_outbound();
        // Point the escrow hotkey at the failing netuid's route by using a
        // buy on a netuid with no route: fallback credits the buyer's mirror.
        let env = GatewayEnvelope {
            asset: AssetId::Usd(USDC),
            amount: 50_000_000_000,
            dest: AccountId32::new([0u8; 32]),
            action: GatewayAction::BuyShares {
                netuid: NetUid::from(42), // no route, no escrow hotkey
                recipient: buyer_h160().0,
                min_alpha: 0u64.into(),
                domain: BASE_DOMAIN,
            },
            nonce: 0,
        }
        .to_wire();
        execute(50_000_000_000, &env);

        let mirror = pallet::Pallet::<Test>::evm_account(&buyer_h160());
        assert_eq!(TUsdBalances::<Test>::get(&mirror), 50_000_000_000);
        assert_eq!(
            ProcessedNonces::<Test>::get(0).unwrap().fallback,
            Some(FallbackReason::BuyFailed)
        );
        assert!(outbound_dispatches().is_empty());
    });
}

#[test]
fn sell_shares_unstakes_and_releases_usd() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        setup_outbound();

        let usd_in = 100_000_000_000;
        execute(usd_in, &buy_envelope(usd_in, 0));
        let shares = SharesOutstanding::<Test>::get(NetUid::from(NETUID));
        let reserves_before = pallet::Pallet::<Test>::psm_asset(USDC).unwrap().reserves;

        execute(shares, &sell_envelope(shares, 1));

        assert_eq!(SharesOutstanding::<Test>::get(NetUid::from(NETUID)), 0);
        assert_eq!(hub_escrow_alpha(), 0);
        assert_eq!(ProcessedNonces::<Test>::get(1).unwrap().fallback, None);

        // Reserves dropped by the released USD.
        let asset = pallet::Pallet::<Test>::psm_asset(USDC).unwrap();
        let released = reserves_before - asset.reserves;
        assert!(released > 0 && released <= usd_in);

        // Outbound release goes to the portal route with (address, uint64).
        let dispatches = outbound_dispatches();
        let (_, _, domain, recipient, body) = dispatches.last().unwrap().clone();
        assert_eq!(domain, BASE_DOMAIN);
        assert_eq!(recipient, portal_recipient());
        assert_eq!(body.len(), 64);
        assert_eq!(&body[12..32], buyer_h160().as_bytes());
        let mut amount_bytes = [0u8; 8];
        amount_bytes.copy_from_slice(&body[56..64]);
        assert_eq!(u64::from_be_bytes(amount_bytes), released);

        // The hub holds no residual tUSD.
        let hub = pallet::Pallet::<Test>::hub_account();
        assert_eq!(TUsdBalances::<Test>::get(&hub), 0);
    });
}

#[test]
fn sell_shares_failure_records_receipt_without_reverting() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        setup_outbound();
        // Nothing outstanding: the sell must fail but delivery must succeed.
        execute(1_000_000_000, &sell_envelope(1_000_000_000, 0));
        assert_eq!(
            ProcessedNonces::<Test>::get(0).unwrap().fallback,
            Some(FallbackReason::SellFailed)
        );
        assert!(outbound_dispatches().is_empty());
    });
}

#[test]
fn heartbeat_pushes_index_on_interval() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        setup_outbound();

        let usd_in = 100_000_000_000;
        execute(usd_in, &buy_envelope(usd_in, 0));
        let mints = outbound_dispatches().len();

        // Off-interval block: nothing pushed.
        System::set_block_number(11);
        UsdPsm::on_idle(11, frame_support::weights::Weight::MAX);
        assert_eq!(outbound_dispatches().len(), mints);

        // Emissions land, then an on-interval block pushes the new index.
        let hub = pallet::Pallet::<Test>::hub_account();
        set_staked_alpha(
            &hub,
            &escrow_hotkey(),
            NetUid::from(NETUID),
            hub_escrow_alpha() * 2,
        );
        System::set_block_number(20);
        UsdPsm::on_idle(20, frame_support::weights::Weight::MAX);
        let dispatches = outbound_dispatches();
        assert_eq!(dispatches.len(), mints + 1);
        let (_, _, _, recipient, body) = dispatches.last().unwrap().clone();
        assert_eq!(recipient, chutes_recipient());
        assert_eq!(body.len(), 96);
        // Zero address, zero shares, doubled index.
        assert_eq!(&body[12..32], H160::zero().as_bytes());
        let mut index_bytes = [0u8; 8];
        index_bytes.copy_from_slice(&body[88..96]);
        assert_eq!(u64::from_be_bytes(index_bytes), 2_000_000_000);

        // No shares outstanding => no heartbeat.
        SharesOutstanding::<Test>::remove(NetUid::from(NETUID));
        System::set_block_number(30);
        UsdPsm::on_idle(30, frame_support::weights::Weight::MAX);
        assert_eq!(outbound_dispatches().len(), mints + 1);
    });
}

#[test]
fn haircut_applies_on_gateway_deposit() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        assert_ok!(UsdPsm::register_usd_asset(
            RuntimeOrigin::root(),
            1,
            H160::from_low_u64_be(0xdddd),
            1_000_000_000_000,
            0,
            100, // 1% haircut
        ));
        let env = GatewayEnvelope {
            asset: AssetId::Usd(1),
            amount: 10_000_000_000,
            dest: account(7),
            action: GatewayAction::CreditTUsd,
            nonce: 0,
        }
        .to_wire();
        execute(10_000_000_000, &env);
        assert_eq!(TUsdBalances::<Test>::get(account(7)), 9_900_000_000);
    });
}

#[test]
fn disabled_asset_rejects_deposits() {
    new_test_ext().execute_with(|| {
        setup_psm_and_pool();
        assert_ok!(UsdPsm::set_asset_enabled(RuntimeOrigin::root(), USDC, false));
        let env = envelope(GatewayAction::CreditTUsd, 1_000, 0);
        assert_err!(
            pallet::Pallet::<Test>::do_gateway_execute(gateway_h160(), 1_000, &env),
            UsdPsmError::AssetDisabled
        );
    });
}
