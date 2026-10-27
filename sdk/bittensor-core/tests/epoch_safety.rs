use bittensor_core::timelock::epoch_schedule::*;

fn state() -> EpochScheduleState {
    EpochScheduleState {
        last_epoch_block: 100,
        pending_epoch_at: 0,
        subnet_epoch_index: 0,
        tempo: 20,
        blocks_since_last_step: 0,
        current_block: 100,
    }
}

#[test]
fn all_upstream_prediction_vectors() {
    // Complete v478 epoch_schedule_vectors.rs suite. Its module is private
    // and cfg(test) upstream, so pin the public inputs and expected outputs.
    let cases = [
        (
            "tempo_zero",
            EpochScheduleState {
                last_epoch_block: 10,
                current_block: 10,
                tempo: 0,
                ..state()
            },
            Err(EpochScheduleError::TempoIsZero),
        ),
        (
            "cycle_reset",
            EpochScheduleState {
                last_epoch_block: 10,
                current_block: 10,
                tempo: 50,
                ..state()
            },
            Ok(60),
        ),
        (
            "pending_fires_before_auto",
            EpochScheduleState {
                last_epoch_block: 80,
                pending_epoch_at: 95,
                current_block: 91,
                ..state()
            },
            Ok(95),
        ),
    ];
    for (name, state, expected) in cases {
        assert_eq!(predict_first_reveal_block(&state, 1), expected, "{name}");
    }
    let head = EpochScheduleState {
        current_block: 120,
        tempo: 50,
        ..state()
    };
    let after = simulate_run_coinbase(&head, 121);
    assert_eq!(current_epoch_pre_run_coinbase(&after, 121), 0);
}

#[test]
fn skipped_scheduled_slot_fires_next_block() {
    let skipped = EpochScheduleState {
        last_epoch_block: 120,
        current_block: 120,
        blocks_since_last_step: 21,
        ..state()
    };
    assert!(should_run_epoch(&skipped, 121));
    assert_eq!(simulate_run_coinbase(&skipped, 121).subnet_epoch_index, 1);
}

#[test]
fn skipped_owner_slot_fires_at_counter_threshold() {
    let skipped = EpochScheduleState {
        last_epoch_block: 90,
        current_block: 90,
        blocks_since_last_step: 5,
        ..state()
    };
    assert_eq!(advance_blocks(&skipped, 91, 105).subnet_epoch_index, 0);
    assert_eq!(advance_blocks(&skipped, 91, 106).subnet_epoch_index, 1);
}

#[test]
fn lowered_tempo_fires_next_block_and_counter_is_bounded() {
    let lowered = EpochScheduleState {
        blocks_since_last_step: 30,
        ..state()
    };
    assert!(should_run_epoch(&lowered, 101));
    let disabled = EpochScheduleState {
        tempo: 0,
        blocks_since_last_step: u64::MAX,
        ..state()
    };
    let next = simulate_run_coinbase(&disabled, 101);
    assert_eq!(next.blocks_since_last_step, 1);
    assert_eq!(next.subnet_epoch_index, 0);
}

#[test]
fn inclusion_counter_fire_assigns_post_coinbase_epoch() {
    // An owner-triggered skip at 90 left a step counter of 5. At head 105
    // the counter is 20. Coinbase at inclusion 106 increments it to 21
    // and succeeds, before the commit extrinsic is applied.
    let head = EpochScheduleState {
        last_epoch_block: 90,
        current_block: 105,
        blocks_since_last_step: 20,
        ..state()
    };
    assert!(!should_run_epoch(&head, 106));
    let after = simulate_run_coinbase(&head, 106);
    assert_eq!(after.subnet_epoch_index, 1);
    assert_eq!(current_epoch_pre_run_coinbase(&after, 106), 1);
    assert_eq!(predict_first_reveal_block(&head, 1), Ok(126));
}

#[test]
fn prediction_refuses_overflow_and_disabled_tempo() {
    assert_eq!(
        predict_first_reveal_block(
            &EpochScheduleState {
                current_block: u64::MAX,
                ..state()
            },
            1
        ),
        Err(EpochScheduleError::BoundExceeded)
    );
    assert_eq!(
        predict_first_reveal_block(
            &EpochScheduleState {
                subnet_epoch_index: u64::MAX,
                ..state()
            },
            1
        ),
        Err(EpochScheduleError::BoundExceeded)
    );
    assert_eq!(
        predict_first_reveal_block(
            &EpochScheduleState {
                tempo: 0,
                ..state()
            },
            1
        ),
        Err(EpochScheduleError::TempoIsZero)
    );
}

#[test]
fn overflowing_simulation_budget_fails_before_searching() {
    assert_eq!(
        predict_first_reveal_block(&state(), u64::MAX),
        Err(EpochScheduleError::BoundExceeded)
    );
}
