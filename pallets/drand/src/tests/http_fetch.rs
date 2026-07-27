use super::*;

#[test]
fn can_execute_and_handle_valid_http_responses() {
    use serde_json;

    let expected_pulse: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();

    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();

        for endpoint in ENDPOINTS.iter() {
            state.expect_request(PendingRequest {
                method: "GET".into(),
                uri: format!("{endpoint}/{QUICKNET_CHAIN_HASH}/public/1000"),
                response: Some(DRAND_PULSE.as_bytes().to_vec()),
                sent: true,
                ..Default::default()
            });
        }

        for endpoint in ENDPOINTS.iter() {
            state.expect_request(PendingRequest {
                method: "GET".into(),
                uri: format!("{endpoint}/{QUICKNET_CHAIN_HASH}/public/latest"),
                response: Some(DRAND_PULSE.as_bytes().to_vec()),
                sent: true,
                ..Default::default()
            });
        }
    }

    t.execute_with(|| {
        let actual_specific = Drand::fetch_drand_by_round(1000u64).unwrap();
        assert_eq!(actual_specific, expected_pulse);

        let actual_pulse = Drand::fetch_drand_latest().unwrap();
        assert_eq!(actual_pulse, expected_pulse);
    });
}

#[test]
fn test_all_endpoints_fail() {
    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();
        let endpoints = ENDPOINTS;

        for endpoint in endpoints.iter() {
            state.expect_request(PendingRequest {
                method: "GET".into(),
                uri: format!("{endpoint}/{QUICKNET_CHAIN_HASH}/public/1000"),
                response: Some(INVALID_JSON.as_bytes().to_vec()),
                sent: true,
                ..Default::default()
            });
        }
    }

    t.execute_with(|| {
        let result = Drand::fetch_drand_by_round(1000u64);
        assert!(
            result.is_err(),
            "All endpoints should fail due to invalid JSON responses"
        );
    });
}

#[test]
fn test_eventual_success() {
    let expected_pulse: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();

    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();
        let endpoints = ENDPOINTS;

        // We'll make all endpoints except the last return invalid JSON.
        // Since no meta is provided, these are "200 OK" but invalid JSON, causing decode failures.
        // The last endpoint returns the valid DRAND_PULSE JSON, leading to success.

        // Endpoint 0: Invalid JSON (decode fail)
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[0], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 1: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[1], QUICKNET_CHAIN_HASH),
            response: Some(Vec::new()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 2: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[2], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 3: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[3], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 4: Valid JSON (success)
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[4], QUICKNET_CHAIN_HASH),
            response: Some(DRAND_PULSE.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });
    }

    t.execute_with(|| {
        let actual = Drand::fetch_drand_by_round(1000u64).unwrap();
        assert_eq!(
            actual, expected_pulse,
            "Should succeed on the last endpoint after failing at the previous ones"
        );
    });
}

#[test]
fn test_invalid_json_then_success() {
    let expected_pulse: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();

    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();

        let endpoints = ENDPOINTS;

        // Endpoint 1: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[0], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 2: Valid response
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[1], QUICKNET_CHAIN_HASH),
            response: Some(DRAND_PULSE.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });
    }

    t.execute_with(|| {
        let actual = Drand::fetch_drand_by_round(1000u64).unwrap();
        assert_eq!(actual, expected_pulse);
    });
}
