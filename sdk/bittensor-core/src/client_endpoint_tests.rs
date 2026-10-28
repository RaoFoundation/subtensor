//! Endpoint identity regression tests exercise the real HTTP failover path.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn server(replies: Vec<(&'static str, Option<JsonValue>)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let worker = thread::spawn(move || {
        for (method, result) in replies {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut request = Vec::new();
            let (header_end, length) = loop {
                let mut byte = [0];
                stream.read_exact(&mut byte).expect("HTTP header");
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().expect("content length"))
                        })
                        .expect("content length header");
                    break (request.len(), length);
                }
            };
            request.resize(header_end + length, 0);
            stream
                .read_exact(&mut request[header_end..])
                .expect("JSON body");
            let body: JsonValue =
                serde_json::from_slice(&request[header_end..]).expect("JSON request");
            assert_eq!(
                body["method"], method,
                "unvalidated endpoint received application request"
            );
            if method == "chain_getBlockHash" {
                assert_eq!(body["params"], json!([0]));
            }
            let (status, response) = match result {
                Some(value) => (
                    "200 OK",
                    json!({ "jsonrpc": "2.0", "id": body["id"], "result": value }).to_string(),
                ),
                None => ("503 Service Unavailable", "{}".to_string()),
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).expect("HTTP response");
        }
    });
    (endpoint, worker)
}

fn test_client(endpoints: Vec<String>, validated: HashMap<usize, Instant>) -> Client {
    let fixture: JsonValue =
        serde_json::from_str(include_str!("../../python/tests/fixtures/golden.json"))
            .expect("metadata fixture");
    let encoded = decode_hex(
        fixture["metadata"]["v15_hex"]
            .as_str()
            .expect("V15 metadata"),
    )
    .expect("metadata hex");
    let metadata = Option::<Vec<u8>>::decode(&mut encoded.as_slice())
        .expect("metadata envelope")
        .expect("metadata present");
    let runtime = Runtime::parse(&metadata, 419, 1, 42).expect("runtime metadata");
    Client {
        endpoints,
        active_endpoint: RwLock::new(0),
        validated_endpoints: Mutex::new(validated),
        http: HttpClient::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("HTTP client"),
        next_id: AtomicU64::new(1),
        runtime: RwLock::new(Arc::new(runtime)),
        nonce_reservations: Mutex::new(NonceReservations::default()),
        genesis_hash: [1; 32],
        ss58_format: 42,
    }
}

#[test]
fn submission_response_loss_is_uncertain_and_never_retried() {
    let (primary, worker) = server(vec![("author_submitExtrinsic", None)]);
    let fallback = TcpListener::bind("127.0.0.1:0").unwrap();
    fallback.set_nonblocking(true).unwrap();
    let client = test_client(
        vec![
            primary,
            format!("http://{}", fallback.local_addr().unwrap()),
        ],
        HashMap::from([(0, Instant::now())]),
    );
    let outcome = client
        .submit_encoded_with_wait(
            &[1, 2],
            "0xexpected".into(),
            TxWait::Submitted,
            Duration::from_millis(50),
        )
        .unwrap();
    assert_eq!(outcome.extrinsic_hash, "0xexpected");
    assert_eq!(
        outcome.data.get("submission_uncertain"),
        Some(&Value::Bool(true))
    );
    assert!(!is_definite_rejection(&outcome));
    assert!(
        fallback.accept().is_err(),
        "submission must not reach a fallback"
    );
    worker.join().unwrap();
}

#[test]
fn read_failover_still_validates_chain_identity() {
    let (primary, primary_thread) = server(vec![("system_health", None)]);
    let (wrong, wrong_thread) = server(vec![(
        "chain_getBlockHash",
        Some(json!(hex_prefixed(&[2; 32]))),
    )]);
    let (fallback, fallback_thread) = server(vec![
        ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
        ("system_health", Some(json!({"peers":2}))),
    ]);
    let client = test_client(
        vec![primary, wrong, fallback.clone()],
        HashMap::from([(0, Instant::now())]),
    );
    assert_eq!(
        client.rpc_value("system_health", json!([])).unwrap(),
        json!({"peers":2})
    );
    assert_eq!(client.endpoint(), fallback);
    primary_thread.join().unwrap();
    wrong_thread.join().unwrap();
    fallback_thread.join().unwrap();
}

#[test]
fn only_definite_admission_errors_allow_nonce_reuse() {
    for code in [1010, 1011, 1012, 1014, -32600, -32601, -32602] {
        assert!(is_definite_submit_error(&CoreError::Rpc(format!(
            "author_submitExtrinsic: {{\"code\":{code}}}"
        ))));
    }
    for message in [
        "author_submitExtrinsic request failed: timeout",
        "author_submitExtrinsic: {\"code\":1013}",
        "author_submitExtrinsic: {\"code\":-32603}",
    ] {
        assert!(!is_definite_submit_error(&CoreError::Rpc(message.into())));
    }
    assert!(!is_nonce_pool_rejection(&TxOutcome::uncertain(
        "0xhash".into(),
        "future nonce after lost response".into()
    )));
}

#[test]
fn releasing_unused_ranges_preserves_other_reservations() {
    let mut nonces = NonceReservations::default();
    assert_eq!(nonces.reserve("account".into(), 7, 2).unwrap(), 7);
    nonces.release("account".into(), 7, 2);
    assert_eq!(nonces.reserve("account".into(), 7, 1).unwrap(), 7);
    assert_eq!(nonces.reserve("account".into(), 7, 1).unwrap(), 8);
    nonces.release("account".into(), 7, 1);
    assert_eq!(nonces.floors["account"], 9);
    nonces.release("account".into(), 8, 1);
    assert_eq!(nonces.floors["account"], 7);
}

#[test]
fn expired_identity_is_rechecked_before_submitting() {
    let (endpoint, worker) = server(vec![(
        "chain_getBlockHash",
        Some(json!(hex_prefixed(&[2; 32]))),
    )]);
    let stale = Instant::now()
        .checked_sub(ENDPOINT_IDENTITY_TTL + Duration::from_secs(1))
        .expect("expired timestamp");
    let client = test_client(vec![endpoint], HashMap::from([(0, stale)]));
    let error = client
        .rpc_value("author_submitExtrinsic", json!(["0x0102"]))
        .expect_err("changed chain must be rejected");
    assert!(error.to_string().contains("belongs to another chain"));
    worker.join().expect("server");
}

#[test]
fn earlier_failed_reservations_are_reused_without_discarding_later_plans() {
    let mut state = NonceReservations::default();
    assert_eq!(state.reserve("a".into(), 7, 1).unwrap(), 7);
    assert_eq!(state.reserve("a".into(), 7, 1).unwrap(), 8);
    state.release("a".into(), 7, 1);
    assert!(state.reserve("a".into(), 7, 2).is_err());
    assert_eq!(state.reserve("a".into(), 7, 1).unwrap(), 7);
    assert_eq!(state.reserve("a".into(), 7, 1).unwrap(), 9);
    state.release("a".into(), 8, 1);
    assert_eq!(
        state.reserve("a".into(), 10, 1).unwrap(),
        10,
        "remote progress discards obsolete holes"
    );
}

fn version() -> JsonValue {
    json!({"specVersion":419,"transactionVersion":1,"specName":"node-subtensor"})
}

#[test]
fn ordinary_external_and_local_signing_bind_metadata_by_default() {
    let replies = vec![
        ("system_accountNextIndex", Some(json!(7))),
        ("state_getRuntimeVersion", Some(version())),
        ("chain_getHeader", Some(json!({"number":"0x0"}))),
        ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
        ("state_getRuntimeVersion", Some(version())),
        ("system_properties", Some(json!({}))),
        ("payment_queryInfo", Some(json!({"partialFee":"10"}))),
        ("state_getRuntimeVersion", Some(version())),
        ("state_getRuntimeVersion", Some(version())),
        ("system_properties", Some(json!({}))),
    ];
    let (endpoint, worker) = server(replies);
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let runtime = client.runtime().unwrap();
    let call = runtime
        .compose_call(
            "System",
            "remark",
            &Value::record(vec![("remark".into(), Value::Bytes(vec![1]))]),
        )
        .unwrap();
    let plan = client
        .external_signing_plan(
            &call,
            ExternalSigner {
                ss58_address: key.ss58_address(),
                public_key: key.public_key_bytes(),
                crypto_type: key.crypto_type(),
                requires_metadata_proof: false,
            },
            ExternalSigningOptions {
                period: None,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(plan.params.metadata_hash.is_some());
    let local = client.transaction_params_at(7, None, 10).unwrap();
    assert_eq!(local.metadata_hash, plan.params.metadata_hash);
    let mut unprotected = plan.params.clone();
    unprotected.metadata_hash = None;
    assert_ne!(
        runtime.signature_payload(&call, &unprotected).unwrap(),
        plan.payload
    );
    worker.join().unwrap();
}

#[test]
fn failed_preparation_releases_its_nonce_without_another_rpc() {
    let (endpoint, worker) = server(vec![
        ("system_accountNextIndex", Some(json!(7))),
        ("state_getRuntimeVersion", Some(version())),
        ("chain_getHeader", Some(json!({"number":"bad number"}))),
        ("system_accountNextIndex", Some(json!(7))),
    ]);
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    assert!(client
        .external_signing_plan(
            &[0, 0],
            ExternalSigner {
                ss58_address: key.ss58_address(),
                public_key: key.public_key_bytes(),
                crypto_type: key.crypto_type(),
                requires_metadata_proof: false
            },
            ExternalSigningOptions::default()
        )
        .is_err());
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 7);
    worker.join().unwrap();
}

#[test]
fn semantic_policy_rejection_releases_external_plan_nonce() {
    use crate::transaction::{Executor, IntentCall, Policy};
    let (endpoint, worker) = server(vec![
        ("state_getRuntimeVersion", Some(version())),
        ("system_accountNextIndex", Some(json!(7))),
        ("state_getRuntimeVersion", Some(version())),
        ("chain_getHeader", Some(json!({"number":"0x0"}))),
        ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
        ("payment_queryInfo", Some(json!({"partialFee":"10"}))),
        ("system_accountNextIndex", Some(json!(7))),
    ]);
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let executor = Executor::new(&client);
    let result = executor.external_signing_plan(
        &IntentCall::transfer(key.ss58_address(), 1),
        ExternalSigner {
            ss58_address: key.ss58_address(),
            public_key: key.public_key_bytes(),
            crypto_type: key.crypto_type(),
            requires_metadata_proof: false,
        },
        ExternalSigningOptions {
            period: None,
            metadata_hash: MetadataHashMode::Disabled,
            ..Default::default()
        },
        Some(&Policy {
            max_fee_rao: Some(9),
            ..Default::default()
        }),
    );
    assert!(matches!(result, Err(CoreError::Policy(_))));
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 7);
    worker.join().unwrap();
}

#[test]
fn shield_fee_cap_checks_carrier_before_any_signing() {
    use crate::transaction::{Executor, IntentCall, Policy, Wallet};
    let mut replies = vec![
        ("state_getRuntimeVersion", Some(version())),
        ("system_accountNextIndex", Some(json!(7))),
    ];
    let quote = || {
        vec![
            ("state_getRuntimeVersion", Some(version())),
            ("chain_getHeader", Some(json!({"number":"0x0"}))),
            ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
            ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
            ("state_getRuntimeVersion", Some(version())),
            ("payment_queryInfo", Some(json!({"partialFee":"10"}))),
        ]
    };
    replies.extend(quote());
    replies.push(("state_getRuntimeVersion", Some(version())));
    replies.extend(quote());
    replies.extend(quote());
    let (endpoint, worker) = server(replies);
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    // This fixture isolates the fee guard; metadata binding has its own tests.
    Arc::get_mut(&mut client.runtime.write().unwrap())
        .unwrap()
        .extrinsic
        .signed_extensions
        .retain(|ext| ext.identifier != "CheckMetadataHash");
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let public = Keypair::new(Some(&key.ss58_address()), None, key.crypto_type(), 42).unwrap();
    let wallet = Wallet {
        coldkey: public.clone(),
        hotkey: public,
    };
    let result = Executor::new(&client).submit_shielded(
        &IntentCall::transfer(key.ss58_address(), 1),
        &wallet,
        Some(&Policy {
            max_fee_rao: Some(15),
            ..Default::default()
        }),
    );
    assert!(
        matches!(&result,Err(CoreError::Policy(message)) if message.contains("fee")),
        "{result:?}"
    );
    worker.join().unwrap();
}
