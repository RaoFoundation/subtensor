use super::*;
use frame_support::{traits::Get, weights::Weight};
use log;
use scale_info::prelude::string::String;
use sp_core::H256;
use sp_std::collections::vec_deque::VecDeque;

/// One-shot migration for the dynamic-tempo / owner-triggered-epochs feature.
///
/// 1. Back-fills `LastEpochBlock[netuid]` for every existing subnet so the first
///    post-upgrade epoch lands on the same block as the legacy modulo formula
///    `(block + netuid + 1) % (tempo + 1) == 0`. The new scheduler period is
///    `tempo` (next firing at `LastEpochBlock + tempo`).
///    Existing `Tempo[netuid]` values are preserved as-is regardless of whether
///    they fall inside `[MIN_TEMPO, MAX_TEMPO]`. Owner-side `set_tempo` enforces
///    the bounds for new updates; root-side `sudo_set_tempo` can still write any
///    `u16`. Subnets with `Tempo == 0` are left as-is — the legacy short-circuit
///    keeps them dormant and matches their pre-upgrade behaviour.
/// 2. Converts each subnet's existing `ActivityCutoff[netuid]` (absolute block count)
///    into `ActivityCutoffFactorMilli[netuid]` (per-mille of `tempo`) so that
///    `factor * tempo / 1000 ≈ old_cutoff` post-upgrade. Production defaults
///    (`tempo=360`, `cutoff=5000`) round-trip to 5000 blocks exactly via ceiling
///    division. Out-of-range factors are clamped to
///    `[MIN_ACTIVITY_CUTOFF_FACTOR_MILLI, MAX_ACTIVITY_CUTOFF_FACTOR_MILLI]` —
///    extreme historical cutoffs may shift to the nearest representable factor.
/// 3. Seeds `SubnetEpochIndex[netuid]` (the new stateful epoch counter) with the
///    legacy modulo epoch index `(block + netuid + 1) / (tempo + 1)` so that
///    existing commit-reveal commit keys — `TimelockedWeightCommits` (CR-v4) keyed
///    by epoch, and `WeightCommits` (CR-v2) tagged with `commit_epoch` — stay
///    valid and continuous across the upgrade.
/// 4. Rewrites every CR-v2 `WeightCommits` entry to `(hash, commit_epoch,
///    commit_block, _)`: field 1 (previously the absolute `commit_block`) becomes
///    `commit_epoch` under the legacy modulo formula; field 2 keeps the absolute
///    `commit_block` (used by the epoch's commit-reveal weight column-mask).
pub fn migrate_dynamic_tempo<T: Config>() -> Weight {
    let mig_name: Vec<u8> = b"dynamic_tempo_v1".to_vec();
    let mig_name_str = String::from_utf8_lossy(&mig_name);

    let mut total_weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&mig_name) {
        log::info!("Migration '{mig_name_str}' already executed - skipping");
        return total_weight;
    }

    log::info!("Running migration '{mig_name_str}'");

    let current_block = Pallet::<T>::get_current_block_as_u64();
    let mut visited: u64 = 0;
    let mut last_epoch_seeded: u64 = 0;
    let mut epoch_index_seeded: u64 = 0;
    let mut activity_factor_seeded: u64 = 0;
    let mut activity_factor_clamped: u64 = 0;
    let mut crv2_commits_converted: u64 = 0;
    let mut reads: u64 = 0;
    let mut writes: u64 = 0;

    let netuids: Vec<NetUid> = Tempo::<T>::iter_keys().collect();
    reads = reads.saturating_add(netuids.len() as u64);

    for netuid in netuids.into_iter() {
        visited = visited.saturating_add(1);
        let tempo = Tempo::<T>::get(netuid);
        reads = reads.saturating_add(1);

        if tempo == 0 {
            // Legacy `tempo == 0` short-circuit preserved; do not seed `LastEpochBlock`.
            continue;
        }

        // Compute next-epoch block under the *legacy* modulo formula and back-fill
        // `LastEpochBlock` so the *new* scheduler fires its first epoch on the same
        // block the legacy chain would have.
        // Legacy `blocks_until_next_epoch` (pre-upgrade behaviour, period `tempo + 1`):
        //   adjusted = current_block + netuid + 1
        //   remainder = adjusted % (tempo + 1)
        //   blocks_until_next = tempo - remainder
        // New scheduler period is `tempo`, next firing at `LastEpochBlock + tempo`.
        // Solve for `LastEpochBlock`:
        //   LastEpochBlock = current_block + blocks_until_next - tempo
        //                  = current_block - (tempo - blocks_until_next)
        let netuid_plus_one = (u16::from(netuid) as u64).saturating_add(1);
        let tempo_plus_one = (tempo as u64).saturating_add(1);
        let adjusted = current_block.wrapping_add(netuid_plus_one);
        let remainder = adjusted.checked_rem(tempo_plus_one).unwrap_or(0);
        let blocks_until_next = (tempo as u64).saturating_sub(remainder);
        let offset = (tempo as u64).saturating_sub(blocks_until_next);
        let last_epoch = current_block.saturating_sub(offset);

        LastEpochBlock::<T>::insert(netuid, last_epoch);
        last_epoch_seeded = last_epoch_seeded.saturating_add(1);
        writes = writes.saturating_add(1);

        // Seed the stateful epoch counter with the legacy modulo epoch index
        // `(current_block + netuid + 1) / (tempo + 1)` so CR commit keys
        // (TimelockedWeightCommits epoch keys, WeightCommits commit_epoch) stay
        // continuous across the upgrade.
        let legacy_epoch = adjusted.checked_div(tempo_plus_one).unwrap_or(0);
        SubnetEpochIndex::<T>::insert(netuid, legacy_epoch);
        epoch_index_seeded = epoch_index_seeded.saturating_add(1);
        writes = writes.saturating_add(1);

        // Convert legacy absolute `ActivityCutoff` into per-mille `ActivityCutoffFactorMilli`
        let old_cutoff = ActivityCutoff::<T>::get(netuid) as u64;
        reads = reads.saturating_add(1);
        let tempo_u64 = tempo as u64;
        let raw_factor = old_cutoff
            .saturating_mul(1_000)
            .saturating_add(tempo_u64.saturating_sub(1))
            .checked_div(tempo_u64)
            .unwrap_or(INITIAL_ACTIVITY_CUTOFF_FACTOR_MILLI as u64);
        let clamped = raw_factor
            .max(MIN_ACTIVITY_CUTOFF_FACTOR_MILLI as u64)
            .min(MAX_ACTIVITY_CUTOFF_FACTOR_MILLI as u64) as u32;
        if clamped as u64 != raw_factor {
            activity_factor_clamped = activity_factor_clamped.saturating_add(1);
        }
        ActivityCutoffFactorMilli::<T>::insert(netuid, clamped);
        activity_factor_seeded = activity_factor_seeded.saturating_add(1);
        writes = writes.saturating_add(1);
    }

    // --- CR-v2: rewrite every `WeightCommits` entry to the
    // `(hash, commit_epoch, commit_block, _)` layout. Field 1 was the absolute
    // `commit_block`; it becomes `commit_epoch` (legacy modulo epoch). Field 2
    // keeps the absolute `commit_block` (used by the epoch column-mask).
    let crv2_entries: Vec<_> = WeightCommits::<T>::iter().collect();
    reads = reads.saturating_add(crv2_entries.len() as u64);
    for (netuid_index, account, commits) in crv2_entries.into_iter() {
        let (netuid, _) = Pallet::<T>::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let tempo = Tempo::<T>::get(netuid);
        reads = reads.saturating_add(1);
        let tempo_plus_one = (tempo as u64).saturating_add(1);
        let netuid_plus_one = (u16::from(netuid) as u64).saturating_add(1);

        let converted: VecDeque<(H256, u64, u64, u64)> = commits
            .into_iter()
            .map(|(hash, commit_block, _, _)| {
                let commit_epoch = commit_block
                    .saturating_add(netuid_plus_one)
                    .checked_div(tempo_plus_one)
                    .unwrap_or(0);
                (hash, commit_epoch, commit_block, 0u64)
            })
            .collect();
        WeightCommits::<T>::insert(netuid_index, account, converted);
        crv2_commits_converted = crv2_commits_converted.saturating_add(1);
        writes = writes.saturating_add(1);
    }

    total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(reads, writes));

    log::info!(
        "Dynamic tempo migration: visited={visited}, last_epoch_seeded={last_epoch_seeded}, epoch_index_seeded={epoch_index_seeded}, activity_factor_seeded={activity_factor_seeded}, activity_factor_clamped={activity_factor_clamped}, crv2_commits_converted={crv2_commits_converted}"
    );

    HasMigrationRun::<T>::insert(&mig_name, true);
    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!("Migration '{mig_name_str}' completed");

    total_weight
}
