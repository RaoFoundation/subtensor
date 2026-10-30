//! Endpoint identity regression tests exercise the real HTTP failover path.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn server(replies: Vec<(&'static str, Option<JsonValue>)>) -> (String, thread::JoinHandle<()>) {
    inspected_server(replies, |_| {})
}

fn inspected_server(
    replies: Vec<(&'static str, Option<JsonValue>)>,
    inspect: impl Fn(&JsonValue) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
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
            inspect(&body);
            if method == "chain_getBlockHash" {
                assert_eq!(body["params"], json!([0]));
            }
            let (status, response) = match result {
                Some(value) if value.get("error").is_some() => (
                    "200 OK",
                    json!({ "jsonrpc": "2.0", "id": body["id"], "error": value["error"] })
                        .to_string(),
                ),
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

fn external_plan_replies() -> Vec<(&'static str, Option<JsonValue>)> {
    vec![
        ("system_accountNextIndex", Some(json!(7))),
        ("state_getRuntimeVersion", Some(version())),
        ("chain_getHeader", Some(json!({"number":"0x0"}))),
        ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
        ("payment_queryInfo", Some(json!({"partialFee":"10"}))),
    ]
}

fn external_plan(client: &Client, key: &Keypair) -> ExternalSigningPlan {
    let call = client
        .runtime()
        .unwrap()
        .compose_call(
            "System",
            "remark",
            &Value::record(vec![("remark".into(), Value::Bytes(vec![1]))]),
        )
        .unwrap();
    client
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
                metadata_hash: MetadataHashMode::Disabled,
                ..Default::default()
            },
        )
        .unwrap()
}

#[test]
fn failed_external_plan_and_clones_cannot_reuse_or_release_reassigned_nonce() {
    for cancelled in [false, true] {
        let mut replies = external_plan_replies();
        replies.extend(external_plan_replies());
        replies.push(("system_accountNextIndex", Some(json!(7))));
        let (endpoint, worker) = server(replies);
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
        let failed = external_plan(&client, &key);
        let clone = failed.clone();
        let signature = key.sign(&failed.payload).unwrap();
        let token = AtomicBool::new(cancelled);
        let error = client
            .submit_external_with_cancel(
                &failed,
                if cancelled { &signature } else { &[0; 64] },
                None,
                TxWait::Submitted,
                Duration::from_millis(50),
                Some(&token),
            )
            .unwrap_err();
        assert!(!error.to_string().contains("no longer usable"));
        let replacement = external_plan(&client, &key);
        assert_eq!(replacement.params.nonce, failed.params.nonce);
        for stale in [&failed, &clone] {
            let error = client
                .submit_external(
                    stale,
                    &signature,
                    None,
                    TxWait::Submitted,
                    Duration::from_millis(50),
                )
                .unwrap_err();
            assert!(error.to_string().contains("no longer usable"));
            assert!(client
                .assemble_external_extrinsic(stale, &signature, None)
                .is_err());
            client.release_external_plan(stale).unwrap();
        }
        let replacement_signature = key.sign(&replacement.payload).unwrap();
        assert!(client
            .assemble_external_extrinsic(&replacement, &replacement_signature, None)
            .is_ok());
        assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
        worker.join().unwrap();
    }
}

#[test]
fn explicit_nonce_release_and_cache_reset_invalidate_detached_plans() {
    for reset in [false, true] {
        let mut replies = external_plan_replies();
        if reset {
            replies.push(("system_accountNextIndex", Some(json!(7))));
        }
        replies.extend(external_plan_replies());
        replies.push(("system_accountNextIndex", Some(json!(7))));
        let (endpoint, worker) = server(replies);
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
        let stale = external_plan(&client, &key);
        let clone = stale.clone();
        if reset {
            client.invalidate_nonce(&key.ss58_address()).unwrap();
        } else {
            client
                .release_nonces(&key.ss58_address(), stale.params.nonce, 1)
                .unwrap();
        }
        let replacement = external_plan(&client, &key);
        assert_eq!(replacement.params.nonce, 7);
        let signature = key.sign(&stale.payload).unwrap();
        assert!(client
            .submit_external(
                &clone,
                &signature,
                None,
                TxWait::Submitted,
                Duration::from_millis(50)
            )
            .is_err());
        assert!(client
            .assemble_external_extrinsic(&stale, &signature, None)
            .is_err());
        client.release_external_plan(&stale).unwrap();
        assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
        worker.join().unwrap();
    }
}

#[test]
fn public_plan_fields_cannot_transfer_or_discard_reservation_ownership() {
    let mut replies = external_plan_replies();
    replies.extend(external_plan_replies());
    replies.push(("system_accountNextIndex", Some(json!(7))));
    let (endpoint, worker) = server(replies);
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let mut plan = external_plan(&client, &key);
    let signature = key.sign(&plan.payload).unwrap();
    let mut changed_nonce = plan.clone();
    changed_nonce.params.nonce += 1;
    let mut changed_signer = plan.clone();
    changed_signer.signer_address = Keypair::from_uri("//Bob", crate::keys::CRYPTO_SR25519)
        .unwrap()
        .ss58_address();
    for altered in [&changed_nonce, &changed_signer] {
        let error = client
            .submit_external(
                altered,
                &signature,
                None,
                TxWait::Submitted,
                Duration::from_millis(50),
            )
            .unwrap_err();
        assert!(error.to_string().contains("reservation was modified"));
    }
    assert!(client
        .assemble_external_extrinsic(&plan, &signature, None)
        .is_ok());
    // This legacy public field is descriptive, never the authority for cleanup.
    plan.reserved_nonce = false;
    assert!(client
        .submit_external(
            &plan,
            &[0; 64],
            None,
            TxWait::Submitted,
            Duration::from_millis(50)
        )
        .is_err());
    let replacement = external_plan(&client, &key);
    assert_eq!(replacement.params.nonce, 7);
    client.release_external_plan(&changed_nonce).unwrap();
    client.release_external_plan(&changed_signer).unwrap();
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
    worker.join().unwrap();
}

#[test]
fn external_plan_rejection_releases_nonce_but_uncertain_delivery_retains_it() {
    for rejected in [false, true] {
        let mut replies = external_plan_replies();
        replies.push((
            "author_submitExtrinsic",
            rejected.then(|| json!({"error":{"code":1010,"message":"Invalid Transaction"}})),
        ));
        replies.extend(external_plan_replies());
        replies.push(("system_accountNextIndex", Some(json!(7))));
        let (endpoint, worker) = server(replies);
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
        let plan = external_plan(&client, &key);
        let clone = plan.clone();
        let signature = key.sign(&plan.payload).unwrap();
        let outcome = client
            .submit_external(
                &plan,
                &signature,
                None,
                TxWait::Submitted,
                Duration::from_millis(50),
            )
            .unwrap();
        assert_eq!(is_definite_rejection(&outcome), rejected);
        let next = external_plan(&client, &key);
        assert_eq!(next.params.nonce, if rejected { 7 } else { 8 });
        assert!(client
            .submit_external(
                &clone,
                &signature,
                None,
                TxWait::Submitted,
                Duration::from_millis(50)
            )
            .is_err());
        assert!(client
            .assemble_external_extrinsic(&clone, &signature, None)
            .is_err());
        assert_eq!(
            client.reserve_nonce(&key.ss58_address()).unwrap(),
            next.params.nonce + 1
        );
        worker.join().unwrap();
    }
}

#[test]
fn concurrent_external_plan_clones_submit_only_once() {
    let mut replies = external_plan_replies();
    replies.push((
        "author_submitExtrinsic",
        Some(json!(hex_prefixed(&[3; 32]))),
    ));
    replies.push(("system_accountNextIndex", Some(json!(7))));
    let (endpoint, worker) = server(replies);
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let plan = external_plan(&client, &key);
    let clone = plan.clone();
    let signature = key.sign(&plan.payload).unwrap();
    let barrier = std::sync::Barrier::new(2);
    let submit = |plan: &ExternalSigningPlan| {
        barrier.wait();
        client.submit_external(
            plan,
            &signature,
            None,
            TxWait::Submitted,
            Duration::from_millis(50),
        )
    };
    thread::scope(|scope| {
        let first = scope.spawn(|| submit(&plan));
        let second = scope.spawn(|| submit(&clone));
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results.iter().any(|result| result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("no longer usable"))));
    });
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
    worker.join().unwrap();
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

#[test]
fn explicit_block_reads_resolve_historical_and_upgraded_metadata() {
    let fixture: JsonValue =
        serde_json::from_str(include_str!("../../python/tests/fixtures/golden.json")).unwrap();
    let template = test_client(vec![], HashMap::new());
    let runtime = template.runtime().unwrap();
    let info = storage_info(&runtime, "SubtensorModule", "NetworksAdded").unwrap();
    let key = hex_prefixed(&runtime.storage_key(&info, &[Value::Uint(7)]).unwrap());
    // Exercise both historical reads and a new head on a client whose live
    // metadata cache still holds the previous runtime.
    for version in [418, 420] {
        let hash = format!("0x{version:064x}");
        let mut replies = Vec::new();
        for operation in 0..4 {
            replies.push((
                "state_getRuntimeVersion",
                Some(json!({"specVersion":version,"transactionVersion":2})),
            ));
            replies.push(("state_call", Some(fixture["metadata"]["v15_hex"].clone())));
            match operation {
                0 => replies.push(("state_getStorage", Some(json!("0x01")))),
                1 => replies.push((
                    "state_queryStorageAt",
                    Some(json!([{"changes":[[key,"0x01"]]}])),
                )),
                2 => {
                    replies.push(("state_getKeysPaged", Some(json!([key]))));
                    replies.push((
                        "state_queryStorageAt",
                        Some(json!([{"changes":[[key,"0x01"]]}])),
                    ));
                }
                _ => replies.push(("state_call", Some(json!("0x0c000000")))),
            }
        }
        let expected_hash = hash.clone();
        let (endpoint, worker) = inspected_server(replies, move |request| {
            assert_eq!(
                request["params"].as_array().unwrap().last().unwrap(),
                &json!(expected_hash)
            );
        });
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        {
            let mut guard = client.runtime.write().unwrap();
            let cached = Arc::get_mut(&mut guard).unwrap();
            // Deliberately incompatible cached layouts must never be used.
            for pallet in &mut cached.pallets {
                pallet.storage.clear();
            }
            cached.apis.clear();
        }
        assert_eq!(
            client
                .query(
                    "SubtensorModule",
                    "NetworksAdded",
                    &[Value::Uint(7)],
                    Some(&hash)
                )
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            client
                .query_batch(
                    "SubtensorModule",
                    "NetworksAdded",
                    &[vec![Value::Uint(7)]],
                    Some(&hash)
                )
                .unwrap(),
            vec![Value::Bool(true)]
        );
        assert_eq!(
            client
                .query_map("SubtensorModule", "NetworksAdded", &[], Some(&hash))
                .unwrap(),
            vec![(Value::Int(7), Value::Bool(true))]
        );
        assert_eq!(
            client
                .runtime_call(
                    "AccountNonceApi",
                    "account_nonce",
                    &[Value::Bytes(vec![0; 32])],
                    Some(&hash)
                )
                .unwrap(),
            Value::Int(12)
        );
        assert_eq!(
            client.runtime().unwrap().spec_version,
            419,
            "historical reads must not replace the live cache"
        );
        worker.join().unwrap();
    }
}

#[test]
fn included_nonces_allow_reaped_accounts_without_erasing_later_reservations() {
    let mut state = NonceReservations::default();
    let account = "a".to_string();
    assert_eq!(state.reserve(account.clone(), 7, 2).unwrap(), 7);
    let carrier_token = state.tokens[&account].clone();
    state.included(&account, 7, &carrier_token);
    assert_eq!(
        state.floors[&account], 9,
        "carrier must preserve inner nonce"
    );
    state.included(&account, 8, &carrier_token);
    assert_eq!(
        state.reserve(account.clone(), 0, 1).unwrap(),
        0,
        "refunded account restarts at zero"
    );
    state.included(&account, 8, &carrier_token);
    assert_eq!(
        state.floors[&account], 1,
        "late receipt cannot clear new epoch"
    );
    let token = state.tokens[&account].clone();
    assert_eq!(state.reserve(account.clone(), 0, 1).unwrap(), 1);
    state.included(&account, 0, &token);
    assert_eq!(
        state.floors[&account], 2,
        "later reservation stays protected"
    );
}

#[test]
fn encoded_receipt_tracking_retires_only_included_authorizations() {
    let (endpoint, worker) = server(external_plan_replies());
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let plan = external_plan(&client, &key);
    let signature = key.sign(&plan.payload).unwrap();
    let (encoded, hash) = client
        .assemble_external_extrinsic(&plan, &signature, None)
        .unwrap();
    let observation = client
        .nonce_observation(&encoded)
        .expect("signed account and nonce");
    assert_eq!(
        observation.0,
        nonce_account_key(&key.ss58_address()).unwrap()
    );
    assert_eq!(observation.1, 7);
    let mut outcome = TxOutcome::uncertain(hash, "lost response".into());
    client.retire_included_nonce(Some(observation.clone()), &outcome);
    assert_eq!(
        client.nonce_reservations.lock().unwrap().floors[&observation.0],
        8
    );
    // Dispatch failure still consumes the nonce; success is not the criterion.
    outcome.block_hash = Some(hex_prefixed(&[2; 32]));
    client.retire_included_nonce(Some(observation.clone()), &outcome);
    assert!(!client
        .nonce_reservations
        .lock()
        .unwrap()
        .floors
        .contains_key(&observation.0));
    assert!(client
        .assemble_external_extrinsic(&plan, &signature, None)
        .is_err());
    worker.join().unwrap();
}
