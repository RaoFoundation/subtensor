use super::*;

#[test]
fn test_validate_unsigned_write_pulse() {
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        let pulse = Pulse {
            round: 1,
            randomness: frame_support::BoundedVec::truncate_from(vec![0u8; 32]),
            signature: frame_support::BoundedVec::truncate_from(vec![1u8; 96]),
        };

        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![pulse],
            public: alice.public(),
        };
        let signature = alice.sign(&pulses_payload.encode());

        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_ok!(validity);
    });
}

#[test]
fn validate_unsigned_accepts_first_live_round_as_storage_anchor() {
    // On a fresh chain the current drand round is far beyond the normal catch-up
    // window. It must be admitted so `write_pulse` can anchor both round markers.
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        assert_eq!(LastStoredRound::<Test>::get(), 0);
        assert_eq!(OldestStoredRound::<Test>::get(), 0);

        let pulse = Pulse {
            round: crate::MAX_PULSES_TO_FETCH + 1,
            randomness: frame_support::BoundedVec::truncate_from(vec![0u8; 32]),
            signature: frame_support::BoundedVec::truncate_from(vec![1u8; 96]),
        };
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![pulse],
            public: alice.public(),
        };
        let signature = alice.sign(&pulses_payload.encode());
        let call = Call::write_pulse {
            pulses_payload,
            signature: Some(signature),
        };

        assert_ok!(Drand::validate_unsigned(TransactionSource::Local, &call));
    });
}

#[test]
fn validate_unsigned_rejects_round_too_far_ahead() {
    // A round that would leap LastStoredRound by more than the offchain worker ever
    // submits in one run is not a legitimate catch-up pulse. Drop it at the mempool
    // before it can reach dispatch (#2794).
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        LastStoredRound::<Test>::put(100);

        // 151 == last(100) + MAX_PULSES_TO_FETCH(50) + 1, i.e. one beyond the cap.
        let pulse = Pulse {
            round: 100 + crate::MAX_PULSES_TO_FETCH + 1,
            randomness: frame_support::BoundedVec::truncate_from(vec![0u8; 32]),
            signature: frame_support::BoundedVec::truncate_from(vec![1u8; 96]),
        };
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![pulse],
            public: alice.public(),
        };
        let signature = alice.sign(&pulses_payload.encode());

        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::Stale);
    });
}

#[test]
fn test_not_validate_unsigned_write_pulse_with_bad_proof() {
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![],
            public: alice.public(),
        };

        // Bad signature
        let signature = <Test as frame_system::offchain::SigningTypes>::Signature::default();
        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::BadProof);
    });
}

#[test]
fn test_not_validate_unsigned_write_pulse_with_no_payload_signature() {
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![],
            public: alice.public(),
        };

        // No signature
        let signature = None;
        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature,
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::BadSigner);
    });
}

#[test]
fn validate_unsigned_rejects_future_block_number() {
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let future_block_number = 100_000_100;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number: future_block_number,
            pulses: vec![],
            public: alice.public(),
        };
        let signature = alice.sign(&pulses_payload.encode());

        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::Future);
    });
}
