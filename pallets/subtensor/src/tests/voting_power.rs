#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use alloc::collections::BTreeMap;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_core::U256;
use subtensor_runtime_common::NetUid;

use super::mock;
use super::mock::*;
use crate::epoch::run_epoch::EpochTerms;
use crate::utils::voting_power::{
    MAX_VOTING_POWER_EMA_ALPHA, VOTING_POWER_DISABLE_GRACE_PERIOD_BLOCKS,
};
use crate::*;

// ============================================
// === Test Helpers ===
// ============================================

const DEFAULT_STAKE_AMOUNT: u64 = 1_000_000_000_000; // 1 million RAO

/// Build epoch output from current state for testing voting power updates.
fn build_mock_epoch_output(netuid: NetUid) -> BTreeMap<U256, EpochTerms> {
    let n = SubtensorModule::get_subnetwork_n(netuid);
    let validator_permits = ValidatorPermit::<Test>::get(netuid);

    let mut output = BTreeMap::new();
    for uid in 0..n {
        if let Ok(hotkey) = SubtensorModule::get_hotkey_for_net_and_uid(netuid, uid) {
            let has_permit = validator_permits
                .get(uid as usize)
                .copied()
                .unwrap_or(false);
            let stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&hotkey, netuid).to_u64();
            output.insert(
                hotkey,
                EpochTerms {
                    uid: uid as usize,
                    new_validator_permit: has_permit,
                    stake: stake.into(),
                    ..Default::default()
                },
            );
        }
    }
    output
}

/// Test fixture containing common test setup data
struct VotingPowerTestFixture {
    hotkey: U256,
    coldkey: U256,
    netuid: NetUid,
}

impl VotingPowerTestFixture {
    /// Create a basic fixture with a dynamic network
    fn new() -> Self {
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let netuid = add_dynamic_network(&hotkey, &coldkey);
        Self {
            hotkey,
            coldkey,
            netuid,
        }
    }

    /// Setup reserves and add balance to coldkey for staking
    fn setup_for_staking(&self) {
        self.setup_for_staking_with_amount(DEFAULT_STAKE_AMOUNT);
    }

    /// Setup reserves and add balance with custom amount
    #[allow(clippy::arithmetic_side_effects)]
    fn setup_for_staking_with_amount(&self, amount: u64) {
        mock::setup_reserves(self.netuid, (amount * 100).into(), (amount * 100).into());
        add_balance_to_coldkey_account(&self.coldkey, (amount * 10).into());
    }

    /// Enable voting power tracking for the subnet
    fn enable_tracking(&self) {
        assert_ok!(SubtensorModule::enable_voting_power_tracking(
            RuntimeOrigin::signed(self.coldkey),
            self.netuid
        ));
    }

    /// Add stake from coldkey to hotkey
    fn add_stake(&self, amount: u64) {
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(self.coldkey),
            self.hotkey,
            self.netuid,
            amount.into()
        ));
    }

    /// Set validator permit for the hotkey (uid 0)
    fn set_validator_permit(&self, has_permit: bool) {
        ValidatorPermit::<Test>::insert(self.netuid, vec![has_permit]);
    }

    /// Run voting power update for N epochs
    fn run_epochs(&self, n: u32) {
        for _ in 0..n {
            let epoch_output = build_mock_epoch_output(self.netuid);
            SubtensorModule::update_voting_power_for_subnet(self.netuid, &epoch_output);
        }
    }

    /// Get current voting power for the hotkey
    fn get_voting_power(&self) -> u64 {
        SubtensorModule::get_voting_power(self.netuid, &self.hotkey)
    }

    /// Full setup: reserves, balance, tracking enabled, stake added, validator permit
    fn setup_full(&self) {
        self.setup_for_staking();
        self.enable_tracking();
        self.add_stake(DEFAULT_STAKE_AMOUNT);
        self.set_validator_permit(true);
    }
}

// ============================================
// === Test Enable/Disable Voting Power ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_enable_voting_power_tracking --exact --nocapture
#[test]
fn test_enable_voting_power_tracking() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();

        // Initially disabled
        assert!(!SubtensorModule::get_voting_power_tracking_enabled(
            f.netuid
        ));

        // Enable tracking (subnet owner can do this)
        f.enable_tracking();

        // Now enabled
        assert!(SubtensorModule::get_voting_power_tracking_enabled(f.netuid));
        assert_eq!(
            SubtensorModule::get_voting_power_disable_at_block(f.netuid),
            0
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_enable_voting_power_tracking_root_can_enable --exact --nocapture
#[test]
fn test_enable_voting_power_tracking_root_can_enable() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();

        // Root can enable
        assert_ok!(SubtensorModule::enable_voting_power_tracking(
            RuntimeOrigin::root(),
            f.netuid
        ));

        assert!(SubtensorModule::get_voting_power_tracking_enabled(f.netuid));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_disable_voting_power_tracking_schedules_disable --exact --nocapture
#[test]
fn test_disable_voting_power_tracking_schedules_disable() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.enable_tracking();

        let current_block = SubtensorModule::get_current_block_as_u64();

        // Schedule disable
        assert_ok!(SubtensorModule::disable_voting_power_tracking(
            RuntimeOrigin::signed(f.coldkey),
            f.netuid
        ));

        // Still enabled, but scheduled for disable
        assert!(SubtensorModule::get_voting_power_tracking_enabled(f.netuid));
        let disable_at = SubtensorModule::get_voting_power_disable_at_block(f.netuid);
        assert_eq!(
            disable_at,
            current_block + VOTING_POWER_DISABLE_GRACE_PERIOD_BLOCKS
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_disable_voting_power_tracking_fails_when_not_enabled --exact --nocapture
#[test]
fn test_disable_voting_power_tracking_fails_when_not_enabled() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();

        // Try to disable when not enabled
        assert_noop!(
            SubtensorModule::disable_voting_power_tracking(
                RuntimeOrigin::signed(f.coldkey),
                f.netuid
            ),
            Error::<Test>::VotingPowerTrackingNotEnabled
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_enable_voting_power_tracking_non_owner_fails --exact --nocapture
#[test]
fn test_enable_voting_power_tracking_non_owner_fails() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        let random_account = U256::from(999);

        // Non-owner cannot enable (returns BadOrigin)
        assert_noop!(
            SubtensorModule::enable_voting_power_tracking(
                RuntimeOrigin::signed(random_account),
                f.netuid
            ),
            sp_runtime::DispatchError::BadOrigin
        );

        // Should still be disabled
        assert!(!SubtensorModule::get_voting_power_tracking_enabled(
            f.netuid
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_disable_voting_power_tracking_non_owner_fails --exact --nocapture
#[test]
fn test_disable_voting_power_tracking_non_owner_fails() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        let random_account = U256::from(999);
        f.enable_tracking();

        // Non-owner cannot disable (returns BadOrigin)
        assert_noop!(
            SubtensorModule::disable_voting_power_tracking(
                RuntimeOrigin::signed(random_account),
                f.netuid
            ),
            sp_runtime::DispatchError::BadOrigin
        );

        // Should still be enabled with no disable scheduled
        assert!(SubtensorModule::get_voting_power_tracking_enabled(f.netuid));
        assert_eq!(
            SubtensorModule::get_voting_power_disable_at_block(f.netuid),
            0
        );
    });
}

// ============================================
// === Test EMA Alpha ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_set_voting_power_ema_alpha --exact --nocapture
#[test]
fn test_set_voting_power_ema_alpha() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();

        // Get default alpha
        let default_alpha = SubtensorModule::get_voting_power_ema_alpha(f.netuid);
        assert_eq!(default_alpha, 3_570_000_000_000_000); // 0.00357 * 10^18 = 2 weeks e-folding

        // Set new alpha (only root can do this)
        let new_alpha: u64 = 500_000_000_000_000_000; // 0.5 * 10^18
        assert_ok!(SubtensorModule::sudo_set_voting_power_ema_alpha(
            RuntimeOrigin::root(),
            f.netuid,
            new_alpha
        ));

        assert_eq!(
            SubtensorModule::get_voting_power_ema_alpha(f.netuid),
            new_alpha
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_set_voting_power_ema_alpha_fails_above_one --exact --nocapture
#[test]
fn test_set_voting_power_ema_alpha_fails_above_one() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();

        // Try to set alpha > 1.0 (> 10^18)
        let invalid_alpha: u64 = MAX_VOTING_POWER_EMA_ALPHA + 1;
        assert_noop!(
            SubtensorModule::sudo_set_voting_power_ema_alpha(
                RuntimeOrigin::root(),
                f.netuid,
                invalid_alpha
            ),
            Error::<Test>::InvalidVotingPowerEmaAlpha
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_set_voting_power_ema_alpha_non_root_fails --exact --nocapture
#[test]
fn test_set_voting_power_ema_alpha_non_root_fails() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();

        // Non-root cannot set alpha
        assert_noop!(
            SubtensorModule::sudo_set_voting_power_ema_alpha(
                RuntimeOrigin::signed(f.coldkey),
                f.netuid,
                500_000_000_000_000_000
            ),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

// ============================================
// === Test EMA Calculation ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_ema_calculation --exact --nocapture
#[test]
fn test_voting_power_ema_calculation() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_full();

        // Initially voting power is 0
        assert_eq!(f.get_voting_power(), 0);

        // Run epoch to update voting power
        f.run_epochs(1);

        // Voting power should now be > 0 (but less than full stake due to EMA starting from 0)
        let voting_power_after_first_epoch = f.get_voting_power();
        assert!(voting_power_after_first_epoch > 0);

        // Run more epochs - voting power should increase towards stake
        f.run_epochs(10);

        let voting_power_after_many_epochs = f.get_voting_power();
        assert!(voting_power_after_many_epochs > voting_power_after_first_epoch);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_cleared_when_deregistered --exact --nocapture
#[test]
fn test_voting_power_cleared_when_deregistered() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_full();

        // Run epochs to build up voting power
        f.run_epochs(10);

        let voting_power_before = f.get_voting_power();
        assert!(voting_power_before > 0, "Voting power should be built up");

        // Deregister the hotkey (simulate by removing from IsNetworkMember)
        IsNetworkMember::<Test>::remove(f.hotkey, f.netuid);

        // Run epoch - voting power should be cleared for deregistered hotkey
        f.run_epochs(1);

        // Should be removed from storage immediately when deregistered
        assert_eq!(f.get_voting_power(), 0);
        assert!(
            !VotingPower::<Test>::contains_key(f.netuid, f.hotkey),
            "Entry should be removed when hotkey is deregistered"
        );
    });
}

// ============================================
// === Test Validators Only ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_only_validators_get_voting_power --exact --nocapture
#[test]
fn test_only_validators_get_voting_power() {
    new_test_ext(1).execute_with(|| {
        let validator_hotkey = U256::from(1);
        let miner_hotkey = U256::from(2);
        let coldkey = U256::from(3);

        let netuid = add_dynamic_network(&validator_hotkey, &coldkey);

        mock::setup_reserves(
            netuid,
            (DEFAULT_STAKE_AMOUNT * 100).into(),
            (DEFAULT_STAKE_AMOUNT * 100).into(),
        );
        add_balance_to_coldkey_account(&coldkey, (DEFAULT_STAKE_AMOUNT * 20).into());

        // Register miner
        register_ok_neuron(netuid, miner_hotkey, coldkey, 0);

        // Enable voting power tracking
        assert_ok!(SubtensorModule::enable_voting_power_tracking(
            RuntimeOrigin::signed(coldkey),
            netuid
        ));

        // Add stake to both
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            validator_hotkey,
            netuid,
            DEFAULT_STAKE_AMOUNT.into()
        ));
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            miner_hotkey,
            netuid,
            DEFAULT_STAKE_AMOUNT.into()
        ));

        // Set validator permit: uid 0 (validator) has permit, uid 1 (miner) does not
        ValidatorPermit::<Test>::insert(netuid, vec![true, false]);

        // Run epoch
        let epoch_output = build_mock_epoch_output(netuid);
        SubtensorModule::update_voting_power_for_subnet(netuid, &epoch_output);

        // Only validator should have voting power
        assert!(SubtensorModule::get_voting_power(netuid, &validator_hotkey) > 0);
        assert_eq!(SubtensorModule::get_voting_power(netuid, &miner_hotkey), 0);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_miner_voting_power_removed_when_loses_vpermit --exact --nocapture
#[test]
fn test_miner_voting_power_removed_when_loses_vpermit() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_full();

        // Run epochs to build voting power
        f.run_epochs(10);

        let voting_power_before = f.get_voting_power();
        assert!(voting_power_before > 0);

        // Remove validator permit (now they're a miner)
        f.set_validator_permit(false);

        // Run epoch - voting power should be removed
        f.run_epochs(1);

        assert_eq!(f.get_voting_power(), 0);
    });
}

// ============================================
// === Test Hotkey Swap ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_transfers_on_hotkey_swap --exact --nocapture
#[test]
fn test_voting_power_transfers_on_hotkey_swap() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        let new_hotkey = U256::from(99);
        let voting_power_value = 5_000_000_000_000_u64;

        // Set some voting power for the old hotkey
        VotingPower::<Test>::insert(f.netuid, f.hotkey, voting_power_value);

        // Verify old hotkey has voting power
        assert_eq!(f.get_voting_power(), voting_power_value);
        assert_eq!(SubtensorModule::get_voting_power(f.netuid, &new_hotkey), 0);

        // Perform hotkey swap for this subnet
        SubtensorModule::swap_voting_power_for_hotkey(&f.hotkey, &new_hotkey, f.netuid);

        // Old hotkey should have 0, new hotkey should have the voting power
        assert_eq!(f.get_voting_power(), 0);
        assert_eq!(
            SubtensorModule::get_voting_power(f.netuid, &new_hotkey),
            voting_power_value
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_swap_adds_to_existing --exact --nocapture
#[test]
fn test_voting_power_swap_adds_to_existing() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        let new_hotkey = U256::from(99);
        let old_voting_power = 5_000_000_000_000_u64;
        let new_existing_voting_power = 2_000_000_000_000_u64;

        // Set voting power for both hotkeys
        VotingPower::<Test>::insert(f.netuid, f.hotkey, old_voting_power);
        VotingPower::<Test>::insert(f.netuid, new_hotkey, new_existing_voting_power);

        // Perform swap
        SubtensorModule::swap_voting_power_for_hotkey(&f.hotkey, &new_hotkey, f.netuid);

        // New hotkey should have combined voting power
        assert_eq!(f.get_voting_power(), 0);
        assert_eq!(
            SubtensorModule::get_voting_power(f.netuid, &new_hotkey),
            old_voting_power + new_existing_voting_power
        );
    });
}

// ============================================
// === Test Threshold Logic ===
// ============================================
// Tests the rule: Only remove voting power entry if it decayed FROM above threshold TO below.
// New validators building up from 0 should NOT be removed even if below threshold.

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_not_removed_if_never_above_threshold --exact --nocapture
#[test]
fn test_voting_power_not_removed_if_never_above_threshold() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_full();

        // Get the threshold
        let min_stake = SubtensorModule::get_stake_threshold();

        // Set voting power directly to a value below threshold (simulating building up)
        // This is below threshold but was never above it
        let below_threshold = min_stake.saturating_sub(1);
        VotingPower::<Test>::insert(f.netuid, f.hotkey, below_threshold);

        // Run epoch
        f.run_epochs(1);

        // Key assertion: Entry should NOT be removed because previous_ema was below threshold
        // The removal rule only triggers when previous_ema >= threshold and new_ema < threshold
        let voting_power = f.get_voting_power();
        assert!(
            voting_power > 0,
            "Voting power should still exist - it was never above threshold"
        );
        assert!(
            VotingPower::<Test>::contains_key(f.netuid, f.hotkey),
            "Entry should exist - it was never above threshold so shouldn't be removed"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_not_removed_with_small_dip_below_threshold --exact --nocapture
#[test]
fn test_voting_power_not_removed_with_small_dip_below_threshold() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_for_staking();
        f.enable_tracking();
        f.set_validator_permit(true);

        let min_stake = SubtensorModule::get_stake_threshold();

        // Set voting power above threshold (validator was established)
        let above_threshold = min_stake + 100;
        VotingPower::<Test>::insert(f.netuid, f.hotkey, above_threshold);

        // Simulate a small dip: new EMA drops to 95% of threshold (within 10% buffer)
        // This is above the removal threshold (90%) so should NOT be removed
        let small_dip = min_stake * 95 / 100;
        VotingPower::<Test>::insert(f.netuid, f.hotkey, small_dip);

        // Manually trigger the removal check by setting previous to above threshold
        // and running with stake that would produce EMA in the buffer zone
        VotingPower::<Test>::insert(f.netuid, f.hotkey, above_threshold);

        // Build epoch output with stake that will produce EMA around 95% of threshold
        let mut epoch_output = build_mock_epoch_output(f.netuid);
        if let Some(terms) = epoch_output.get_mut(&f.hotkey) {
            terms.stake = small_dip.into(); // Stake drops but stays in buffer zone
        }

        SubtensorModule::update_voting_power_for_subnet(f.netuid, &epoch_output);

        // Should NOT be removed - dip is within hysteresis buffer
        assert!(
            VotingPower::<Test>::contains_key(f.netuid, f.hotkey),
            "Entry should exist - small dip within 10% buffer should not trigger removal"
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_removed_with_significant_drop_below_threshold --exact --nocapture
#[test]
fn test_voting_power_removed_with_significant_drop_below_threshold() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.enable_tracking();

        // Use explicit values since get_stake_threshold() may return 0 in tests
        let min_stake: u64 = 1_000_000_000;
        StakeThreshold::<Test>::put(min_stake);

        // Set voting power above threshold (validator was established)
        VotingPower::<Test>::insert(f.netuid, f.hotkey, min_stake);

        // Set alpha to 100% so new_ema = current_stake directly (for testing removal)
        VotingPowerEmaAlpha::<Test>::insert(f.netuid, MAX_VOTING_POWER_EMA_ALPHA);

        // Build epoch output manually with stake = 0 and validator permit = true
        let mut epoch_output = BTreeMap::new();
        epoch_output.insert(
            f.hotkey,
            EpochTerms {
                uid: 0,
                new_validator_permit: true,
                stake: 0.into(), // Complete unstake
                ..Default::default()
            },
        );

        // With alpha = 1.0: new_ema = 1.0 * 0 + 0 * previous = 0
        // 0 < removal_threshold (90% of min_stake = 900M) AND previous (1B) >= min_stake (1B)
        // Should trigger removal
        SubtensorModule::update_voting_power_for_subnet(f.netuid, &epoch_output);

        assert!(
            !VotingPower::<Test>::contains_key(f.netuid, f.hotkey),
            "Entry should be removed - stake dropped to 0 with alpha=1.0"
        );
    });
}

// ============================================
// === Test Tracking Not Active ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_not_updated_when_disabled --exact --nocapture
#[test]
fn test_voting_power_not_updated_when_disabled() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_for_staking();
        // DON'T enable voting power tracking
        f.add_stake(DEFAULT_STAKE_AMOUNT);
        f.set_validator_permit(true);

        // Run epoch
        f.run_epochs(1);

        // Voting power should still be 0 since tracking is disabled
        assert_eq!(f.get_voting_power(), 0);
    });
}

// ============================================
// === Test Re-enable After Disable ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_reenable_voting_power_clears_disable_schedule --exact --nocapture
#[test]
fn test_reenable_voting_power_clears_disable_schedule() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.enable_tracking();

        // Schedule disable
        assert_ok!(SubtensorModule::disable_voting_power_tracking(
            RuntimeOrigin::signed(f.coldkey),
            f.netuid
        ));

        assert!(SubtensorModule::get_voting_power_disable_at_block(f.netuid) > 0);

        // Re-enable should clear the disable schedule
        f.enable_tracking();

        assert!(SubtensorModule::get_voting_power_tracking_enabled(f.netuid));
        assert_eq!(
            SubtensorModule::get_voting_power_disable_at_block(f.netuid),
            0
        );
    });
}

// ============================================
// === Test Grace Period Finalization ===
// ============================================

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_finalized_after_grace_period --exact --nocapture
#[test]
fn test_voting_power_finalized_after_grace_period() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_full();

        // Build up voting power
        f.run_epochs(10);

        let voting_power_before = f.get_voting_power();
        assert!(voting_power_before > 0);

        // Schedule disable
        assert_ok!(SubtensorModule::disable_voting_power_tracking(
            RuntimeOrigin::signed(f.coldkey),
            f.netuid
        ));

        let disable_at = SubtensorModule::get_voting_power_disable_at_block(f.netuid);

        // Advance block past grace period (time travel!)
        System::set_block_number(disable_at + 1);

        // Run epoch - should finalize disable
        f.run_epochs(1);

        // Tracking should be disabled and all entries cleared
        assert!(!SubtensorModule::get_voting_power_tracking_enabled(
            f.netuid
        ));
        assert_eq!(
            SubtensorModule::get_voting_power_disable_at_block(f.netuid),
            0
        );
        assert_eq!(f.get_voting_power(), 0);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::voting_power::test_voting_power_continues_during_grace_period --exact --nocapture
#[test]
fn test_voting_power_continues_during_grace_period() {
    new_test_ext(1).execute_with(|| {
        let f = VotingPowerTestFixture::new();
        f.setup_full();

        // Schedule disable
        assert_ok!(SubtensorModule::disable_voting_power_tracking(
            RuntimeOrigin::signed(f.coldkey),
            f.netuid
        ));

        let disable_at = SubtensorModule::get_voting_power_disable_at_block(f.netuid);

        // Set block to middle of grace period (time travel!)
        System::set_block_number(disable_at - 1000);

        // Run epoch - should still update voting power during grace period
        f.run_epochs(1);

        // Tracking should still be enabled and voting power should exist
        assert!(SubtensorModule::get_voting_power_tracking_enabled(f.netuid));
        assert!(f.get_voting_power() > 0);
    });
}
