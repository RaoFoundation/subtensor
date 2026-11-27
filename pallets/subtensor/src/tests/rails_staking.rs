#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::{assert_noop, assert_ok, weights::Weight};
use sp_core::{H160, U256};
use subtensor_runtime_common::{
    NetUid, TaoBalance,
    rails::{BridgedPayment, OriginChain, Payer, RailsStaking, SubnetStatus},
};

use super::gamma::{OWNER_COLDKEY, OWNER_HOTKEY, TAO, gamma_subnet};
use super::mock::*;
use crate::weights::WeightInfo;
use crate::*;

/// The hub's keyless account that stakes for a subnet.
const VAULT: u64 = 9_000;
/// The service that `gamma_subnet` lists.
const SERVICE: u64 = 7_000;

/// The hub's view of staking.
type Staking = SubtensorModule;

fn vault() -> U256 {
    U256::from(VAULT)
}

fn fund_vault(tao: u64) {
    add_balance_to_coldkey_account(&vault(), TaoBalance::from(tao));
}

fn vault_balance() -> u64 {
    SubtensorModule::get_coldkey_balance(&vault()).into()
}

fn existential_deposit() -> u64 {
    ExistentialDeposit::get().into()
}

/// A subnet with a deep pool, and its index hotkey open for the vault.
fn subnet_with_index() -> NetUid {
    let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
    setup_reserves(netuid, (1_000_000 * TAO).into(), (1_000_000 * TAO).into());
    assert_ok!(<Staking as RailsStaking<U256>>::open_index(
        netuid,
        &vault()
    ));
    netuid
}

fn staked(netuid: NetUid) -> u64 {
    <Staking as RailsStaking<U256>>::staked(&vault(), netuid).into()
}

/// Runs the cleanup of dissolved subnets to its end.
fn finish_dissolution() {
    for _ in 0..64 {
        if DissolveCleanupQueue::<Test>::get().is_empty() {
            return;
        }
        SubtensorModule::on_idle(0, Weight::from_parts(u64::MAX, u64::MAX));
    }
    panic!("dissolution did not finish");
}

#[test]
fn subnet_status_follows_the_life_of_a_netuid() {
    new_test_ext(1).execute_with(|| {
        let status = <Staking as RailsStaking<U256>>::subnet_status;
        assert_eq!(status(NetUid::ROOT), SubnetStatus::Absent);
        assert_eq!(status(NetUid::from(77)), SubnetStatus::Absent);

        let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
        let SubnetStatus::Live { generation } = status(netuid) else {
            panic!("the subnet does not live");
        };

        // While the stake of a dissolved subnet is paid out, the netuid is neither.
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert_eq!(status(netuid), SubnetStatus::Dissolving);
        finish_dissolution();
        assert_eq!(status(netuid), SubnetStatus::Absent);

        // Another subnet at the same netuid is another generation.
        let again = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
        assert_eq!(again, netuid);
        assert_eq!(
            status(netuid),
            SubnetStatus::Live {
                generation: generation + 1
            }
        );
    });
}

#[test]
fn subnet_name_is_the_published_identity() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        assert_eq!(<Staking as RailsStaking<U256>>::subnet_name(netuid), None);
        SubnetIdentitiesV3::<Test>::insert(
            netuid,
            SubnetIdentityOfV3 {
                subnet_name: b"Chutes".to_vec(),
                ..Default::default()
            },
        );
        assert_eq!(
            <Staking as RailsStaking<U256>>::subnet_name(netuid),
            Some(b"Chutes".to_vec())
        );
    });
}

#[test]
fn a_vault_stakes_at_the_index_hotkey_and_is_told_what_it_got() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet_with_index();
        let index = SubtensorModule::index_hotkey(netuid);
        assert_eq!(Owner::<Test>::get(index), vault());

        // The first stake leaves the existential deposit in the vault.
        fund_vault(100 * TAO);
        let first =
            <Staking as RailsStaking<U256>>::stake(&vault(), netuid, (100 * TAO).into()).unwrap();
        assert!(u64::from(first) > 99 * TAO);
        assert_eq!(staked(netuid), u64::from(first));
        assert_eq!(vault_balance(), existential_deposit());

        // Later stakes use all they are given. The answer is the growth of the position.
        fund_vault(50 * TAO);
        let second =
            <Staking as RailsStaking<U256>>::stake(&vault(), netuid, (50 * TAO).into()).unwrap();
        assert_eq!(staked(netuid), u64::from(first) + u64::from(second));
        assert_eq!(vault_balance(), existential_deposit());
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&index, &vault(), netuid),
            (u64::from(first) + u64::from(second)).into()
        );
    });
}

#[test]
fn a_vault_unstakes_exactly_what_it_asks_for() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet_with_index();
        fund_vault(100 * TAO);
        <Staking as RailsStaking<U256>>::stake(&vault(), netuid, (100 * TAO).into()).unwrap();
        let held = staked(netuid);
        let balance = vault_balance();

        let tao =
            <Staking as RailsStaking<U256>>::unstake(&vault(), netuid, (held / 4).into()).unwrap();

        // The answer is the TAO that reached the vault's balance.
        assert_eq!(vault_balance(), balance + u64::from(tao));
        assert!(u64::from(tao) > 24 * TAO && u64::from(tao) < 25 * TAO);
        assert!(staked(netuid).abs_diff(held - held / 4) <= 1);

        // The extrinsic lowers a request to what is staked. The hub is refused instead: it
        // must not hand out shares' worth of TAO that it did not get.
        let held = staked(netuid);
        assert_noop!(
            <Staking as RailsStaking<U256>>::unstake(&vault(), netuid, (held + 1).into()),
            Error::<Test>::NotEnoughStakeToWithdraw
        );
        // It can take everything out.
        assert_ok!(<Staking as RailsStaking<U256>>::unstake(
            &vault(),
            netuid,
            held.into()
        ));
        assert_eq!(staked(netuid), 0);
    });
}

#[test]
fn staking_needs_an_open_index() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
        setup_reserves(netuid, (1_000_000 * TAO).into(), (1_000_000 * TAO).into());
        fund_vault(100 * TAO);
        assert_noop!(
            <Staking as RailsStaking<U256>>::stake(&vault(), netuid, (100 * TAO).into()),
            Error::<Test>::HotKeyAccountNotExists
        );
        // Root has no index.
        assert_noop!(
            <Staking as RailsStaking<U256>>::open_index(NetUid::ROOT, &vault()),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn a_dissolved_subnet_pays_the_vaults_stake_to_the_vault_in_tao() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet_with_index();
        fund_vault(100 * TAO);
        <Staking as RailsStaking<U256>>::stake(&vault(), netuid, (100 * TAO).into()).unwrap();
        assert!(IndexSubnets::<Test>::get().contains(&netuid));
        let balance = vault_balance();

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        // The index closes with the subnet.
        assert!(!IndexSubnets::<Test>::get().contains(&netuid));
        assert!(!IndexRefreshedAt::<Test>::contains_key(netuid));
        finish_dissolution();

        // The stake is gone and its worth is in the vault account, where the hub's wind-down
        // divides it among the shares.
        assert_eq!(staked(netuid), 0);
        assert!(vault_balance() > balance + TAO, "{}", vault_balance());
    });
}

#[test]
fn a_spend_for_a_bridged_payer_goes_through_the_credits_path() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let payer = Payer::Evm(H160::repeat_byte(0xAB));
        fund_vault(5 * TAO);

        let (alpha, gamma) = <Staking as RailsStaking<U256>>::spend_at_service(
            &vault(),
            netuid,
            &U256::from(SERVICE),
            (5 * TAO).into(),
            u64::MAX,
            BridgedPayment {
                payer: payer.clone(),
                origin_chain: OriginChain::Evm(8453),
                origin_nonce: 3,
            },
        )
        .unwrap();

        assert!(u64::from(alpha) > 4 * TAO);
        assert_eq!(
            GammaSpentByPayer::<Test>::get((netuid, U256::from(SERVICE), payer)),
            gamma
        );
        // The account that pays keeps its existential deposit, and nothing else.
        assert_eq!(vault_balance(), existential_deposit());
    });
}

#[test]
fn the_weight_covers_the_staking_of_any_route() {
    new_test_ext(1).execute_with(|| {
        let weight = <Staking as RailsStaking<U256>>::weight();
        let unstake = <Test as Config>::WeightInfo::remove_stake();
        for then in [
            <Test as Config>::WeightInfo::add_stake(),
            <Test as Config>::WeightInfo::mint_gamma(),
        ] {
            assert!(weight.all_gte(unstake.saturating_add(then)));
        }
    });
}
