#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok, weights::Weight};
use sp_core::U256;
use sp_runtime::PerU16;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};

use super::mock::*;
use crate::staking::index_hotkey::{
    MAX_INDEX_CANDIDATES, MAX_INDEX_CHILD_PROPORTION, MAX_INDEX_CHILDREN,
};
use crate::*;

const TAO: u64 = 1_000_000_000;
/// The keyless account that holds stake at an index for others.
const VAULT: u64 = 9_000;

fn subnet() -> NetUid {
    let netuid = NetUid::from(1);
    add_network(netuid, 10, 0);
    setup_reserves(netuid, (1_000_000 * TAO).into(), (1_000_000 * TAO).into());
    netuid
}

/// Gives `netuid` an epoch result: UID `i` is hotkey `1_000 + i` with `dividends[i]` and a
/// validator permit unless its dividend is zero.
fn epoch_result(netuid: NetUid, dividends: &[u16]) {
    for (uid, _) in dividends.iter().enumerate() {
        Keys::<Test>::insert(netuid, uid as u16, validator(uid));
    }
    ValidatorPermit::<Test>::insert(netuid, vec![true; dividends.len()]);
    Dividends::<Test>::insert(
        netuid,
        dividends
            .iter()
            .map(|dividend| PerU16::from_parts(*dividend))
            .collect::<Vec<_>>(),
    );
}

fn validator(uid: usize) -> U256 {
    U256::from(1_000 + uid as u64)
}

fn children_of_index(netuid: NetUid) -> Vec<(u64, U256)> {
    ChildKeys::<Test>::get(SubtensorModule::index_hotkey(netuid), netuid)
}

fn idle() {
    SubtensorModule::on_idle(
        System::block_number(),
        Weight::from_parts(u64::MAX, u64::MAX),
    );
}

// ---------------------------------------------------------------- identity

#[test]
fn index_hotkey_is_a_keyless_account_of_its_subnet() {
    new_test_ext(1).execute_with(|| {
        let one = SubtensorModule::index_hotkey(NetUid::from(1));
        let two = SubtensorModule::index_hotkey(NetUid::from(2));
        assert_ne!(one, two);
        assert_eq!(
            SubtensorModule::index_hotkey_netuid(&one),
            Some(NetUid::from(1))
        );
        assert_eq!(
            SubtensorModule::index_hotkey_netuid(&two),
            Some(NetUid::from(2))
        );
        for netuid in [0u16, 1, 255, 256, u16::MAX] {
            let index = SubtensorModule::index_hotkey(NetUid::from(netuid));
            assert_eq!(
                SubtensorModule::index_hotkey_netuid(&index),
                Some(NetUid::from(netuid))
            );
            assert!(SubtensorModule::is_system_hotkey(&index));
            // It is no other account of the pallet.
            assert_eq!(SubtensorModule::is_subnet_account_id(&index), None);
            assert_ne!(index, SubtensorModule::get_beta_escrow_account_id());
        }

        // Ordinary accounts, and the pallet's other accounts, are not index hotkeys.
        for account in [
            U256::from(0),
            U256::from(1),
            U256::MAX,
            SubtensorModule::get_beta_escrow_account_id(),
        ] {
            assert!(!SubtensorModule::is_index_hotkey(&account));
        }
        add_network(NetUid::from(1), 10, 0);
        let subnet_account = SubtensorModule::get_subnet_account_id(NetUid::from(1)).unwrap();
        assert!(!SubtensorModule::is_index_hotkey(&subnet_account));
        assert!(SubtensorModule::is_system_hotkey(&subnet_account));
    });
}

#[test]
fn nobody_registers_or_swaps_to_an_index_hotkey() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        let index = SubtensorModule::index_hotkey(netuid);
        let coldkey = U256::from(1);
        add_balance_to_coldkey_account(&coldkey, (1_000 * TAO).into());

        assert_noop!(
            SubtensorModule::create_account_if_non_existent(&coldkey, &index),
            Error::<Test>::CannotUseSystemAccount
        );
        assert_noop!(
            SubtensorModule::set_hotkey_owner(&coldkey, &index),
            Error::<Test>::CannotUseSystemAccount
        );
        assert_eq!(
            SubtensorModule::burned_register(RuntimeOrigin::signed(coldkey), netuid, index)
                .unwrap_err()
                .error,
            Error::<Test>::CannotUseSystemAccount.into()
        );
        assert_noop!(
            SubtensorModule::set_subnet_owner_hotkey(netuid, &index),
            Error::<Test>::CannotUseSystemAccount
        );
        assert!(!Owner::<Test>::contains_key(index));
    });
}

// ---------------------------------------------------------------- opening

#[test]
fn opening_an_index_gives_it_an_owner_and_no_take() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        let index = SubtensorModule::index_hotkey(netuid);
        let vault = U256::from(VAULT);

        assert_ok!(SubtensorModule::do_open_index(netuid, &vault));

        assert_eq!(Owner::<Test>::get(index), vault);
        assert_eq!(OwnedHotkeys::<Test>::get(vault), vec![index]);
        assert!(SubtensorModule::hotkey_account_exists(&index));
        assert_eq!(Delegates::<Test>::get(index), PerU16::zero());
        assert_eq!(IndexSubnets::<Test>::get().to_vec(), vec![netuid]);
        assert_eq!(IndexRefreshedAt::<Test>::get(netuid), 1);

        // Opening it again changes nothing; another owner cannot take it.
        assert_ok!(SubtensorModule::do_open_index(netuid, &vault));
        assert_eq!(OwnedHotkeys::<Test>::get(vault), vec![index]);
        assert_eq!(IndexSubnets::<Test>::get().len(), 1);
        assert_noop!(
            SubtensorModule::do_open_index(netuid, &U256::from(VAULT + 1)),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn an_index_opens_only_on_an_existing_subnet() {
    new_test_ext(1).execute_with(|| {
        let vault = U256::from(VAULT);
        assert_noop!(
            SubtensorModule::do_open_index(NetUid::from(7), &vault),
            Error::<Test>::SubnetNotExists
        );
        assert_noop!(
            SubtensorModule::do_open_index(NetUid::ROOT, &vault),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn index_take_is_zero_whatever_storage_says() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        let index = SubtensorModule::index_hotkey(netuid);
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        Delegates::<Test>::insert(index, PerU16::from_percent(18));
        assert_eq!(SubtensorModule::get_hotkey_take(&index), 0);
        // An ordinary hotkey is unaffected.
        Delegates::<Test>::insert(U256::from(5), PerU16::from_percent(18));
        assert_eq!(
            SubtensorModule::get_hotkey_take(&U256::from(5)),
            PerU16::from_percent(18).deconstruct()
        );
    });
}

// ---------------------------------------------------------------- the rule

#[test]
fn rule_lends_equally_to_the_eight_validators_with_the_highest_dividends() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        // Twelve validators; UIDs 3 and 9 tie.
        epoch_result(
            netuid,
            &[50, 900, 300, 700, 100, 800, 10, 600, 500, 700, 400, 200],
        );

        let children = SubtensorModule::index_children(netuid);

        let expected: Vec<U256> = [1, 5, 3, 9, 7, 8, 10, 2].map(validator).to_vec();
        assert_eq!(
            children.iter().map(|(_, child)| *child).collect::<Vec<_>>(),
            expected
        );
        assert!(children.iter().all(|(share, _)| *share == u64::MAX / 8));
        let lent: u128 = children.iter().map(|(share, _)| u128::from(*share)).sum();
        assert!(lent <= u128::from(u64::MAX) && lent > u128::from(u64::MAX) - 8);
    });
}

#[test]
fn rule_skips_validators_without_permit_or_dividend() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        epoch_result(netuid, &[900, 0, 800, 700, 600]);
        // UID 2 earned a dividend but lost its permit.
        ValidatorPermit::<Test>::insert(netuid, vec![true, true, false, true, true]);

        let children = SubtensorModule::index_children(netuid);

        assert_eq!(
            children.iter().map(|(_, child)| *child).collect::<Vec<_>>(),
            [0, 3, 4].map(validator).to_vec()
        );
    });
}

#[test]
fn rule_caps_each_child_at_a_quarter() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        for count in 1..=MAX_INDEX_CHILDREN + 2 {
            epoch_result(netuid, &vec![100; count]);
            let children = SubtensorModule::index_children(netuid);
            let kept = count.min(MAX_INDEX_CHILDREN);
            assert_eq!(children.len(), kept);
            let share = (u64::MAX / kept as u64).min(MAX_INDEX_CHILD_PROPORTION);
            assert!(children.iter().all(|(given, _)| *given == share));
            assert!(share <= MAX_INDEX_CHILD_PROPORTION);
        }
        // With no validator there is nobody to lend to.
        epoch_result(netuid, &[]);
        assert!(SubtensorModule::index_children(netuid).is_empty());
    });
}

#[test]
fn rule_skips_validators_whose_childkey_take_exceeds_the_delegate_ceiling() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        epoch_result(netuid, &[900, 800, 700]);
        SubtensorModule::set_max_delegate_take(PerU16::from_percent(9));
        SubtensorModule::set_max_childkey_take(PerU16::from_percent(18));
        ChildkeyTake::<Test>::insert(validator(0), netuid, PerU16::from_percent(18));
        ChildkeyTake::<Test>::insert(validator(1), netuid, PerU16::from_percent(9));

        let children = SubtensorModule::index_children(netuid);

        assert_eq!(
            children.iter().map(|(_, child)| *child).collect::<Vec<_>>(),
            [1, 2].map(validator).to_vec()
        );
    });
}

#[test]
fn rule_skips_validators_that_cannot_take_another_parent() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        epoch_result(netuid, &[900, 800]);
        for parent in 1..=100u64 {
            SubtensorModule::persist_pending_chidren_ok(
                netuid,
                &U256::from(parent),
                &vec![(1, validator(0))],
            );
        }

        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));

        assert_eq!(
            children_of_index(netuid),
            vec![(MAX_INDEX_CHILD_PROPORTION, validator(1))]
        );
    });
}

#[test]
fn rule_examines_a_bounded_number_of_validators() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        // Far more validators than the rule looks at, all barred but the last few by take.
        let count = MAX_INDEX_CANDIDATES + 20;
        epoch_result(netuid, &vec![100; count]);
        SubtensorModule::set_max_delegate_take(PerU16::from_percent(9));
        SubtensorModule::set_max_childkey_take(PerU16::from_percent(18));
        for uid in 0..count - 4 {
            ChildkeyTake::<Test>::insert(validator(uid), netuid, PerU16::from_percent(18));
        }
        // The eligible ones lie beyond the candidates it walks: it gives up, it does not scan.
        assert!(SubtensorModule::index_children(netuid).is_empty());
    });
}

// ---------------------------------------------------------------- children

#[test]
fn an_index_has_eight_children_and_an_ordinary_hotkey_five() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        epoch_result(netuid, &[100; 10]);
        let index = SubtensorModule::index_hotkey(netuid);

        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));

        let children = children_of_index(netuid);
        assert_eq!(children.len(), MAX_INDEX_CHILDREN);
        for (share, child) in &children {
            assert_eq!(*share, u64::MAX / 8);
            assert_eq!(
                ParentKeys::<Test>::get(child, netuid),
                vec![(*share, index)]
            );
        }
        assert_eq!(SubtensorModule::max_children(&index), 8);

        // An ordinary parent is still held to five.
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        register_ok_neuron(netuid, parent, coldkey, 0);
        assert_eq!(SubtensorModule::max_children(&parent), 5);
        let six: Vec<(u64, U256)> = (0..6).map(|uid| (1, validator(uid))).collect();
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                parent,
                netuid,
                six
            ),
            Error::<Test>::TooManyChildren
        );
    });
}

#[test]
fn an_index_is_nobodys_child() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        let index = SubtensorModule::index_hotkey(netuid);
        let coldkey = U256::from(1);
        let parent = U256::from(2);
        register_ok_neuron(netuid, parent, coldkey, 0);
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(coldkey),
                parent,
                netuid,
                vec![(u64::MAX, index)]
            ),
            Error::<Test>::InvalidChild
        );
        // Not even the account that owns it sets its children by hand.
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        assert_noop!(
            SubtensorModule::do_schedule_children(
                RuntimeOrigin::signed(U256::from(VAULT)),
                index,
                netuid,
                vec![(u64::MAX, parent)]
            ),
            Error::<Test>::InvalidChild
        );
    });
}

#[test]
fn refresh_replaces_children_that_fell_out_of_the_rule() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        let index = SubtensorModule::index_hotkey(netuid);
        epoch_result(netuid, &[900, 800, 700, 600]);
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        assert_eq!(children_of_index(netuid).len(), 4);

        // UID 0 stops earning; UID 4 appears.
        epoch_result(netuid, &[0, 800, 700, 600, 500]);
        SubtensorModule::refresh_index(netuid);

        let children: Vec<U256> = children_of_index(netuid)
            .into_iter()
            .map(|(_, child)| child)
            .collect();
        assert_eq!(children.len(), 4);
        assert!(!children.contains(&validator(0)));
        assert!(children.contains(&validator(4)));
        assert!(ParentKeys::<Test>::get(validator(0), netuid).is_empty());
        assert_eq!(
            ParentKeys::<Test>::get(validator(4), netuid),
            vec![(MAX_INDEX_CHILD_PROPORTION, index)]
        );
    });
}

#[test]
fn refresh_runs_on_idle_once_the_cooldown_has_passed() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        PendingChildKeyCooldown::<Test>::put(20);
        epoch_result(netuid, &[900, 800, 700, 600]);
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        let before = children_of_index(netuid);

        // The epoch result changes, but the cooldown has not passed.
        epoch_result(netuid, &[0, 800, 700, 600, 500]);
        System::set_block_number(20);
        idle();
        assert_eq!(children_of_index(netuid), before);
        assert_eq!(IndexRefreshedAt::<Test>::get(netuid), 1);

        System::set_block_number(21);
        idle();
        assert_ne!(children_of_index(netuid), before);
        assert_eq!(IndexRefreshedAt::<Test>::get(netuid), 21);
    });
}

#[test]
fn each_index_takes_its_turn_on_idle() {
    new_test_ext(1).execute_with(|| {
        PendingChildKeyCooldown::<Test>::put(0);
        let netuids: Vec<NetUid> = (1..=3u16).map(NetUid::from).collect();
        for netuid in &netuids {
            add_network(*netuid, 10, 0);
            epoch_result(*netuid, &[900, 800, 700, 600]);
            assert_ok!(SubtensorModule::do_open_index(*netuid, &U256::from(VAULT)));
        }
        // One subnet a block, by the block number.
        for block in 30..36u64 {
            System::set_block_number(block);
            idle();
            let turn = netuids[(block % 3) as usize];
            assert_eq!(IndexRefreshedAt::<Test>::get(turn), block);
        }
    });
}

#[test]
fn idle_does_nothing_without_weight_or_without_an_index() {
    new_test_ext(1).execute_with(|| {
        // No index is open: one read.
        assert_eq!(
            SubtensorModule::process_index_refresh(Weight::from_parts(u64::MAX, u64::MAX)),
            <Test as frame_system::Config>::DbWeight::get().reads(1)
        );
        let netuid = subnet();
        PendingChildKeyCooldown::<Test>::put(0);
        epoch_result(netuid, &[900, 800, 700, 600]);
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        System::set_block_number(50);
        assert_eq!(
            SubtensorModule::process_index_refresh(Weight::zero()),
            Weight::zero()
        );
        assert_eq!(IndexRefreshedAt::<Test>::get(netuid), 1);
    });
}

#[test]
fn the_index_of_a_dissolved_subnet_is_closed() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        epoch_result(netuid, &[900, 800, 700, 600]);
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        NetworksAdded::<Test>::remove(netuid);

        System::set_block_number(2);
        idle();

        assert!(IndexSubnets::<Test>::get().is_empty());
        assert!(!IndexRefreshedAt::<Test>::contains_key(netuid));
    });
}

#[test]
fn an_index_is_exempt_from_the_stake_threshold() {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet();
        let index = SubtensorModule::index_hotkey(netuid);
        epoch_result(netuid, &[900, 800, 700, 600]);
        assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
        SubtensorModule::set_stake_threshold(1_000_000 * TAO);

        // Neither the queued check nor a direct one suspends it.
        SubtensorModule::queue_childkey_threshold_check(&index);
        assert!(!ChildkeyThresholdChecks::<Test>::contains_key(index));
        SubtensorModule::recheck_childkey_threshold(&index);
        assert!(!ChildkeyThresholdSuspended::<Test>::contains_key(index));
        assert_eq!(SubtensorModule::get_children(&index, netuid).len(), 4);

        // An ordinary parent below the threshold is suspended.
        let parent = U256::from(77);
        SubtensorModule::persist_pending_chidren_ok(netuid, &parent, &vec![(1, validator(0))]);
        SubtensorModule::recheck_childkey_threshold(&parent);
        assert!(SubtensorModule::get_children(&parent, netuid).is_empty());
    });
}

// ---------------------------------------------------------------- stake and dividends

/// A subnet with one validator that earns all dividends, and an open index that lends to it.
fn subnet_with_validator() -> (NetUid, U256, U256) {
    let netuid = NetUid::from(1);
    let validator_hotkey = U256::from(4);
    let validator_coldkey = U256::from(2);
    let miner_hotkey = U256::from(6);
    let stake = 100 * TAO;
    add_network(netuid, 10, 0);
    SubtensorModule::set_ck_burn(0);
    setup_reserves(netuid, (stake * 10_000).into(), (stake * 10_000).into());
    register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
    register_ok_neuron(netuid, miner_hotkey, U256::from(5), 1);
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);
    SubtensorModule::set_max_allowed_validators(netuid, 1);
    step_block(10);
    set_subnet_owner_cut_for_test(netuid, 0);
    Delegates::<Test>::insert(validator_hotkey, PerU16::zero());

    // The validator earned the last epoch's dividend, so the rule picks it.
    ValidatorPermit::<Test>::insert(netuid, vec![true, false]);
    Dividends::<Test>::insert(netuid, vec![PerU16::one(), PerU16::zero()]);
    assert_ok!(SubtensorModule::do_open_index(netuid, &U256::from(VAULT)));
    assert_eq!(
        children_of_index(netuid),
        vec![(MAX_INDEX_CHILD_PROPORTION, validator_hotkey)]
    );

    // Yuma: the validator sets all weight on the miner.
    Weights::<Test>::insert(NetUidStorageIndex::from(netuid), 0, vec![(1, 0xFFFF)]);
    BlockAtRegistration::<Test>::set(netuid, 0, 1);
    BlockAtRegistration::<Test>::set(netuid, 1, 1);
    LastUpdate::<Test>::set(NetUidStorageIndex::from(netuid), vec![2, 2]);
    Kappa::<Test>::set(netuid, u16::MAX / 5);
    ActivityCutoff::<Test>::set(netuid, u16::MAX);
    (netuid, validator_hotkey, validator_coldkey)
}

fn stake_at_index(coldkey: U256, netuid: NetUid) -> AlphaBalance {
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
        &SubtensorModule::index_hotkey(netuid),
        &coldkey,
        netuid,
    )
}

#[test]
fn stake_at_an_index_earns_what_its_children_earn_on_it_and_pays_no_take() {
    new_test_ext(1).execute_with(|| {
        let (netuid, validator_hotkey, _) = subnet_with_validator();
        let index = SubtensorModule::index_hotkey(netuid);
        let vault = U256::from(VAULT);
        let nominator = U256::from(7);
        // The vault that owns the index and an unrelated nominator stake alike.
        for coldkey in [vault, nominator] {
            add_balance_to_coldkey_account(&coldkey, TaoBalance::from(101 * TAO));
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                index,
                netuid,
                (100 * TAO).into()
            ));
        }
        let vault_before = stake_at_index(vault, netuid);
        let nominator_before = stake_at_index(nominator, netuid);
        assert!(vault_before > AlphaBalance::ZERO);
        // The index lends a quarter of its stake to its one child and keeps no key.
        let own = SubtensorModule::get_stake_for_hotkey_on_subnet(&validator_hotkey, netuid);
        let lent =
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&validator_hotkey, netuid) - own;
        let pool = SubtensorModule::get_stake_for_hotkey_on_subnet(&index, netuid);
        // A quarter, to the precision of the fixed-point proportion.
        assert!(
            u64::from(lent).abs_diff(u64::from(pool) / 4) <= 1_000,
            "lent {lent:?} of {pool:?}, own {own:?}"
        );
        assert_eq!(Uids::<Test>::get(netuid, index), None);

        step_block(10);

        let vault_gain = stake_at_index(vault, netuid) - vault_before;
        let nominator_gain = stake_at_index(nominator, netuid) - nominator_before;
        assert!(vault_gain > AlphaBalance::ZERO, "the index earned nothing");
        // No take: owner and nominator earn the same on the same stake.
        assert!(
            u64::from(vault_gain).abs_diff(u64::from(nominator_gain)) <= 2,
            "{vault_gain:?} != {nominator_gain:?}"
        );
        // All of it is in the pool: nothing was paid to the owner beside its share.
        let pool_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&index, netuid);
        assert!(
            u64::from(pool_after - pool)
                .abs_diff(u64::from(vault_gain) + u64::from(nominator_gain))
                <= 4
        );
    });
}

#[test]
fn a_child_keeps_its_childkey_take_of_what_the_index_earns() {
    let gain_with_take = |take: PerU16| {
        let mut gain = 0u64;
        new_test_ext(1).execute_with(|| {
            let (netuid, validator_hotkey, validator_coldkey) = subnet_with_validator();
            let index = SubtensorModule::index_hotkey(netuid);
            let vault = U256::from(VAULT);
            SubtensorModule::set_max_childkey_take(PerU16::from_percent(18));
            if !take.is_zero() {
                assert_ok!(SubtensorModule::set_childkey_take(
                    RuntimeOrigin::signed(validator_coldkey),
                    validator_hotkey,
                    netuid,
                    take
                ));
            }
            add_balance_to_coldkey_account(&vault, TaoBalance::from(101 * TAO));
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(vault),
                index,
                netuid,
                (100 * TAO).into()
            ));
            let before = stake_at_index(vault, netuid);
            step_block(10);
            gain = u64::from(stake_at_index(vault, netuid) - before);
        });
        gain
    };

    let full = gain_with_take(PerU16::zero());
    let after_take = gain_with_take(PerU16::from_percent(18));
    assert!(full > 0);
    // 18% stays with the child.
    let expected = full / 100 * 82;
    assert!(
        after_take.abs_diff(expected) <= full / 1_000,
        "{after_take} is not 82% of {full}"
    );
}

#[test]
fn the_owner_position_is_not_swept_as_a_small_nomination() {
    new_test_ext(1).execute_with(|| {
        let (netuid, _, _) = subnet_with_validator();
        let index = SubtensorModule::index_hotkey(netuid);
        let vault = U256::from(VAULT);
        let nominator = U256::from(7);
        SubtensorModule::set_nominator_min_required_stake(10_000_000);
        for coldkey in [vault, nominator] {
            add_balance_to_coldkey_account(&coldkey, TaoBalance::from(101 * TAO));
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                index,
                netuid,
                (100 * TAO).into()
            ));
        }
        // Both leave a position far below the nominator minimum.
        for coldkey in [vault, nominator] {
            let held = stake_at_index(coldkey, netuid);
            assert_ok!(SubtensorModule::remove_stake(
                RuntimeOrigin::signed(coldkey),
                index,
                netuid,
                held - AlphaBalance::from(1_000_000)
            ));
        }
        // The vault holds stake for others: its remainder stays. A nominator's is swept.
        assert_eq!(stake_at_index(vault, netuid), AlphaBalance::from(1_000_000));
        assert_eq!(stake_at_index(nominator, netuid), AlphaBalance::ZERO);
    });
}
