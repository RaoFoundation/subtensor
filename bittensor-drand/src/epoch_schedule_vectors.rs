//! Table-driven epoch schedule test vectors.
//!
//! Numeric expectations are pinned against `bittensor_drand::epoch_schedule` and
//! cite the subtensor tests that motivated each scenario.

use crate::epoch_schedule::{EpochScheduleError, EpochScheduleState};

pub struct PredictVector {
    pub name: &'static str,
    pub source: &'static str,
    pub state: EpochScheduleState,
    pub reveal_period_epochs: u64,
    pub expected_reveal_block: Option<u64>,
    pub expected_error: Option<EpochScheduleError>,
}

pub fn predict_vectors() -> &'static [PredictVector] {
    &[
        PredictVector {
            name: "tempo_zero",
            source: "epoch_schedule.rs tempo guard",
            state: EpochScheduleState {
                last_epoch_block: 10,
                pending_epoch_at: 0,
                subnet_epoch_index: 0,
                tempo: 0,
                blocks_since_last_step: 0,
                current_block: 10,
            },
            reveal_period_epochs: 1,
            expected_reveal_block: None,
            expected_error: Some(EpochScheduleError::TempoIsZero),
        },
        PredictVector {
            name: "cycle_reset",
            source: "subtensor tempo_control.rs:195 get_next_epoch_start_block_reflects_set_tempo_cycle_reset",
            state: EpochScheduleState {
                last_epoch_block: 10,
                pending_epoch_at: 0,
                subnet_epoch_index: 0,
                tempo: 50,
                blocks_since_last_step: 0,
                current_block: 10,
            },
            reveal_period_epochs: 1,
            expected_reveal_block: Some(60),
            expected_error: None,
        },
        PredictVector {
            name: "pending_fires_before_auto",
            source: "subtensor tempo_control.rs pending epoch path",
            state: EpochScheduleState {
                last_epoch_block: 80,
                pending_epoch_at: 95,
                subnet_epoch_index: 0,
                tempo: 20,
                blocks_since_last_step: 0,
                current_block: 91,
            },
            reveal_period_epochs: 1,
            expected_reveal_block: Some(95),
            expected_error: None,
        },
    ]
}

pub struct CommitEpochVector {
    pub name: &'static str,
    pub state: EpochScheduleState,
    pub expected_commit_epoch: u64,
}

pub fn commit_epoch_vectors() -> &'static [CommitEpochVector] {
    &[CommitEpochVector {
        name: "commit_epoch_at_extrinsic_block",
        state: EpochScheduleState {
            last_epoch_block: 100,
            pending_epoch_at: 0,
            subnet_epoch_index: 0,
            tempo: 50,
            blocks_since_last_step: 0,
            current_block: 120,
        },
        expected_commit_epoch: 0,
    }]
}
