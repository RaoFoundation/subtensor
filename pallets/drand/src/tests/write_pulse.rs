use super::*;

#[test]
fn it_can_submit_valid_pulse_when_beacon_config_exists() {
    new_test_ext().execute_with(|| {
        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();

        let alice = sp_keyring::Sr25519Keyring::Alice;
        let block_number = 100_000_000;
        System::set_block_number(block_number);

        // Set the beacon config
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };

        // The signature doesn't really matter here because the signature is validated in the
        // transaction validation phase not in the dispatchable itself.
        let signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };

        // Dispatch an unsigned extrinsic.
        assert_ok!(Drand::write_pulse(
            RuntimeOrigin::none(),
            pulses_payload,
            signature
        ));

        // Read pallet storage and assert an expected result.
        let pulse = Pulses::<Test>::get(ROUND_NUMBER);
        assert!(pulse.is_some());
        assert_eq!(pulse, Some(p));
    });
}

#[test]
fn it_rejects_invalid_pulse_due_to_bad_signature() {
    new_test_ext().execute_with(|| {
        let alice = sp_keyring::Sr25519Keyring::Alice;
		let block_number = 100_000_000;
        System::set_block_number(block_number);

        // Set the beacon config using Root origin
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        // Signature is not required for Root origin
        let config_signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload.clone(),
            config_signature
        ));

        // Get a bad pulse (invalid signature within the pulse data)
        let bad_http_response = "{\"round\":1000,\"randomness\":\"87f03ef5f62885390defedf60d5b8132b4dc2115b1efc6e99d166a37ab2f3a02\",\"signature\":\"b0a8b04e009cf72534321aca0f50048da596a3feec1172a0244d9a4a623a3123d0402da79854d4c705e94bc73224c341\"}";
        let u_p: DrandResponseBody = serde_json::from_str(bad_http_response).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();

        // Prepare the pulses payload
        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };
        let pulses_signature = alice.sign(&pulses_payload.encode());

        assert_noop!(
            Drand::write_pulse(
                RawOrigin::None.into(),
                pulses_payload.clone(),
                Some(pulses_signature)
            ),
            Error::<Test>::PulseVerificationError
        );

        let pulse = Pulses::<Test>::get(ROUND_NUMBER);
        assert!(pulse.is_none());
    });
}

#[test]
fn it_rejects_pulses_with_non_incremental_round_numbers() {
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        // Set the beacon config
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        // The signature doesn't really matter here because the signature is validated in the
        // transaction validation phase not in the dispatchable itself.
        let signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();
        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };

        // Dispatch an unsigned extrinsic.
        assert_ok!(Drand::write_pulse(
            RuntimeOrigin::none(),
            pulses_payload.clone(),
            signature
        ));
        let pulse = Pulses::<Test>::get(ROUND_NUMBER);
        assert!(pulse.is_some());

        System::set_block_number(2);

        // Attempt to submit the same pulse again, which should fail
        assert_noop!(
            Drand::write_pulse(RuntimeOrigin::none(), pulses_payload, signature),
            Error::<Test>::InvalidRoundNumber,
        );
    });
}

#[test]
fn write_pulse_rejects_round_skip() {
    // A single pulse must not be allowed to leap LastStoredRound past the rounds
    // in between, or those rounds can never be stored and any reveal/timelock that
    // references them is wedged (#2794). Here round 1000 is a valid (BLS-verified)
    // pulse, but LastStoredRound is seeded to 998 so round 1000 is a skip of two.
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        let signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        // Seed an existing baseline so this is not the anchor (first) storage.
        LastStoredRound::<Test>::put(998);
        OldestStoredRound::<Test>::put(998);

        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();
        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };

        // Round 1000 is NOT last(998) + 1, so it must be rejected.
        assert_noop!(
            Drand::write_pulse(RuntimeOrigin::none(), pulses_payload, signature),
            Error::<Test>::InvalidRoundNumber,
        );

        // State is unchanged: no leap, nothing stored.
        assert_eq!(LastStoredRound::<Test>::get(), 998);
        assert!(Pulses::<Test>::get(ROUND_NUMBER).is_none());
    });
}

#[test]
fn write_pulse_accepts_consecutive_round() {
    // The strict-advance rule must still accept the legitimate next round.
    // Round 1000 == last(999) + 1, so it is stored.
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        let signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        LastStoredRound::<Test>::put(999);
        OldestStoredRound::<Test>::put(999);

        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();
        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };

        assert_ok!(Drand::write_pulse(
            RuntimeOrigin::none(),
            pulses_payload,
            signature
        ));
        assert_eq!(LastStoredRound::<Test>::get(), ROUND_NUMBER);
        assert!(Pulses::<Test>::get(ROUND_NUMBER).is_some());
    });
}
