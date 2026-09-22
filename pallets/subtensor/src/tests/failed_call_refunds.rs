//! Spec 469: a failed heavy call is charged the work it did, not the envelope it declared.
//!
//! v468 defect 2: every unstake-side call refunded its `StakingHotkeys` scan allowance on
//! success only; the error branch returned a plain `DispatchError`, whose default
//! `PostDispatchInfo` "stands for the worst case static weight". A failed `unstake_all`
//! therefore held 50% of a block's normal capacity for 0.0000833 TAO. Every call here
//! now returns `DispatchErrorWithPostInfo` with the weight it actually used, on every
//! error path: the pre-check reads when refused before any write, the scan plus the
//! rolled-back work when admitted and then failed. Never below what admission cost, never
//! above the declaration.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::mock::*;
use crate::tests::claim_root::register_on_root;
use crate::weights::WeightInfo;
use crate::*;
use frame_support::assert_ok;
use frame_support::dispatch::{DispatchResultWithPostInfo, GetDispatchInfo};
use frame_support::weights::Weight;
use sp_core::U256;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

/// The weight `call` declares, the way FRAME reserves it.
fn declared(call: RuntimeCall) -> Weight {
    call.get_dispatch_info().call_weight
}

/// The actual weight a failed dispatch reported. Panics when the call succeeded or when
/// it failed without post-dispatch info (that is the v468 bug this file guards).
fn failed_weight(result: DispatchResultWithPostInfo, error: Error<Test>) -> Weight {
    let err = result.expect_err("call must fail");
    assert_eq!(err.error, error.into());
    err.post_info
        .actual_weight
        .expect("a failed heavy call reports the weight it used")
}

fn stake_exit_fixture() -> (NetUid, U256, U256) {
    let owner_coldkey = U256::from(1001);
    let owner_hotkey = U256::from(1002);
    let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
    let coldkey = U256::from(1);
    let hotkey = U256::from(2);
    let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey,
        &coldkey,
        netuid,
        AlphaBalance::from(10_000_000_000_u64),
    );
    (netuid, coldkey, hotkey)
}

// ------------------------------------------------------------ stake exits ---

/// v468 defect 2, the reported case: an oversize `transfer_stake` fails after the scan
/// and pays base + its real `StakingHotkeys` walk, not the 256-key bound.
#[test]
fn failed_transfer_stake_refunds_the_walk_bound() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey) = stake_exit_fixture();
        let destination = U256::from(3);
        let oversize = AlphaBalance::from(10_000_000_001_u64);
        let call = RuntimeCall::SubtensorModule(crate::Call::transfer_stake {
            destination_coldkey: destination,
            hotkey,
            origin_netuid: netuid,
            destination_netuid: netuid,
            alpha_amount: oversize,
        });
        let actual = failed_weight(
            SubtensorModule::transfer_stake(
                RuntimeOrigin::signed(coldkey),
                destination,
                hotkey,
                netuid,
                netuid,
                oversize,
            ),
            Error::<Test>::NotEnoughStakeToWithdraw,
        );
        let expected = <Test as Config>::WeightInfo::transfer_stake()
            .saturating_add(SubtensorModule::staking_hotkeys_walk_actual(&coldkey));
        assert_eq!(actual, expected);
        assert!(actual.all_lt(declared(call)));
        // The same figure a successful exit reports.
        let ok = SubtensorModule::transfer_stake(
            RuntimeOrigin::signed(coldkey),
            destination,
            hotkey,
            netuid,
            netuid,
            AlphaBalance::from(5_000_000_000_u64),
        )
        .expect("transfer succeeds");
        assert_eq!(ok.actual_weight, Some(expected));
    });
}

/// Every explicit-amount exit reports actual weight on failure.
#[test]
fn every_stake_exit_reports_actual_weight_on_failure() {
    new_test_ext(1).execute_with(|| {
        let (netuid, coldkey, hotkey) = stake_exit_fixture();
        let other_hotkey = U256::from(3);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &other_hotkey);
        let oversize = AlphaBalance::from(10_000_000_001_u64);
        let ghost_netuid = NetUid::from(200);
        let walk = SubtensorModule::staking_hotkeys_walk_actual(&coldkey);
        let origin = || RuntimeOrigin::signed(coldkey);
        let cases: Vec<(DispatchResultWithPostInfo, Weight)> = vec![
            (
                SubtensorModule::remove_stake(origin(), hotkey, ghost_netuid, oversize),
                <Test as Config>::WeightInfo::remove_stake(),
            ),
            (
                SubtensorModule::remove_stake_limit(
                    origin(),
                    hotkey,
                    ghost_netuid,
                    oversize,
                    TaoBalance::from(1_u64),
                    false,
                ),
                <Test as Config>::WeightInfo::remove_stake_limit(),
            ),
            (
                SubtensorModule::remove_stake_full_limit(origin(), hotkey, ghost_netuid, None),
                <Test as Config>::WeightInfo::remove_stake_full_limit(),
            ),
            (
                SubtensorModule::move_stake(
                    origin(),
                    hotkey,
                    other_hotkey,
                    netuid,
                    netuid,
                    oversize,
                ),
                <Test as Config>::WeightInfo::move_stake(),
            ),
            (
                SubtensorModule::move_stake_limit(
                    origin(),
                    hotkey,
                    other_hotkey,
                    netuid,
                    netuid,
                    oversize,
                    TaoBalance::ZERO,
                    false,
                ),
                <Test as Config>::WeightInfo::move_stake_limit(),
            ),
            (
                SubtensorModule::transfer_stake_and_hotkey(
                    origin(),
                    U256::from(4),
                    hotkey,
                    other_hotkey,
                    netuid,
                    netuid,
                    oversize,
                ),
                <Test as Config>::WeightInfo::transfer_stake_and_hotkey(),
            ),
            (
                SubtensorModule::swap_stake(origin(), hotkey, netuid, netuid, oversize),
                <Test as Config>::WeightInfo::swap_stake(),
            ),
            (
                SubtensorModule::swap_stake_limit(
                    origin(),
                    hotkey,
                    netuid,
                    netuid,
                    oversize,
                    TaoBalance::ZERO,
                    false,
                ),
                <Test as Config>::WeightInfo::swap_stake_limit(),
            ),
        ];
        for (result, base) in cases {
            let err = result.expect_err("oversize exit fails");
            assert_eq!(
                err.post_info.actual_weight,
                Some(base.saturating_add(walk)),
                "{:?}",
                err.error
            );
            assert!(walk.ref_time() < SubtensorModule::staking_hotkeys_walk_bound().ref_time());
        }
    });
}

/// v468 defect 2, the cheapest lever: `unstake_all` on a hotkey that does not exist fails
/// at its first guard and used to keep half a block. It now pays the base plus its walk.
#[test]
fn failed_unstake_all_refunds_the_envelope() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let ghost = U256::from(99);
        add_dynamic_network(&U256::from(1002), &U256::from(1001));
        for (result, call) in [
            (
                SubtensorModule::unstake_all(RuntimeOrigin::signed(coldkey), ghost),
                RuntimeCall::SubtensorModule(crate::Call::unstake_all { hotkey: ghost }),
            ),
            (
                SubtensorModule::unstake_all_alpha(RuntimeOrigin::signed(coldkey), ghost),
                RuntimeCall::SubtensorModule(crate::Call::unstake_all_alpha { hotkey: ghost }),
            ),
        ] {
            let actual = failed_weight(result, Error::<Test>::HotKeyAccountNotExists);
            let envelope = declared(call);
            assert!(actual.all_lt(envelope), "{actual:?} < {envelope:?}");
            // No leg ran and no subnet was scanned: the fixed base plus nothing else.
            assert!(
                actual.ref_time() <= <Test as Config>::WeightInfo::unstake_all_alpha().ref_time()
            );
        }
    });
}

// ------------------------------------------------------------ basket calls ---

#[test]
fn refused_stake_into_basket_pays_its_prechecks() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(1_000_000_000_u64));
        // Not registered on root: refused before any write.
        let actual = failed_weight(
            SubtensorModule::stake_into_basket(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                TaoBalance::from(100_000_000_u64),
            ),
            Error::<Test>::HotKeyNotRegisteredInSubNet,
        );
        assert_eq!(actual, SubtensorModule::stake_into_basket_precheck_weight());
        assert!(actual.all_lt(SubtensorModule::stake_into_basket_declared_weight()));
    });
}

#[test]
fn refused_swap_basket_pays_its_prechecks() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        BasketTradingEnabled::<Test>::put(false);
        let actual = failed_weight(
            SubtensorModule::swap_basket(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                NetUid::from(1),
                NetUid::from(2),
                AlphaBalance::from(1_u64),
                0,
            ),
            Error::<Test>::BasketTradingDisabled,
        );
        assert_eq!(actual, SubtensorModule::swap_basket_precheck_weight());
        assert!(actual.all_lt(SubtensorModule::swap_basket_declared_weight()));
    });
}

// ---------------------------------------------------------- swap_hotkey* ---

/// A single-subnet swap refused by its pre-checks pays the fixed pre-check reads: those
/// checks read one row each (subnet, owners, account, collateral, membership).
#[test]
fn refused_single_subnet_swap_hotkey_pays_its_prechecks() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let not_owner = U256::from(3);
        add_network(netuid, 1, 0);
        register_ok_neuron(netuid, old_hotkey, coldkey, 0);
        let some = Some(netuid);
        let precheck = SubtensorModule::swap_hotkey_precheck_weight(&old_hotkey, &some);
        for (result, error) in [
            (
                SubtensorModule::swap_hotkey_v2(
                    RuntimeOrigin::signed(not_owner),
                    old_hotkey,
                    U256::from(4),
                    some,
                    false,
                ),
                Error::<Test>::NonAssociatedColdKey,
            ),
            (
                SubtensorModule::swap_hotkey_v2(
                    RuntimeOrigin::signed(coldkey),
                    old_hotkey,
                    old_hotkey,
                    some,
                    false,
                ),
                Error::<Test>::NewHotKeyIsSameWithOld,
            ),
        ] {
            let actual = failed_weight(result, error);
            assert_eq!(actual, precheck);
            assert!(
                actual.all_lt(SubtensorModule::swap_hotkey_v2_dispatch_weight(
                    &old_hotkey,
                    &some,
                    false
                ))
            );
        }
    });
}

/// The longest single-subnet refusal: a root swap that passes every ownership, collateral
/// and membership check and is refused by the last clean-root rule (`RootClaimed` residue).
/// It performs about fourteen single reads; the fixed pre-check figure covers them all.
#[test]
fn late_clean_root_refusal_is_charged_every_precheck_read() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(3);
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        TotalNetworks::<Test>::put(1);
        Owner::<Test>::insert(old_hotkey, coldkey);
        Owner::<Test>::insert(new_hotkey, coldkey);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(10_000_000_000_u64));
        // Every earlier rule passes; only the legacy claim residue on the new hotkey fails.
        RootClaimed::<Test>::insert((NetUid::ROOT, new_hotkey, U256::from(9)), 1u128);
        let root = Some(NetUid::ROOT);
        let actual = failed_weight(
            SubtensorModule::swap_hotkey_v2(
                RuntimeOrigin::signed(coldkey),
                old_hotkey,
                new_hotkey,
                root,
                true,
            ),
            Error::<Test>::NewHotKeyNotCleanForRootSwap,
        );
        assert_eq!(
            actual,
            SubtensorModule::swap_hotkey_precheck_weight(&old_hotkey, &root)
        );
        // Subnet, owners ×2 each, account, collateral, membership, seed state, BasketRate,
        // BasketShares, root stake, RootClaimable, RootClaimed: fourteen reads.
        assert!(actual.all_gte(<Test as frame_system::Config>::DbWeight::get().reads(14)));
        assert!(
            actual.ref_time()
                < SubtensorModule::swap_hotkey_v2_dispatch_weight(&old_hotkey, &root, true)
                    .ref_time()
        );
    });
}

/// An all-subnet swap's pre-checks walk the subnet list (collateral rule, twice) and the
/// new hotkey's membership prefix; neither is bounded by a stored count, so a refusal there
/// keeps the declared weight rather than a refund that could under-bill (skeptic 533f63c3).
#[test]
fn refused_all_subnet_swap_hotkey_keeps_the_declaration() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let old_hotkey = U256::from(2);
        let new_hotkey = U256::from(3);
        for raw in 1..=6u16 {
            add_network(NetUid::from(raw), 1, 0);
            register_ok_neuron(NetUid::from(raw), old_hotkey, coldkey, 0);
        }
        // The new hotkey is already registered somewhere: refused after the collateral walk.
        register_ok_neuron(NetUid::from(1), new_hotkey, coldkey, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(10_000_000_000_u64));
        for keep_stake in [true, false] {
            let err = SubtensorModule::swap_hotkey_v2(
                RuntimeOrigin::signed(coldkey),
                old_hotkey,
                new_hotkey,
                None,
                keep_stake,
            )
            .expect_err("refused");
            assert_eq!(
                err.error,
                Error::<Test>::HotKeyAlreadyRegisteredInSubNet.into()
            );
            assert_eq!(
                err.post_info.actual_weight, None,
                "keep_stake={keep_stake}: all-subnet pre-checks are not provably metered"
            );
        }
    });
}

/// A hotkey swap that fails inside its transaction keeps the declared weight on every
/// path, `keep_stake` included: the body's meter walks `NetworksAdded` more than it
/// charges, so the envelope is the only honest figure there.
#[test]
fn swap_hotkey_failing_inside_the_transaction_keeps_the_declaration() {
    new_test_ext(1).execute_with(|| {
        let old_hotkey = U256::from(1);
        let new_hotkey = U256::from(2);
        let coldkey = U256::from(3);
        Owner::<Test>::insert(old_hotkey, coldkey);
        for raw in 1..=4u16 {
            add_network(NetUid::from(raw), 1, 0);
        }
        // No balance for the swap cost: the failure is inside the transaction, after the
        // per-subnet cooldown and collateral walks.
        for keep_stake in [true, false] {
            let err = SubtensorModule::swap_hotkey_v2(
                RuntimeOrigin::signed(coldkey),
                old_hotkey,
                new_hotkey,
                None,
                keep_stake,
            )
            .expect_err("cannot pay the swap cost");
            assert_eq!(
                err.error,
                Error::<Test>::NotEnoughBalanceToPaySwapHotKey.into()
            );
            assert_eq!(
                err.post_info.actual_weight, None,
                "keep_stake={keep_stake}: a late failure keeps the declaration"
            );
        }
    });
}

// --------------------------------------------------------- swap_coldkey* ---

#[test]
fn refused_swap_coldkey_announced_pays_its_prechecks() {
    new_test_ext(1).execute_with(|| {
        let who = U256::from(1);
        let actual = failed_weight(
            SubtensorModule::swap_coldkey_announced(RuntimeOrigin::signed(who), U256::from(2)),
            Error::<Test>::ColdkeySwapAnnouncementNotFound,
        );
        assert_eq!(
            actual,
            <Test as Config>::WeightInfo::swap_coldkey_announced()
                .saturating_add(<Test as frame_system::Config>::DbWeight::get().reads(1))
        );
        assert!(actual.all_lt(SubtensorModule::swap_coldkey_announced_declared_weight()));
    });
}

#[test]
fn too_heavy_swap_coldkey_pays_the_scan_it_did() {
    new_test_ext(1).execute_with(|| {
        let old = U256::from(1);
        let new = U256::from(2);
        let netuid = add_dynamic_network(&U256::from(1002), &U256::from(1001));
        // More hotkeys than one swap admits, each with one position.
        for i in 0..=crate::MAX_COLDKEY_SWAP_HOTKEYS {
            let hotkey = U256::from(10_000 + u64::from(i));
            let _ = SubtensorModule::create_account_if_non_existent(&old, &hotkey);
            mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &old,
                netuid,
                AlphaBalance::from(1_000_000_u64),
            );
        }
        let (done, error) =
            SubtensorModule::do_swap_coldkey_tracked(&old, &new).expect_err("too heavy");
        assert_eq!(error, Error::<Test>::ColdkeySwapTooHeavy.into());
        let work = SubtensorModule::coldkey_swap_work(&old);
        assert!(work.hotkeys > crate::MAX_COLDKEY_SWAP_HOTKEYS);
        // Never below the scan: one StakingHotkeys read plus one read per hotkey and
        // per position, at the real (over-cap) counts.
        let scan = <Test as frame_system::Config>::DbWeight::get()
            .reads(1 + u64::from(work.hotkeys) + u64::from(work.positions));
        assert!(done.all_gte(scan));
        assert!(
            <Test as Config>::WeightInfo::swap_coldkey()
                .saturating_add(done)
                .all_lt(SubtensorModule::swap_coldkey_declared_weight())
        );
    });
}

// ------------------------------------------------------------- register* ---

#[test]
fn refused_registration_pays_its_prechecks() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let ghost = NetUid::from(77);
        for (result, error) in [
            (
                SubtensorModule::burned_register(RuntimeOrigin::signed(coldkey), ghost, hotkey),
                Error::<Test>::SubnetNotExists,
            ),
            (
                SubtensorModule::register_limit(RuntimeOrigin::signed(coldkey), ghost, hotkey, 1),
                Error::<Test>::SubnetNotExists,
            ),
            (
                SubtensorModule::burned_register(
                    RuntimeOrigin::signed(coldkey),
                    NetUid::ROOT,
                    hotkey,
                ),
                Error::<Test>::RegistrationNotPermittedOnRootSubnet,
            ),
        ] {
            let actual = failed_weight(result, error);
            assert_eq!(actual, SubtensorModule::registration_precheck_weight());
            assert!(actual.all_lt(<Test as Config>::WeightInfo::burned_register()));
        }
    });
}

/// A full subnet whose every uid is owner-immortal has no prune candidate. Finding that out
/// walks the owner's hotkeys and every uid — the benchmarked registration's own scan — so
/// the refusal keeps the declared weight rather than reporting the fixed pre-check figure.
#[test]
fn registration_refused_after_the_prune_search_keeps_the_declaration() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(4);
        let owner_ck = U256::from(7777);
        let owner_hks = [U256::from(9001), U256::from(9002), U256::from(9003)];
        add_network(netuid, 1, 0);
        SubnetOwner::<Test>::insert(netuid, owner_ck);
        for hk in owner_hks {
            register_ok_neuron(netuid, hk, owner_ck, 0);
            Owner::<Test>::insert(hk, owner_ck);
        }
        OwnedHotkeys::<Test>::insert(owner_ck, owner_hks.to_vec());
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 10);
        SubtensorModule::set_max_allowed_uids(netuid, owner_hks.len() as u16);
        assert_eq!(SubtensorModule::get_neuron_to_prune(netuid), None);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(10_000_000_000_u64));
        let err = SubtensorModule::burned_register(RuntimeOrigin::signed(coldkey), netuid, hotkey)
            .expect_err("no slot");
        assert_eq!(err.error, Error::<Test>::NoNeuronIdAvailable.into());
        assert_eq!(
            err.post_info.actual_weight, None,
            "the prune search is the registration's own scan: keep the declaration"
        );
    });
}

// -------------------------------------------------------- terminate_lease ---

#[test]
fn refused_terminate_lease_pays_its_prechecks() {
    new_test_ext(1).execute_with(|| {
        let actual = failed_weight(
            SubtensorModule::terminate_lease(
                RuntimeOrigin::signed(U256::from(1)),
                0,
                U256::from(2),
            ),
            Error::<Test>::LeaseDoesNotExist,
        );
        assert_eq!(
            actual,
            <Test as frame_system::Config>::DbWeight::get().reads(3)
        );
        assert!(actual.all_lt(SubtensorModule::terminate_lease_declared_weight()));
    });
}

// ------------------------------------------------------------------ smoke ---

/// The success path is unchanged: a root-registered validator's fund still reports its
/// real row count after a successful deposit.
#[test]
fn successful_stake_into_basket_still_reports_actual_weight() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_on_root(&hotkey, 0);
        add_balance_to_coldkey_account(&coldkey, TaoBalance::from(10_000_000_000_u64));
        let post = SubtensorModule::stake_into_basket(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            TaoBalance::from(1_000_000_000_u64),
        )
        .expect("deposit succeeds");
        assert_ok!(Ok::<(), ()>(()));
        let actual = post.actual_weight.expect("actual weight reported");
        assert!(actual.all_lt(SubtensorModule::stake_into_basket_declared_weight()));
    });
}
