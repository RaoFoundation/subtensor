#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

//! The block-step hook adds the weight of each Null epoch that runs. It keeps the
//! fixed envelope that it reserved before, and it adds nothing for a Yuma epoch.

use super::mock::*;
use crate::weights::WeightInfo;
use crate::*;
use frame_support::traits::Hooks;
use frame_support::weights::Weight;
use sp_core::U256;
use subtensor_runtime_common::{MechId, NetUid};

const TEMPO: u16 = 100;

fn subnet(netuid: u16, consensus: EpochConsensus, mechanisms: u8, uids: u16) -> NetUid {
    let netuid = NetUid::from(netuid);
    add_network(netuid, TEMPO, 0);
    SubtensorModule::set_epoch_consensus(netuid, consensus);
    MaxMechanismCount::<Test>::put(MechId::from(16));
    MechanismCountCurrent::<Test>::insert(netuid, MechId::from(mechanisms));
    for uid in 0..uids {
        let hotkey = U256::from(1_000 * u64::from(u16::from(netuid)) + u64::from(uid));
        SubtensorModule::append_neuron(netuid, &hotkey, 0);
    }
    netuid
}

/// The mock runtime takes its weights from the unit implementation.
fn null_epoch_weight(mechanisms: u32, uids: u32) -> Weight {
    <() as WeightInfo>::null_epoch(mechanisms, uids)
}

/// Runs the coinbase of `block` and returns the weight of its Null epochs.
fn coinbase_at(block: u64) -> Weight {
    System::set_block_number(block);
    SubtensorModule::run_coinbase_weighing_null_epochs(SubtensorModule::mint_tao(0.into()))
}

#[test]
fn coinbase_weighs_each_null_epoch_that_runs() {
    new_test_ext(1).execute_with(|| {
        let null = subnet(1, EpochConsensus::Null, 4, 3);
        let yuma = subnet(2, EpochConsensus::Yuma, 2, 5);
        let wide_null = subnet(3, EpochConsensus::Null, 16, 7);
        assert!(null_epoch_weight(4, 3).ref_time() > 0);
        let epochs_of = |netuid| SubnetEpochIndex::<Test>::get(netuid);

        // Block 2: no epoch is due.
        assert_eq!(coinbase_at(2), Weight::zero());
        assert_eq!(
            (epochs_of(null), epochs_of(yuma), epochs_of(wide_null)),
            (0, 0, 0)
        );

        // Block 3: only the Yuma epoch runs. The fixed envelope covers it.
        PendingEpochAt::<Test>::insert(yuma, 3);
        assert_eq!(coinbase_at(3), Weight::zero());
        assert_eq!(epochs_of(yuma), 1);

        // Block 4: one Null epoch runs, and it is weighed by its own counts.
        PendingEpochAt::<Test>::insert(null, 4);
        assert_eq!(coinbase_at(4), null_epoch_weight(4, 3));
        assert_eq!(epochs_of(null), 1);

        // Block 5: two Null epochs are due. One Null epoch runs in a block, so the
        // block weighs that one. The other one runs, and is weighed, in block 6.
        PendingEpochAt::<Test>::insert(null, 5);
        PendingEpochAt::<Test>::insert(wide_null, 5);
        assert_eq!(coinbase_at(5), null_epoch_weight(4, 3));
        assert_eq!((epochs_of(null), epochs_of(wide_null)), (2, 0));
        assert_eq!(coinbase_at(6), null_epoch_weight(16, 7));
        assert_eq!(epochs_of(wide_null), 1);

        // Block 7: a due Null epoch whose hotkey map is inconsistent is skipped, not run.
        Keys::<Test>::insert(null, 2, Keys::<Test>::get(null, 0));
        PendingEpochAt::<Test>::insert(null, 7);
        assert_eq!(coinbase_at(7), Weight::zero());
    });
}

/// Weight that `on_initialize` returns for block 2 of a chain with one subnet.
fn hook_weight(consensus: EpochConsensus, epoch_due: bool) -> Weight {
    new_test_ext(1).execute_with(|| {
        let netuid = subnet(1, consensus, 4, 3);
        if epoch_due {
            PendingEpochAt::<Test>::insert(netuid, 2);
        }
        System::set_block_number(2);
        let weight = SubtensorModule::on_initialize(2);
        assert_eq!(SubnetEpochIndex::<Test>::get(netuid), u64::from(epoch_due));
        weight
    })
}

#[test]
fn hook_adds_the_null_epoch_weight_to_the_fixed_envelope() {
    let idle = hook_weight(EpochConsensus::Null, false);
    assert!(idle.ref_time() >= <() as WeightInfo>::block_step().ref_time());
    assert_eq!(
        hook_weight(EpochConsensus::Null, true),
        idle + null_epoch_weight(4, 3)
    );
}

#[test]
fn hook_weight_of_a_yuma_epoch_is_unchanged() {
    let idle = hook_weight(EpochConsensus::Yuma, false);
    assert_eq!(hook_weight(EpochConsensus::Yuma, true), idle);
    assert_eq!(idle, hook_weight(EpochConsensus::Null, false));
}
