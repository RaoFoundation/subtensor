#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Epoch cap deferral and CRV3 reveal interaction.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_epochs_deferred_this_block_respects_cap() {
    new_test_ext(1).execute_with(|| {
        let cap = SubtensorModule::get_max_epochs_per_block() as usize;
        let n = cap + 2;

        for i in 0..n {
            let netuid = NetUid::from((i + 1) as u16);
            add_network(netuid, 100, 0);
            // Force "due this block".
            PendingEpochAt::<Test>::insert(netuid, 1);
        }

        let block = SubtensorModule::get_current_block_as_u64();
        let subnets: Vec<NetUid> = SubtensorModule::get_all_subnet_netuids()
            .into_iter()
            .filter(|x| *x != NetUid::ROOT)
            .collect();

        // All `n` subnets are due, but only `cap` may fire — the rest are deferred.
        let deferred = SubtensorModule::epochs_deferred_this_block(&subnets, block);
        assert_eq!(
            deferred.len(),
            n - cap,
            "exactly the due subnets beyond MaxEpochsPerBlock are deferred"
        );
        for netuid in &deferred {
            assert!(SubtensorModule::should_run_epoch(*netuid, block));
        }
    });
}

// Regression test for the dynamic-tempo / CR-v3 interaction: when a subnet's epoch
// is deferred by the per-block cap, its timelock reveal must be held back to the
// deferred fire-block (not run on the originally-scheduled block, which would
// surface weights before the epoch consumes them).
//
// Crypto-free probe: the reveal path removes *expired* commits only when it runs
// for a subnet, so a retained expired (epoch-0) commit means the reveal was skipped.
#[test]
fn test_reveal_crv3_defers_with_capped_epoch() {
    new_test_ext(1).execute_with(|| {
        let cap = SubtensorModule::get_max_epochs_per_block() as usize;
        let n = cap + 2;
        let mec0 = subtensor_runtime_common::MechId::from(0);

        for i in 0..n {
            let netuid = NetUid::from((i + 1) as u16);
            add_network(netuid, 100, 0);
            PendingEpochAt::<Test>::insert(netuid, 1); // due this block
            SubnetEpochIndex::<Test>::insert(netuid, 10); // cur_epoch >> reveal_period
            // Plant an expired commit at epoch 0 (field types inferred from the queue).
            let idx = SubtensorModule::get_mechanism_storage_index(netuid, mec0);
            TimelockedWeightCommits::<Test>::mutate(idx, 0u64, |q| {
                q.push_back((U256::from(1u64), 0u64, Default::default(), 0u64));
            });
        }

        let subnets: Vec<NetUid> = SubtensorModule::get_all_subnet_netuids()
            .into_iter()
            .filter(|x| *x != NetUid::ROOT)
            .collect();

        let still_holds = |netuid: NetUid| -> bool {
            let idx = SubtensorModule::get_mechanism_storage_index(netuid, mec0);
            TimelockedWeightCommits::<Test>::contains_key(idx, 0u64)
        };
        let retained = |subnets: &[NetUid]| subnets.iter().filter(|n| still_holds(**n)).count();

        // --- Phase 1: cap-deferred subnets must NOT reveal this block.
        SubtensorModule::reveal_crv3_commits();
        assert_eq!(
            retained(&subnets),
            n - cap,
            "only cap-deferred subnets keep their commit (their reveal was skipped)"
        );

        let deferred: Vec<NetUid> = subnets
            .iter()
            .copied()
            .filter(|n| still_holds(*n))
            .collect();

        // --- Phase 2: drop the cap pressure so only the deferred subnets are due;
        // they should now reveal (and clean their expired commit).
        for netuid in &subnets {
            if !deferred.contains(netuid) {
                PendingEpochAt::<Test>::insert(*netuid, 0);
                LastEpochBlock::<Test>::insert(*netuid, 1); // blocks_since < tempo => not due
            }
        }
        SubtensorModule::reveal_crv3_commits();
        assert_eq!(
            retained(&subnets),
            0,
            "deferred subnets reveal once they actually fire"
        );
    });
}
