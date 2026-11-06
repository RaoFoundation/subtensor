use bittensor_core::timelock::constants::GENESIS_TIME;
use bittensor_core::{timelock::*, CoreError};
use codec::Decode;

fn state() -> epoch_schedule::EpochScheduleState {
    epoch_schedule::EpochScheduleState {
        last_epoch_block: 10,
        pending_epoch_at: 0,
        subnet_epoch_index: 0,
        tempo: 50,
        blocks_since_last_step: 0,
        current_block: 10,
    }
}

#[test]
fn clock_and_security_offset_pin_round() -> Result<(), CoreError> {
    let now = GENESIS_TIME as f64 + 100.0;
    // First reveal 60; ingestion 63; head 10. 100 + 53 * 12 = 736 seconds.
    assert_eq!(reveal_round_at(now, &state(), 1, 12.0)?, 245);
    assert_eq!(reveal_round_at(0.0, &state(), 1, 12.0)?, 1);
    assert_eq!(reveal_round_at(now + 3.0, &state(), 1, 12.0)?, 246);
    Ok(())
}

#[test]
fn injected_clock_drives_the_real_commit_api() -> Result<(), Box<dyn std::error::Error>> {
    use ark_serialize::CanonicalDeserialize;
    use tle::{curves::drand::TinyBLS381, tlock::TLECiphertext};

    let now = GENESIS_TIME as f64 + 100.0;
    for (time, expected_round) in [(now, 245), (now + 3.0, 246)] {
        let (ciphertext, round) = generate_commit_v2_at(
            time,
            vec![1, 2],
            vec![65535, 1],
            42,
            state(),
            1,
            12.0,
            vec![7; 32],
        )?;
        assert_eq!(round, expected_round);
        TLECiphertext::<TinyBLS381>::deserialize_compressed(ciphertext.as_slice())?;
    }
    assert!(
        generate_commit_v2_at(now, vec![1], vec![], 42, state(), 1, 12.0, vec![7; 32],).is_err()
    );
    Ok(())
}

#[test]
fn payload_uses_upstream_scale_definition() -> Result<(), codec::Error> {
    let encoded = commit_payload(vec![7; 32], vec![1, 2], vec![65535, 1], 42);
    let mut bytes = encoded.as_slice();
    let decoded = WeightsTlockPayload::decode(&mut bytes)?;
    assert!(bytes.is_empty());
    assert_eq!(decoded.hotkey, vec![7; 32]);
    assert_eq!(decoded.uids, vec![1, 2]);
    assert_eq!(decoded.values, vec![65535, 1]);
    assert_eq!(decoded.version_key, 42);
    Ok(())
}

#[test]
fn invalid_clock_and_timestamp_overflow_fail_before_crypto() {
    for (now, block_time) in [
        (f64::NAN, 12.0),
        (f64::INFINITY, 12.0),
        (-1.0, 12.0),
        (1.0, 0.0),
        (1.0, -1.0),
        (1.0, f64::NAN),
        (f64::MAX, f64::MAX),
    ] {
        assert!(reveal_round_at(now, &state(), 1, block_time).is_err());
    }
}
