#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_noop, assert_ok, storage::with_storage_layer, weights::WeightMeter};
use sp_core::{H160, U256};
use subtensor_runtime_common::{
    GammaBalance, NetUid, TaoBalance,
    rails::{BridgedPayment, OriginChain, Payer, SpenderKind},
};

use super::gamma::{GAMMA, TAO, gamma_subnet};
use super::mock::*;
use crate::*;

/// The account a bridge holds TAO in while it settles a payment.
const FUNDER: u64 = 8_000;
/// The service that `gamma_subnet` lists.
const SERVICE: u64 = 7_000;
/// Base.
const CHAIN: OriginChain = OriginChain::Evm(8453);
/// 400 dollars per TAO, as `gamma_subnet` feeds.
const PRICE: u64 = 400 * GAMMA;

fn payer(byte: u8) -> Payer<U256> {
    Payer::Evm(H160::repeat_byte(byte))
}

fn payment(byte: u8, origin_nonce: u64) -> BridgedPayment<U256> {
    BridgedPayment {
        payer: payer(byte),
        origin_chain: CHAIN,
        origin_nonce,
    }
}

fn spent(netuid: NetUid, service: u64, byte: u8) -> GammaBalance {
    GammaSpentByPayer::<Test>::get((netuid, U256::from(service), payer(byte)))
}

/// Spends `tao` from a freshly funded `FUNDER` at `SERVICE` for the payer `byte`.
fn spend(
    netuid: NetUid,
    tao: u64,
    cap: u64,
    byte: u8,
) -> Result<(TaoBalance, AlphaBalance, GammaBalance), sp_runtime::DispatchError> {
    add_balance_to_coldkey_account(&U256::from(FUNDER), TaoBalance::from(tao));
    with_storage_layer(|| {
        SubtensorModule::do_spend_tao_at_service(
            &U256::from(FUNDER),
            netuid,
            &U256::from(SERVICE),
            tao.into(),
            cap,
            payment(byte, 7),
        )
    })
}

#[test]
fn spending_tao_at_a_service_burns_alpha_and_credits_the_payer() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let burned = pallet_alpha_assets::AlphaBurned::<Test>::get(netuid);

        let (tao, alpha, gamma) = spend(netuid, 10 * TAO, u64::MAX, 1).unwrap();

        // The funder paid the TAO, but for what the pool could not take, and holds no stake
        // and no deposit for it.
        assert!(tao <= TaoBalance::from(10 * TAO) && tao >= TaoBalance::from(10 * TAO - 1));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&U256::from(FUNDER)),
            TaoBalance::from(10 * TAO) - tao
        );
        assert_eq!(GammaDeposits::<Test>::iter_prefix(netuid).count(), 0);
        assert!(StakingHotkeys::<Test>::get(U256::from(FUNDER)).is_empty());
        // The alpha it bought is burned.
        assert!(alpha > AlphaBalance::ZERO);
        assert_eq!(
            pallet_alpha_assets::AlphaBurned::<Test>::get(netuid),
            burned + alpha
        );
        // About 10 TAO at 400 dollars, less the pool's slippage and fee.
        assert!(
            gamma > GammaBalance::from(3_900 * GAMMA) && gamma < GammaBalance::from(4_000 * GAMMA)
        );
        // The payer's total at the service grew by it; the hotkey map is untouched.
        assert_eq!(spent(netuid, SERVICE, 1), gamma);
        assert_eq!(GammaSpent::<Test>::iter_prefix((netuid,)).count(), 0);
        System::assert_has_event(
            Event::GammaSpentV2 {
                netuid,
                service: U256::from(SERVICE),
                payer: payer(1),
                origin_chain: CHAIN,
                spender_kind: SpenderKind::Bridge { chain: CHAIN },
                amount: gamma,
                origin_nonce: Some(7),
            }
            .into(),
        );
    });
}

/// A bridged spend moves the subnet's pool, burn counter, flow and fees exactly as a mint
/// of the same TAO does, and is worth the same gamma.
#[test]
fn a_bridged_spend_trades_and_is_valued_like_a_mint() {
    let run = |bridged: bool| {
        new_test_ext(1).execute_with(|| {
            let netuid = gamma_subnet();
            let coldkey = U256::from(FUNDER);
            let author = U256::from(MOCK_BLOCK_BUILDER);
            let author_before = SubtensorModule::get_coldkey_balance(&author);
            let flow_before = SubnetTaoFlow::<Test>::get(netuid);
            let gamma = if bridged {
                spend(netuid, 100 * TAO, u64::MAX, 1).unwrap().2
            } else {
                let hotkey = U256::from(8_001);
                add_balance_to_coldkey_account(&coldkey, TaoBalance::from(100 * TAO));
                assert_ok!(SubtensorModule::mint_gamma(
                    RuntimeOrigin::signed(coldkey),
                    hotkey,
                    netuid,
                    (100 * TAO).into(),
                    GammaBalance::from(0),
                    None
                ));
                GammaDeposits::<Test>::get(netuid, hotkey).unwrap().balance
            };
            (
                gamma,
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                SubnetAlphaOut::<Test>::get(netuid),
                TotalStake::<Test>::get(),
                SubnetVolume::<Test>::get(netuid),
                pallet_alpha_assets::AlphaBurned::<Test>::get(netuid),
                SubnetTaoFlow::<Test>::get(netuid) - flow_before,
                SubtensorModule::get_coldkey_balance(&author) - author_before,
                SubtensorModule::get_coldkey_balance(&coldkey),
            )
        })
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn a_cap_at_or_above_the_oracle_price_changes_nothing() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let gamma_at = |cap: u64| {
            let mut gamma = GammaBalance::from(0);
            // Each probe is rolled back, so all see the same pool.
            let _ = with_storage_layer(|| {
                gamma = spend(netuid, 10 * TAO, cap, 1)?.2;
                Err::<(), sp_runtime::DispatchError>("probe".into())
            });
            gamma
        };
        assert_eq!(gamma_at(PRICE), gamma_at(u64::MAX));
        assert_eq!(gamma_at(PRICE + 1), gamma_at(u64::MAX));
        assert!(gamma_at(PRICE - PRICE / 100) < gamma_at(u64::MAX));
    });
}

#[test]
fn a_cheaper_market_price_caps_the_gamma() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let gamma_at = |cap: u64| {
            let mut gamma = GammaBalance::from(0);
            // Each probe is rolled back, so all see the same pool.
            let _ = with_storage_layer(|| {
                gamma = spend(netuid, 10 * TAO, cap, 1)?.2;
                Err::<(), sp_runtime::DispatchError>("probe".into())
            });
            u64::from(gamma)
        };
        let at_oracle = gamma_at(u64::MAX);
        // TAO bought at 300 dollars mints three quarters of the credit.
        assert_eq!(gamma_at(300 * GAMMA), at_oracle / 4 * 3);
        assert_eq!(gamma_at(200 * GAMMA), at_oracle / 2);
        // A dearer market mints no more than the oracle allows.
        assert_eq!(gamma_at(900 * GAMMA), at_oracle);
        assert_eq!(spent(netuid, SERVICE, 1), GammaBalance::from(0));
    });
}

#[test]
fn totals_grow_per_payer_and_service() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (_, _, first) = spend(netuid, 5 * TAO, u64::MAX, 1).unwrap();
        let (_, _, second) = spend(netuid, 5 * TAO, u64::MAX, 1).unwrap();
        let (_, _, other) = spend(netuid, 5 * TAO, u64::MAX, 2).unwrap();
        assert_eq!(spent(netuid, SERVICE, 1), first + second);
        assert_eq!(spent(netuid, SERVICE, 2), other);
        assert_eq!(spent(netuid, SERVICE + 1, 1), GammaBalance::from(0));
        // The same address on another chain is the same payer.
        add_balance_to_coldkey_account(&U256::from(FUNDER), TaoBalance::from(5 * TAO));
        let (_, _, ethereum) = SubtensorModule::do_spend_tao_at_service(
            &U256::from(FUNDER),
            netuid,
            &U256::from(SERVICE),
            (5 * TAO).into(),
            u64::MAX,
            BridgedPayment {
                payer: payer(1),
                origin_chain: OriginChain::Evm(1),
                origin_nonce: 0,
            },
        )
        .unwrap();
        assert_eq!(spent(netuid, SERVICE, 1), first + second + ethereum);
    });
}

#[test]
fn bridged_spend_refusals() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let funder = U256::from(FUNDER);
        let service = U256::from(SERVICE);
        add_balance_to_coldkey_account(&funder, TaoBalance::from(10 * TAO));
        let try_spend = |netuid: NetUid, service: U256, tao: u64| {
            SubtensorModule::do_spend_tao_at_service(
                &funder,
                netuid,
                &service,
                tao.into(),
                u64::MAX,
                payment(1, 0),
            )
        };

        assert_noop!(
            try_spend(NetUid::from(99), service, TAO),
            Error::<Test>::SubnetNotExists
        );
        assert_noop!(
            try_spend(netuid, U256::from(SERVICE + 1), TAO),
            Error::<Test>::ServiceDoesNotExist
        );
        assert_noop!(
            try_spend(netuid, service, 11 * TAO),
            Error::<Test>::NotEnoughBalanceToStake
        );
        // Less than a tenth of a gamma.
        assert_noop!(
            with_storage_layer(|| try_spend(netuid, service, 200_000)),
            Error::<Test>::AmountTooLow
        );

        SubnetGammaEnabled::<Test>::insert(netuid, false);
        assert_noop!(
            try_spend(netuid, service, TAO),
            Error::<Test>::GammaDisabled
        );
        SubnetGammaEnabled::<Test>::insert(netuid, true);

        // A stale oracle price values nothing.
        System::set_block_number(System::block_number() + TAO_USD_PRICE_MAX_AGE + 1);
        assert_noop!(
            with_storage_layer(|| try_spend(netuid, service, TAO)),
            Error::<Test>::TaoUsdPriceUnavailable
        );
        assert_eq!(spent(netuid, SERVICE, 1), GammaBalance::from(0));
    });
}

#[test]
fn dissolution_clears_payer_totals() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        spend(netuid, 5 * TAO, u64::MAX, 1).unwrap();
        spend(netuid, 5 * TAO, u64::MAX, 2).unwrap();
        assert_eq!(GammaSpentByPayer::<Test>::iter_prefix((netuid,)).count(), 2);

        let mut meter = WeightMeter::new();
        assert!(SubtensorModule::remove_network_map_parameters(
            netuid, &mut meter
        ));

        assert_eq!(GammaSpentByPayer::<Test>::iter_prefix((netuid,)).count(), 0);
    });
}
