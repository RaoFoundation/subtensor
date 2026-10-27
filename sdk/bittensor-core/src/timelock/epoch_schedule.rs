//! Epoch scheduling state machine for the stateful tempo model.
//!
//! Ports a minimal subset of `subtensor` epoch logic required to predict
//! reveal blocks for timelock-encrypted commits.  See `constants.rs` for
//! chain-verified bounds.

use crate::timelock::constants::{COMMIT_INCLUSION_BLOCK_OFFSET, MAX_TEMPO_U64};

/// Snapshot of on-chain epoch schedule state at a given block.
///
/// Mirrors the Python `EpochScheduleState` dataclass and contains
/// exactly the fields the SDK reads from storage before calling
/// `generate_commit_v2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochScheduleState {
    pub last_epoch_block: u64,
    pub pending_epoch_at: u64,
    pub subnet_epoch_index: u64,
    pub tempo: u16,
    pub blocks_since_last_step: u64,
    pub current_block: u64,
}

/// Error returned when the reveal-block simulation exceeds its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpochScheduleError {
    BoundExceeded,
    TempoIsZero,
}

impl std::fmt::Display for EpochScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BoundExceeded => write!(f, "reveal block simulation exceeded budget"),
            Self::TempoIsZero => write!(f, "tempo is zero; subnet does not run epochs"),
        }
    }
}

impl std::error::Error for EpochScheduleError {}

/// Whether the pallet's epoch predicate is true for the supplied stored state.
pub fn should_run_epoch(state: &EpochScheduleState, block: u64) -> bool {
    if state.tempo == 0 {
        return false;
    }
    if state.pending_epoch_at > 0 && block >= state.pending_epoch_at {
        return true;
    }
    if state.blocks_since_last_step > u64::from(state.tempo) {
        return true;
    }
    block.saturating_sub(state.last_epoch_block) >= u64::from(state.tempo)
}

/// The look-ahead epoch seen by reveal processing before `run_coinbase`.
pub fn current_epoch_pre_run_coinbase(state: &EpochScheduleState, block: u64) -> u64 {
    if should_run_epoch(state, block) {
        state.subnet_epoch_index.saturating_add(1)
    } else {
        state.subnet_epoch_index
    }
}

/// Advance one block, applying the pallet's bounded counter and a successful epoch.
pub fn simulate_run_coinbase(state: &EpochScheduleState, block: u64) -> EpochScheduleState {
    let mut next = state.clone();
    next.blocks_since_last_step = next
        .blocks_since_last_step
        .saturating_add(1)
        .min(u64::from(next.tempo).saturating_add(1));
    next.current_block = block;
    if should_run_epoch(&next, block) {
        next.last_epoch_block = block;
        next.pending_epoch_at = 0;
        next.subnet_epoch_index = next.subnet_epoch_index.saturating_add(1);
        next.blocks_since_last_step = 0;
    }
    next
}

/// Advance every block in `start..=end`; an empty range returns the original state.
pub fn advance_blocks(from: &EpochScheduleState, start: u64, end: u64) -> EpochScheduleState {
    let mut state = from.clone();
    for block in start..=end {
        state = simulate_run_coinbase(&state, block);
    }
    state
}

/// Predict the first block of the commit's actual reveal epoch.
///
/// Commit extrinsics run after `run_coinbase`, while automatic reveal processing
/// runs before it. Counter-threshold fires can occur only after the coinbase
/// increment, so compute the commit epoch from the simulated post-coinbase state.
pub fn predict_first_reveal_block(
    head_state: &EpochScheduleState,
    reveal_period_epochs: u64,
) -> Result<u64, EpochScheduleError> {
    if head_state.tempo == 0 {
        return Err(EpochScheduleError::TempoIsZero);
    }
    let first_block = head_state
        .current_block
        .checked_add(1)
        .ok_or(EpochScheduleError::BoundExceeded)?;
    let extrinsic_block = head_state
        .current_block
        .checked_add(COMMIT_INCLUSION_BLOCK_OFFSET)
        .ok_or(EpochScheduleError::BoundExceeded)?;
    let before_extrinsic =
        advance_blocks(head_state, first_block, extrinsic_block.saturating_sub(1));
    let after_extrinsic = simulate_run_coinbase(&before_extrinsic, extrinsic_block);
    let commit_epoch = current_epoch_pre_run_coinbase(&after_extrinsic, extrinsic_block);
    let target_epoch = commit_epoch
        .checked_add(reveal_period_epochs)
        .ok_or(EpochScheduleError::BoundExceeded)?;
    let budget = reveal_period_epochs
        .checked_mul(MAX_TEMPO_U64)
        .and_then(|blocks| blocks.checked_add(MAX_TEMPO_U64))
        .ok_or(EpochScheduleError::BoundExceeded)?;
    let end = extrinsic_block
        .checked_add(budget)
        .ok_or(EpochScheduleError::BoundExceeded)?;
    let mut previous = before_extrinsic;
    for block in extrinsic_block..=end {
        if current_epoch_pre_run_coinbase(&previous, block) == target_epoch {
            return Ok(block);
        }
        previous = simulate_run_coinbase(&previous, block);
    }
    Err(EpochScheduleError::BoundExceeded)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_run_epoch_tempo_zero() {
        let state = EpochScheduleState {
            last_epoch_block: 100,
            pending_epoch_at: 0,
            subnet_epoch_index: 0,
            tempo: 0,
            blocks_since_last_step: 0,
            current_block: 120,
        };
        assert!(!should_run_epoch(&state, 200));
    }

    #[test]
    fn should_run_epoch_auto_fire() {
        let state = EpochScheduleState {
            last_epoch_block: 100,
            pending_epoch_at: 0,
            subnet_epoch_index: 0,
            tempo: 50,
            blocks_since_last_step: 0,
            current_block: 120,
        };
        assert!(!should_run_epoch(&state, 149));
        assert!(should_run_epoch(&state, 150));
    }
}

#[cfg(test)]
mod integration {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::timelock::constants::COMMIT_INCLUSION_BLOCK_OFFSET;
    use crate::timelock::epoch_schedule_vectors::{commit_epoch_vectors, predict_vectors};

    #[test]
    fn predict_first_reveal_block_table() {
        for case in predict_vectors() {
            let result = predict_first_reveal_block(&case.state, case.reveal_period_epochs);
            match (&case.expected_error, &case.expected_reveal_block) {
                (Some(expected_err), None) => {
                    assert_eq!(result, Err(expected_err.clone()), "case {}", case.name);
                }
                (None, Some(expected_block)) => {
                    assert_eq!(
                        result.unwrap(),
                        *expected_block,
                        "case {} ({})",
                        case.name,
                        case.source
                    );
                }
                _ => panic!("invalid vector definition for {}", case.name),
            }
        }
    }

    #[test]
    fn commit_epoch_at_extrinsic_block_table() {
        for case in commit_epoch_vectors() {
            let head_block = case.state.current_block;
            let extrinsic_block = head_block + COMMIT_INCLUSION_BLOCK_OFFSET;
            let post_before = if extrinsic_block == head_block + 1 {
                case.state.clone()
            } else {
                advance_blocks(&case.state, head_block + 1, extrinsic_block - 1)
            };
            let commit_epoch = current_epoch_pre_run_coinbase(&post_before, extrinsic_block);
            assert_eq!(
                commit_epoch, case.expected_commit_epoch,
                "case {}",
                case.name
            );
        }
    }

    #[test]
    fn reveal_uses_exact_equality_not_gte() {
        let case = &predict_vectors()[2];
        let reveal_block = predict_first_reveal_block(&case.state, case.reveal_period_epochs)
            .expect("expected reveal block");
        assert!(reveal_block > case.state.current_block);

        let prior = reveal_block.saturating_sub(1);
        let head = case.state.current_block;
        let extrinsic_block = head + COMMIT_INCLUSION_BLOCK_OFFSET;
        let post_before = if extrinsic_block == head + 1 {
            case.state.clone()
        } else {
            advance_blocks(&case.state, head + 1, extrinsic_block - 1)
        };
        let commit_epoch = current_epoch_pre_run_coinbase(&post_before, extrinsic_block);
        let target_epoch = commit_epoch + case.reveal_period_epochs;

        let mut post_prev = post_before;
        for r in extrinsic_block..prior {
            if current_epoch_pre_run_coinbase(&post_prev, r) == target_epoch {
                panic!("reveal block {prior} should not satisfy equality yet (found at {r})");
            }
            post_prev = simulate_run_coinbase(&post_prev, r);
        }
        assert_eq!(
            current_epoch_pre_run_coinbase(&post_prev, reveal_block),
            target_epoch
        );
    }
}
