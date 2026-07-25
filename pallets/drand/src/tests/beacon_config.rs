use super::*;

#[test]
fn it_blocks_non_root_from_submit_beacon_info() {
    new_test_ext().execute_with(|| {
        let block_number = 100_000_000;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        // Prepare the beacon configuration payload
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.try_into_beacon_config().unwrap(),
            public: alice.public(),
        };

        // Signature is not required when using Root origin, but we'll include it for completeness
        let signature = None;

        // Attempt to set the beacon config with a non-root origin (signed by Alice)
        // Expect it to fail with BadOrigin
        assert_noop!(
            Drand::set_beacon_config(
                RuntimeOrigin::signed(alice.public()),
                config_payload.clone(),
                signature
            ),
            sp_runtime::DispatchError::BadOrigin
        );

        // Attempt to set the beacon config with an unsigned origin
        // Expect it to fail with BadOrigin
        assert_noop!(
            Drand::set_beacon_config(RuntimeOrigin::none(), config_payload.clone(), signature),
            sp_runtime::DispatchError::BadOrigin
        );

        // Now attempt to set the beacon config with Root origin
        // Expect it to succeed
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        // Verify that the BeaconConfig storage item has been updated
        let stored_config = BeaconConfig::<Test>::get();
        assert_eq!(stored_config, info.try_into_beacon_config().unwrap());
    });
}

#[test]
fn signed_cannot_submit_beacon_info() {
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
        // Dispatch a signed extrinsic
        assert_noop!(
            Drand::set_beacon_config(
                RuntimeOrigin::signed(alice.public()),
                config_payload,
                signature
            ),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}
