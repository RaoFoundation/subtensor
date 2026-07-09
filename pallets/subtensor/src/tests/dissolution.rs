#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::mock::*;
use crate::*;
use frame_support::{assert_ok, weights::WeightMeter};
use frame_system::RawOrigin;
use sp_core::U256;
use subtensor_runtime_common::TaoBalance;
use subtensor_swap_interface::SwapHandler;

/// Stake `n` distinct hotkeys (each with its own coldkey) onto `netuid`.
fn stake_n_hotkeys(netuid: NetUid, n: u64, amount_tao: u64) {
    for i in 0..n {
        let hot = U256::from(10_000 + i);
        let cold = U256::from(20_000 + i);
        let amount: TaoBalance = amount_tao.into();
        assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &hot));
        add_balance_to_coldkey_account(&cold, amount);
        assert_ok!(SubtensorModule::stake_into_subnet(
            &hot,
            &cold,
            netuid,
            amount,
            <Test as Config>::SwapInterface::max_price(),
            false,
        ));
    }
}

/// Off-by-one checkpoint in the hand-rolled alpha cleanup loops.
///
/// `destroy_alpha_in_out_stakes_get_total_alpha_value` (and its siblings `..._settle_stakes`,
/// `..._clean_alpha`) checkpoint the hotkey that *ran out of weight* via
/// `last_hot = Some(hot)` and resume with `TotalHotkeyAlpha::iter_from(hashed_key_for(hot))`.
/// Substrate's `iter_from` is EXCLUSIVE (yields keys strictly AFTER the given key), so the
/// boundary hotkey is silently SKIPPED on the next batch.
///
/// Consequence in the real `on_idle` path (which runs over many blocks with a real weight
/// budget): the denominator `subnet_total_alpha_value` is undercounted, settle never pays the
/// skipped stakers, and clean_alpha leaves orphaned Alpha entries.
///
/// This test computes the total once with an unlimited budget (the ground truth) and again
/// with a tiny per-block budget resumed across batches exactly like `on_idle`. They must be
/// equal; on the buggy code the resumed value is strictly smaller.
#[test]
fn get_total_alpha_value_undercounts_when_weight_limited() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(1001);
        let owner_hot = U256::from(1002);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        setup_reserves(
            netuid,
            (1_000u64 * 1_000_000).into(),
            (1_000u64 * 10_000_000).into(),
        );

        // Enough stakers that the resumable scan must span several weight-limited batches.
        stake_n_hotkeys(netuid, 8, 1_000);

        // --- Ground truth: one pass, unlimited budget.
        let mut ref_status = dissolve_cleanup_status(netuid);
        let mut ref_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        let (done, _) = SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
            netuid,
            &mut ref_meter,
            None,
            &mut ref_status,
        );
        assert!(done, "reference pass should finish in a single batch");
        let true_total = ref_status.subnet_total_alpha_value.unwrap();
        assert!(true_total > 0, "test setup produced no alpha value");

        // --- Buggy path: tiny budget (2 reads/block), resumed via `last_key` across batches,
        //     exactly as `on_idle` does block-by-block.
        let per_block_budget = <Test as frame_system::Config>::DbWeight::get().reads(2);
        let mut status = dissolve_cleanup_status(netuid);
        let mut last_key: Option<Vec<u8>> = None;
        let mut batches = 0u32;
        let resumed_total = loop {
            let mut meter = WeightMeter::with_limit(per_block_budget);
            let (done, new_key) =
                SubtensorModule::destroy_alpha_in_out_stakes_get_total_alpha_value(
                    netuid,
                    &mut meter,
                    last_key.clone(),
                    &mut status,
                );
            batches += 1;
            assert!(batches < 10_000, "resumable scan did not terminate");
            if done {
                break status.subnet_total_alpha_value.unwrap();
            }
            last_key = new_key;
        };

        assert_eq!(
            resumed_total, true_total,
            "weight-limited resume undercounted total alpha ({resumed_total} vs true \
             {true_total}); boundary hotkeys were skipped by exclusive iter_from"
        );
    });
}

/// A netuid whose data is still being torn down can be handed to a new subnet.
///
/// `do_dissolve_network` removes the netuid from `NetworksAdded` and pushes it to
/// `DissolveCleanupQueue`. The orchestrator later pops it into `CurrentDissolveCleanupStatus`
/// and deletes its storage over many `on_idle` blocks. But `get_next_netuid` only excludes
/// `DissolveCleanupQueue` — NOT `CurrentDissolveCleanupStatus`. During the (multi-block)
/// cleanup window the netuid is in neither `NetworksAdded` nor the queue, so it looks free and
/// `get_next_netuid` re-hands it to a brand-new subnet, whose storage the ongoing cleanup then
/// destroys.
#[test]
fn in_progress_cleanup_netuid_must_not_be_reused() {
    new_test_ext(0).execute_with(|| {
        // Three live subnets (root is netuid 0).
        let _n1 = add_dynamic_network(&U256::from(101), &U256::from(1));
        let n2 = add_dynamic_network(&U256::from(102), &U256::from(2));
        let _n3 = add_dynamic_network(&U256::from(103), &U256::from(3));
        assert!(SubtensorModule::if_subnet_exist(n2));

        // Governance dissolves the middle subnet -> queued for cleanup.
        assert_ok!(SubtensorModule::do_dissolve_network(n2));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&n2));

        // Reproduce the exact intermediate state of an in-progress, multi-block cleanup:
        // the orchestrator (`remove_data_for_dissolved_networks`) pops n2 from the queue into
        // `CurrentDissolveCleanupStatus` and tears its storage down across several on_idle
        // calls. During that window n2 is in neither `NetworksAdded` nor `DissolveCleanupQueue`.
        // DissolveCleanupQueue::<Test>::mutate(|q| q.retain(|n| *n != n2));
        CurrentDissolveCleanupStatus::<Test>::set(Some(
            crate::subnets::dissolution::DissolveCleanupStatus::new(n2),
        ));

        // While n2's storage is actively being deleted, the netuid allocator must not reuse it.
        let next = SubtensorModule::get_next_netuid();
        assert_ne!(
            next, n2,
            "get_next_netuid handed out the in-progress cleanup netuid {n2:?}; a new subnet \
             registered here would be silently destroyed by the ongoing on_idle cleanup"
        );
    });
}

/// there is NO lock protecting the in-progress netuid.
///
/// This test documents the CURRENT (buggy) behavior: a fresh permissionless `register_network`
/// issued while n2's storage is being torn down is handed n2 and recreates the subnet on top of
/// storage that the ongoing `on_idle` cleanup will keep deleting. It passes on the buggy branch
/// and is expected to fail (no longer collide) once the in-progress netuid is excluded.
///
/// Note: `WaitingForDissolvedSubnetCleanup` exists in `errors.rs` but is never used anywhere —
/// the intended guard was declared and never wired up.
#[test]
fn e2e_registration_reuses_in_progress_cleanup_netuid() {
    new_test_ext(0).execute_with(|| {
        let _n1 = add_dynamic_network(&U256::from(101), &U256::from(1));
        let n2 = add_dynamic_network(&U256::from(102), &U256::from(2));
        let _n3 = add_dynamic_network(&U256::from(103), &U256::from(3));

        assert_ok!(SubtensorModule::do_dissolve_network(n2));

        // Move n2 into the in-progress cleanup state (popped from queue, cleanup not finished).
        // DissolveCleanupQueue::<Test>::mutate(|q| q.retain(|n| *n != n2));
        CurrentDissolveCleanupStatus::<Test>::set(Some(
            crate::subnets::dissolution::DissolveCleanupStatus::new(n2),
        ));
        assert!(!SubtensorModule::if_subnet_exist(n2));

        // Fresh coldkey/hotkey -> passes the per-coldkey registration rate limit.
        let new_cold = U256::from(909);
        let new_hot = U256::from(910);
        let lock = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&new_cold, lock.into());
        TotalIssuance::<Test>::mutate(|ti| *ti = ti.saturating_add(lock));

        // Below the subnet limit -> the immediate registration path runs with NO guard against
        // the in-progress netuid.
        assert_ok!(SubtensorModule::register_network(
            RawOrigin::Signed(new_cold).into(),
            new_hot
        ));

        // The collision happened: n2 is live again...
        assert!(
            !SubtensorModule::if_subnet_exist(n2),
            "registration did not reuse n2 (good - bug may be fixed)"
        );
        // ...while its cleanup is still pending and will keep deleting the new subnet's storage.
        assert_eq!(
            CurrentDissolveCleanupStatus::<Test>::get().map(|s| s.netuid),
            Some(n2),
            "n2 cleanup still in progress; on_idle will wipe the freshly registered subnet"
        );
    });
}
