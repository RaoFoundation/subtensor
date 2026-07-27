//! Epoch behavior asserted via captured tracing logs ([`crate::epoch`]).
//!
//! Covers inactive/permit masks, Yuma bonds pipelines, and multi-mechanism weights.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

// Run all tests
// cargo test --package pallet-subtensor --lib -- tests::epoch_logs --show-output

use super::mock::*;
use crate::*;
use frame_support::assert_ok;
use sp_core::U256;
use std::io::{Result as IoResult, Write};
use std::sync::{Arc, Mutex};
use subtensor_runtime_common::{AlphaBalance, MechId};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt};

const NETUID: u16 = 1;
const TEMPO: u16 = 10;
const ACTIVITY_CUTOFF: u16 = 10;
// Coldkey is irrelevant for epoch because it operates only hotkeys. Nominators' stake is distributed downstream
// We can use a single coldkey for all tests here.
const COLDKEY: u16 = 9876;

#[derive(Clone)]
struct Neuron {
    uid: u16,
    hotkey: U256,
    validator: bool,
    alpha_stake: u64,
    registration_block: u64,
    last_update: u64,
}

impl Neuron {
    fn new(
        uid: u16,
        hotkey: u16,
        validator: bool,
        alpha_stake: u64,
        registration_block: u64,
        last_update: u64,
    ) -> Self {
        Neuron {
            uid,
            hotkey: U256::from(hotkey),
            validator,
            alpha_stake,
            registration_block,
            last_update,
        }
    }
}

fn setup_epoch(neurons: Vec<Neuron>, mechanism_count: u8) {
    let netuid = NetUid::from(NETUID);

    // Setup subnet parameters
    NetworksAdded::<Test>::insert(netuid, true);
    let network_n = neurons.len() as u16;
    SubnetworkN::<Test>::insert(netuid, network_n);
    ActivityCutoff::<Test>::insert(netuid, ACTIVITY_CUTOFF);
    Tempo::<Test>::insert(netuid, TEMPO);
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);
    MechanismCountCurrent::<Test>::insert(netuid, MechId::from(mechanism_count));

    // Setup neurons
    let mut last_update_vec: Vec<u64> = Vec::new();
    let mut permit_vec: Vec<bool> = Vec::new();
    neurons.iter().for_each(|neuron| {
        let hotkey = U256::from(neuron.hotkey);

        Keys::<Test>::insert(netuid, neuron.uid, hotkey);
        Uids::<Test>::insert(netuid, hotkey, neuron.uid);
        BlockAtRegistration::<Test>::insert(netuid, neuron.uid, neuron.registration_block);
        last_update_vec.push(neuron.last_update);
        permit_vec.push(neuron.validator);

        // Setup stake
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &U256::from(COLDKEY),
            netuid,
            AlphaBalance::from(neuron.alpha_stake),
        );
    });

    ValidatorPermit::<Test>::insert(netuid, permit_vec);
    for m in 0..mechanism_count {
        let netuid_index = SubtensorModule::get_mechanism_storage_index(netuid, m.into());
        LastUpdate::<Test>::insert(netuid_index, last_update_vec.clone());
    }
}

fn set_weights(netuid: NetUid, weights: Vec<Vec<u16>>, indices: Vec<u16>) {
    for (uid, weight) in weights.iter().enumerate() {
        let hotkey = Keys::<Test>::get(netuid, uid as u16);
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey),
            netuid,
            indices.clone(),
            weight.to_vec(),
            0
        ));
    }
}

/// Write sparse weight rows **for a specific mechanism**.
/// `rows` is a list of `(validator_uid, row)` where `row` is `[(dest_uid, weight_u16)]`.
fn set_weights_for_mech(netuid: NetUid, mecid: MechId, rows: Vec<(u16, Vec<(u16, u16)>)>) {
    let netuid_index = SubtensorModule::get_mechanism_storage_index(netuid, mecid);
    for (uid, sparse_row) in rows {
        Weights::<Test>::insert(netuid_index, uid, sparse_row);
    }
}

/// Run `f` with a per-thread subscriber configured by `spec` and
/// return the captured log text.
pub fn with_log_capture<F, R>(spec: &str, f: F) -> String
where
    F: FnOnce() -> R,
{
    // ensure log::... is bridged to tracing (no-op if already set)
    let _ = tracing_log::LogTracer::init();

    // Shared buffer we'll write logs into
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> IoResult<usize> {
            let mut g = self.0.lock().unwrap();
            g.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    // Formatting layer that writes into our buffer
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .without_time()
        .with_writer({
            let buf = buf.clone();
            move || Buf(buf.clone())
        });

    // Parse filter spec (full RUST_LOG syntax). Fall back to "off" on error.
    let filter = EnvFilter::try_new(spec).unwrap_or_else(|_| EnvFilter::new("off"));

    // Build the per-thread subscriber
    let sub = tracing_subscriber::registry().with(filter).with(fmt_layer);

    // Activate just for this thread/scope and run the code
    tracing::subscriber::with_default(sub, f);

    // Collect captured text
    let mut g = buf.lock().unwrap();
    String::from_utf8(std::mem::take(&mut *g)).unwrap_or_default()
}

#[test]
fn test_simple() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey   vali            stake    reg   upd
                Neuron::new(  0,      1,  true,   1_000_000_000,     0,    1 ),
                Neuron::new(  1,      2,  true,   1_000_000_000,     0,    1 ),
            ];
            setup_epoch(neurons.to_vec(), 1);

            // Run epoch, watch logs
            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(NetUid::from(NETUID), MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);

        assert!(has("Number of Neurons in Network: 2"));
        assert!(has("Inactive: [false, false]"));
        assert!(has("Block at registration: [0, 0]"));
        assert!(has("alpha_stake: [1000000000, 1000000000]"));
        assert!(has("Filtered stake: [1000000000, 1000000000]"));
        assert!(has("Normalised Stake: [0.5, 0.5]"));
        assert!(has("validator_permits: [true, true]"));
        assert!(has("new_validator_permits: [true, true]"));
        assert!(has("Active Stake: [0.5, 0.5]"));
        assert!(has("Consensus: [0, 0]"));
        assert!(has("Normalized Validator Emission: [0.5, 0.5]"));
        assert!(has("Normalized Combined Emission: [0.5, 0.5]"));
        assert!(has(
            "Combined Emission: [AlphaBalance(500000000), AlphaBalance(500000000)]"
        ));
        assert!(!has("math error:"));
    });
}

#[test]
fn test_bad_permit_vector() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey   vali            stake    reg   upd
                Neuron::new(  0,      1,  true,   1_000_000_000,     0,    1 ),
                Neuron::new(  1,      2,  true,   1_000_000_000,     0,    1 ),
            ];
            setup_epoch(neurons.to_vec(), 1);
            ValidatorPermit::<Test>::insert(NetUid::from(NETUID), vec![true]);

            // Run epoch, watch logs
            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(NetUid::from(NETUID), MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);

        assert!(has(
            "math error: inplace_mask_vector input lengths are not equal"
        ));
        assert!(has(
            "Validator Emission: [AlphaBalance(1000000000), AlphaBalance(0)]"
        ));
    });
}

#[test]
fn test_inactive_mask_zeroes_active_stake() {
    // Big block so updated + activity_cutoff < current_block
    new_test_ext(1_000_000).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey  vali   stake          reg  upd
                Neuron::new(  0,     11, true,  1_000_000_000,  0,   0 ),
                Neuron::new(  1,     22, true,  1_000_000_000,  0,   0 ),
            ];
            setup_epoch(neurons.to_vec(), 1);

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(NetUid::from(NETUID), MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        assert!(has("Number of Neurons in Network: 2"));
        assert!(has("Inactive: [true, true]"));
        // After masking + renormalizing, both entries are zero.
        assert!(has("Active Stake: [0, 0]"));
    });
}

#[test]
fn test_validator_permit_masks_active_stake() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey  vali   stake          reg  upd
                Neuron::new(  0,     11, true,  1_000_000_000,  0,   1 ),
                Neuron::new(  1,     22, true,  1_000_000_000,  0,   1 ),
            ];
            setup_epoch(neurons.to_vec(), 1);

            // Forbid validator #1
            let netuid = NetUid::from(NETUID);
            ValidatorPermit::<Test>::insert(netuid, vec![true, false]);

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        assert!(has("validator_permits: [true, false]"));
        // After masking and renormalizing, only the first stays: [1, 0]
        assert!(logs.contains("Active Stake: [1, 0]"));
    });
}

#[test]
fn yuma_emergency_mode() {
    // Large block so everyone is inactive (updated + cutoff < current_block)
    new_test_ext(1_000_000).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey  vali   stake          reg  upd
                Neuron::new(  0,     11, true,  1_000_000_000,  0,   0 ),
                Neuron::new(  1,     22, true,  1_000_000_000,  0,   0 ),
            ];
            setup_epoch(neurons.to_vec(), 1);

            // No weights needed; keep defaults empty to make ranks/dividends zero.
            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(NetUid::from(NETUID), MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        assert!(has("Inactive: [true, true]"));
        // Because emission_sum == 0 and active_stake == 0, we expect fallback to normalized stake.
        assert!(has("Normalized Combined Emission: [0.5, 0.5]"));
    });
}

#[test]
fn epoch_uses_active_stake_when_nonzero_active() {
    new_test_ext(1000).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey  vali   stake          reg  upd
                Neuron::new(  0,     11, true,  1_000_000_000,  0,  999 ), // active
                Neuron::new(  1,     22, true,  1_000_000_000,  0,   1  ), // inactive
            ];
            setup_epoch(neurons.to_vec(), 1);

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(NetUid::from(NETUID), MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        assert!(has("Inactive: [false, true]"));
        // With ranks/dividends zero, fallback should mirror active_stake ~ [1, 0].
        assert!(has("Active Stake: [1, 0]"));
        assert!(has("Normalized Combined Emission: [1, 0]"));
    });
}

#[test]
fn epoch_topk_validator_permits() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //          uid  hotkey  vali   stake          reg  upd
                Neuron::new(  0,     11, true,  2_000_000_000,  0,   1 ),
                Neuron::new(  1,     22, true,  1_000_000_000,  0,   1 ),
            ];
            setup_epoch(neurons.to_vec(), 1);

            // K = 1 (one validator allowed)
            let netuid = NetUid::from(NETUID);
            MaxAllowedValidators::<Test>::insert(netuid, 1u16);

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        assert!(
            has("Normalised Stake: [0.666"),
            "sanity: asymmetric stake normalized"
        );
        assert!(has("max_allowed_validators: 1"));
        assert!(has("new_validator_permits: [true, false]"));
    });
}

#[test]
fn epoch_yuma3_bonds_pipeline() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                Neuron::new(0, 11, true, 1_000_000_000, 0, 1),
                Neuron::new(1, 22, true, 1_000_000_000, 0, 1),
            ];
            setup_epoch(neurons.to_vec(), 1);

            let netuid = NetUid::from(NETUID);
            Yuma3On::<Test>::insert(netuid, true);

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        // These appear only in the Yuma3 branch:
        assert!(has("Bonds: "));
        assert!(has("emaB: ["));
        assert!(has("emaB norm: "));
        assert!(has("total_bonds_per_validator: "));
    });
}

#[test]
fn epoch_original_yuma_bonds_pipeline() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                Neuron::new(0, 11, true, 1_000_000_000, 0, 1),
                Neuron::new(1, 22, true, 1_000_000_000, 0, 1),
            ];
            setup_epoch(neurons.to_vec(), 1);

            let netuid = NetUid::from(NETUID);
            Yuma3On::<Test>::insert(netuid, false);

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);
        // These strings are present in the non-Yuma3 branch:
        assert!(has("B (outdatedmask): "));
        assert!(has("ΔB (norm): "));
        assert!(has("Exponential Moving Average Bonds: "));
    });
}

#[test]
fn test_validators_weight_two_distinct_servers() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                //           uid  hotkey  vali    stake          reg  upd
                Neuron::new(  0,     11,  true,  1_000_000_000,  0,   1 ), // validator
                Neuron::new(  1,     22,  true,  1_000_000_000,  0,   1 ), // validator
                Neuron::new(  2,     33,  true,  1_000_000_000,  0,   1 ), // validator
                Neuron::new(  3,     44, false,  0,              0,   1 ), // server
                Neuron::new(  4,     55, false,  0,              0,   1 ), // server
            ];
            setup_epoch(neurons.to_vec(), 1);

            let netuid = NetUid::from(NETUID);

            // rows are per-validator; columns correspond to server UIDs [3, 4]
            // V0 -> [MAX, 0] (server 3)
            // V1 -> [0, MAX] (server 4)
            // V2 -> [MAX, 0] (server 3)
            CommitRevealWeightsEnabled::<Test>::insert(netuid, false);
            set_weights(
                netuid,
                vec![vec![u16::MAX, 0], vec![0, u16::MAX], vec![u16::MAX, 0]],
                vec![3, 4],
            );

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);

        // topology sanity
        assert!(has("Number of Neurons in Network: 5"));
        assert!(has("validator_permits: [true, true, true, false, false]"));

        // weight pipeline exercised
        assert!(has("Weights: [[(3, 65535), (4, 0)], [(3, 0), (4, 65535)], [(3, 65535), (4, 0)], [], []]"));
        assert!(has("Weights (permit): [[(3, 65535), (4, 0)], [(3, 0), (4, 65535)], [(3, 65535), (4, 0)], [], []]"));
        assert!(has("Weights (permit+diag): [[(3, 65535), (4, 0)], [(3, 0), (4, 65535)], [(3, 65535), (4, 0)], [], []]"));
        assert!(has("Weights (mask+norm): [[(3, 1), (4, 0)], [(3, 0), (4, 1)], [(3, 1), (4, 0)], [], []]"));

        // downstream signals present
        assert!(has("Consensus: [0, 0, 0, 1, 0]"));
        assert!(has("Validator Trust: [1, 0, 1, 0, 0]"));
        assert!(has("Dividends: [0.5, 0, 0.5, 0, 0]"));
        assert!(has("Normalized Combined Emission: [0.25, 0, 0.25, 0.5, 0]"));
        assert!(!has("math error:"));
    });
}

#[test]
fn test_validator_splits_weight_across_two_servers() {
    new_test_ext(1).execute_with(|| {
        let logs = with_log_capture("trace", || {
            #[rustfmt::skip]
            let neurons = [
                Neuron::new(0, 11, true,  1_000_000_000, 0, 1),
                Neuron::new(1, 22, true,  1_000_000_000, 0, 1),
                Neuron::new(2, 33, true,  1_000_000_000, 0, 1),
                Neuron::new(3, 44, false, 0,             0, 1),
                Neuron::new(4, 55, false, 0,             0, 1),
            ];
            setup_epoch(neurons.to_vec(), 1);

            let netuid = NetUid::from(NETUID);

            // V2 splits: both entries nonzero; row normalization should make ~[0.5, 0.5] for V2
            CommitRevealWeightsEnabled::<Test>::insert(netuid, false);
            set_weights(
                netuid,
                vec![vec![u16::MAX, 0], vec![0, u16::MAX], vec![u16::MAX, u16::MAX]],
                vec![3, 4],
            );

            let emission = AlphaBalance::from(1_000_000_000);
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), emission);
        });

        let has = |s: &str| logs.contains(s);

        assert!(has("validator_permits: [true, true, true, false, false]"));
        assert!(has("Weights (mask+norm): [[(3, 1), (4, 0)], [(3, 0), (4, 1)], [(3, 0.5), (4, 0.5)], [], []]"));
        assert!(has("ΔB (norm): [[(3, 0.5), (4, 0)], [(3, 0), (4, 0.5)], [(3, 0.5), (4, 0.5)], [], []]"));
        assert!(has("Dividends: [0.25, 0.25, 0.5, 0, 0]"));
        assert!(has("Normalized Combined Emission: [0.125, 0.125, 0.25, 0.25, 0.25]"));
        assert!(!has("math error:"));
    });
}

#[test]
fn epoch_mechanism_reads_weights_per_mechanism() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(NETUID);

        // 3 validators (0,1,2) and 2 servers (3,4)
        #[rustfmt::skip]
        let neurons = [
            Neuron::new(0, 11, true,  1_000_000_000, 0, 1),
            Neuron::new(1, 22, true,  1_000_000_000, 0, 1),
            Neuron::new(2, 33, true,  1_000_000_000, 0, 1),
            Neuron::new(3, 44, false, 0,             0, 1),
            Neuron::new(4, 55, false, 0,             0, 1),
        ];
        setup_epoch(neurons.to_vec(), 2); // 2 mechanisms

        // Mech 0: V0,V2 -> server 3 ; V1 -> server 4
        set_weights_for_mech(
            netuid,
            MechId::from(0),
            vec![
                (0, vec![(3, u16::MAX)]),
                (1, vec![(4, u16::MAX)]),
                (2, vec![(3, u16::MAX)]),
            ],
        );
        let logs_m0 = with_log_capture("trace", || {
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), AlphaBalance::from(1_000));
        });

        // Mech 1: flipped routing: V0,V2 -> server 4 ; V1 -> server 3
        set_weights_for_mech(
            netuid,
            MechId::from(1),
            vec![
                (0, vec![(4, u16::MAX)]),
                (1, vec![(3, u16::MAX)]),
                (2, vec![(4, u16::MAX)]),
            ],
        );
        let logs_m1 = with_log_capture("trace", || {
            SubtensorModule::epoch_mechanism(netuid, MechId::from(1), AlphaBalance::from(1_000));
        });

        // Both should run the full pipeline…
        assert!(logs_m0.contains("Active Stake: [0.3333333333, 0.3333333333, 0.3333333333, 0, 0]"));
        assert!(logs_m1.contains("Active Stake: [0.3333333333, 0.3333333333, 0.3333333333, 0, 0]"));
        assert!(logs_m0.contains("Weights (mask+norm): [[(3, 1)], [(4, 1)], [(3, 1)], [], []]"));
        assert!(logs_m1.contains("Weights (mask+norm): [[(4, 1)], [(3, 1)], [(4, 1)], [], []]"));
        assert!(logs_m0.contains("ΔB (norm): [[(3, 0.5)], [], [(3, 0.5)], [], []]"));
        assert!(logs_m1.contains("ΔB (norm): [[(4, 0.5)], [], [(4, 0.5)], [], []]"));
        assert!(logs_m0.contains("Normalized Combined Emission: [0.25, 0, 0.25, 0.5, 0]"));
        assert!(logs_m1.contains("Normalized Combined Emission: [0.25, 0, 0.25, 0, 0.5]"));

        // ...and produce different logs because weights differ per mechanism.
        assert_ne!(
            logs_m0, logs_m1,
            "mechanism-specific weights should yield different outcomes/logs"
        );
    });
}

// cargo test --package pallet-subtensor --lib -- tests::epoch_logs::epoch_mechanism_three_mechanisms_separate_state --exact --show-output
#[test]
fn epoch_mechanism_three_mechanisms_separate_state() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(NETUID);

        // 2 validators, 2 servers
        #[rustfmt::skip]
        let neurons = [
            Neuron::new(0, 11, true,  1_000_000_000, 0, 1),
            Neuron::new(1, 22, true,  1_000_000_000, 0, 1),
            Neuron::new(2, 44, false, 0,             0, 1),
            Neuron::new(3, 55, false, 0,             0, 1),
        ];
        setup_epoch(neurons.to_vec(), 3); // 3 mechanisms

        // Mech 0: all validators -> server 2
        set_weights_for_mech(
            netuid,
            MechId::from(0),
            vec![(0, vec![(2, u16::MAX)]), (1, vec![(2, u16::MAX)])],
        );

        // Mech 1: split across both servers (two nonzero entries per row)
        set_weights_for_mech(
            netuid,
            MechId::from(1),
            vec![
                (0, vec![(2, u16::MAX), (3, u16::MAX)]),
                (1, vec![(2, u16::MAX), (3, u16::MAX)]),
            ],
        );

        // Mech 2: all validators -> server 3
        set_weights_for_mech(
            netuid,
            MechId::from(2),
            vec![(0, vec![(3, u16::MAX)]), (1, vec![(3, u16::MAX)])],
        );

        let l0 = with_log_capture("trace", || {
            SubtensorModule::epoch_mechanism(netuid, MechId::from(0), AlphaBalance::from(1_000));
        });
        let l1 = with_log_capture("trace", || {
            SubtensorModule::epoch_mechanism(netuid, MechId::from(1), AlphaBalance::from(1_000));
        });
        let l2 = with_log_capture("trace", || {
            SubtensorModule::epoch_mechanism(netuid, MechId::from(2), AlphaBalance::from(1_000));
        });

        // Check major epoch indicators
        assert!(l0.contains("Weights (mask+norm): [[(2, 1)], [(2, 1)], [], []]"));
        assert!(l0.contains("ΔB (norm): [[(2, 0.5)], [(2, 0.5)], [], []]"));
        assert!(l0.contains("Normalized Combined Emission: [0.25, 0.25, 0.5, 0]"));

        assert!(
            l1.contains(
                "Weights (mask+norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]"
            )
        );
        assert!(l1.contains("ΔB (norm): [[(2, 0.5), (3, 0.5)], [(2, 0.5), (3, 0.5)], [], []]"));
        assert!(l1.contains("Normalized Combined Emission: [0.25, 0.25, 0.25, 0.25]"));

        assert!(l2.contains("Weights (mask+norm): [[(3, 1)], [(3, 1)], [], []]"));
        assert!(l2.contains("ΔB (norm): [[(3, 0.5)], [(3, 0.5)], [], []]"));
        assert!(l2.contains("Normalized Combined Emission: [0.25, 0.25, 0, 0.5]"));

        // Distinct outcomes
        assert_ne!(l0, l1);
        assert_ne!(l1, l2);
        assert_ne!(l0, l2);
    });
}
