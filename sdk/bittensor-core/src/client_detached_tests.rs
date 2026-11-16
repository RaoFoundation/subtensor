use super::*;

fn explicit_plan(client: &Client, key: &Keypair, call: &[u8]) -> ExternalSigningPlan {
    client
        .external_signing_plan(
            call,
            ExternalSigner {
                ss58_address: key.ss58_address(),
                public_key: key.public_key_bytes(),
                crypto_type: key.crypto_type(),
                requires_metadata_proof: false,
            },
            ExternalSigningOptions {
                nonce: Some(9),
                period: None,
                metadata_hash: MetadataHashMode::Disabled,
                ..Default::default()
            },
        )
        .unwrap()
}

// PROTECTED_REGRESSION: abandoning a detached plan must restore automatic submissions without letting old clones reuse its nonce; changes require explicit human approval.
#[test]
fn cancellation_fills_a_gap_without_releasing_later_or_reassigned_plans() {
    let mut replies = external_plan_replies();
    replies.extend(external_plan_replies());
    replies.extend(external_plan_replies());
    replies.push(("system_accountNextIndex", Some(json!(7))));
    let (endpoint, worker) = server(replies);
    let client = test_client(vec![endpoint.clone()], HashMap::from([(0, Instant::now())]));
    let other = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let abandoned = external_plan(&client, &key);
    let clone = abandoned.clone();
    let later = external_plan(&client, &key);
    assert_eq!(abandoned.params.nonce, 7);
    assert_eq!(later.params.nonce, 8);
    assert!(other.cancel_external_signing_plan(&abandoned).is_err());
    client.cancel_external_signing_plan(&abandoned).unwrap();
    let replacement = external_plan(&client, &key);
    assert_eq!(replacement.params.nonce, 7);
    let signature = key.sign(&abandoned.payload).unwrap();
    for old in [&abandoned, &clone] {
        assert!(client.cancel_external_signing_plan(old).is_err());
        assert!(client
            .assemble_external_extrinsic(old, &signature, None)
            .is_err());
        assert!(client
            .submit_external(
                old,
                &signature,
                None,
                TxWait::Submitted,
                Duration::from_secs(1)
            )
            .is_err());
    }
    for active in [&replacement, &later] {
        assert!(client
            .assemble_external_extrinsic(active, &key.sign(&active.payload).unwrap(), None)
            .is_ok());
    }
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 9);
    worker.join().unwrap();
}

// PROTECTED_REGRESSION: cancelling an explicit-nonce plan must not release a different automatic reservation at the same nonce; changes require explicit human approval.
#[test]
fn explicit_nonce_cancellation_is_owned_and_does_not_release_another_plan() {
    let mut replies = external_plan_replies();
    replies.extend(external_plan_replies().into_iter().skip(1));
    replies.push(("system_accountNextIndex", Some(json!(7))));
    let (endpoint, worker) = server(replies);
    let client = test_client(vec![endpoint.clone()], HashMap::from([(0, Instant::now())]));
    let other = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let automatic = external_plan(&client, &key);
    let explicit = client
        .external_signing_plan(
            &automatic.call_data,
            ExternalSigner {
                ss58_address: key.ss58_address(),
                public_key: key.public_key_bytes(),
                crypto_type: key.crypto_type(),
                requires_metadata_proof: false,
            },
            ExternalSigningOptions {
                nonce: Some(7),
                period: None,
                metadata_hash: MetadataHashMode::Disabled,
                ..Default::default()
            },
        )
        .unwrap();
    let clone = explicit.clone();
    assert!(other.cancel_external_signing_plan(&explicit).is_err());
    client.cancel_external_signing_plan(&explicit).unwrap();
    assert!(client
        .assemble_external_extrinsic(&clone, &key.sign(&clone.payload).unwrap(), None)
        .is_err());
    assert!(client
        .assemble_external_extrinsic(&automatic, &key.sign(&automatic.payload).unwrap(), None)
        .is_ok());
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
    worker.join().unwrap();
}

// PROTECTED_REGRESSION: a submitted or uncertain detached transaction must retain its nonce rather than enabling conflicting automatic submissions; changes require explicit human approval.
#[test]
fn cancellation_rejects_submitted_and_uncertain_plans() {
    for uncertain in [false, true] {
        let mut replies = external_plan_replies();
        replies.push((
            "author_submitExtrinsic",
            (!uncertain).then(|| json!(hex_prefixed(&[3; 32]))),
        ));
        replies.push(("system_accountNextIndex", Some(json!(7))));
        let (endpoint, worker) = server(replies);
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
        let plan = external_plan(&client, &key);
        let outcome = client
            .submit_external(
                &plan,
                &key.sign(&plan.payload).unwrap(),
                None,
                TxWait::Submitted,
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(!is_definite_rejection(&outcome));
        assert!(client.cancel_external_signing_plan(&plan).is_err());
        assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
        worker.join().unwrap();
    }
}

// PROTECTED_REGRESSION: cancellation racing a broadcast must not release the in-flight transaction's nonce; changes require explicit human approval.
#[test]
fn cancellation_cannot_race_an_in_flight_submission() {
    let mut replies = external_plan_replies();
    replies.push((
        "author_submitExtrinsic",
        Some(json!(hex_prefixed(&[3; 32]))),
    ));
    replies.push(("system_accountNextIndex", Some(json!(7))));
    let entered = Arc::new(std::sync::Barrier::new(2));
    let finish = Arc::new(std::sync::Barrier::new(2));
    let seen = Arc::clone(&entered);
    let release = Arc::clone(&finish);
    let (endpoint, worker) = inspected_server(replies, move |request| {
        if request.get("method").and_then(JsonValue::as_str) == Some("author_submitExtrinsic") {
            seen.wait();
            release.wait();
        }
    });
    let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let plan = external_plan(&client, &key);
    thread::scope(|scope| {
        let submitting = scope.spawn(|| {
            client.submit_external(
                &plan,
                &key.sign(&plan.payload).unwrap(),
                None,
                TxWait::Submitted,
                Duration::from_secs(1),
            )
        });
        entered.wait();
        let cancelled = client.cancel_external_signing_plan(&plan);
        finish.wait();
        assert!(cancelled.is_err());
        submitting.join().unwrap().unwrap();
    });
    assert_eq!(client.reserve_nonce(&key.ss58_address()).unwrap(), 8);
    worker.join().unwrap();
}

// PROTECTED_REGRESSION: adding cancellation must preserve cross-client explicit-nonce signing while preventing foreign cancellation; changes require explicit human approval.
#[test]
fn explicit_plans_can_still_assemble_and_submit_through_another_client() {
    let mut replies = external_plan_replies();
    replies.extend(external_plan_replies().into_iter().skip(1));
    replies.push((
        "author_submitExtrinsic",
        Some(json!(hex_prefixed(&[3; 32]))),
    ));
    replies.push(("system_accountNextIndex", Some(json!(7))));
    let (endpoint, worker) = server(replies);
    let owner = test_client(vec![endpoint.clone()], HashMap::from([(0, Instant::now())]));
    let other = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let automatic = external_plan(&owner, &key);
    let explicit = explicit_plan(&owner, &key, &automatic.call_data);
    let signature = key.sign(&explicit.payload).unwrap();
    assert!(other.cancel_external_signing_plan(&explicit).is_err());
    assert!(other
        .assemble_external_extrinsic(&explicit, &signature, None)
        .is_ok());
    assert!(other
        .submit_external(
            &explicit,
            &signature,
            None,
            TxWait::Submitted,
            Duration::from_secs(1)
        )
        .is_ok());
    assert!(owner.cancel_external_signing_plan(&explicit).is_err());
    assert!(owner
        .assemble_external_extrinsic(&automatic, &key.sign(&automatic.payload).unwrap(), None)
        .is_ok());
    assert_eq!(owner.reserve_nonce(&key.ss58_address()).unwrap(), 8);
    worker.join().unwrap();
}

// PROTECTED_REGRESSION: an explicit plan shared across clients must have only one winner between cancellation and submission; changes require explicit human approval.
#[test]
fn explicit_cross_client_submission_and_cancellation_have_one_winner() {
    let mut replies = external_plan_replies();
    replies.extend(external_plan_replies().into_iter().skip(1));
    let (endpoint, worker) = server(replies);
    let owner = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
    // No endpoint deliberately makes an admitted submission uncertain without
    // depending on which competitor wins or requiring a mock-server request.
    let other = test_client(vec![], HashMap::new());
    let key = Keypair::from_uri("//Alice", crate::keys::CRYPTO_SR25519).unwrap();
    let automatic = external_plan(&owner, &key);
    let explicit = explicit_plan(&owner, &key, &automatic.call_data);
    let signature = key.sign(&explicit.payload).unwrap();
    let start = std::sync::Barrier::new(2);
    thread::scope(|scope| {
        let cancelled = scope.spawn(|| {
            start.wait();
            owner.cancel_external_signing_plan(&explicit)
        });
        let submitted = scope.spawn(|| {
            start.wait();
            other.submit_external(
                &explicit,
                &signature,
                None,
                TxWait::Submitted,
                Duration::from_secs(1),
            )
        });
        let cancelled = cancelled.join().unwrap();
        let submitted = submitted.join().unwrap();
        assert_ne!(cancelled.is_ok(), submitted.is_ok());
    });
    assert!(owner.cancel_external_signing_plan(&explicit).is_err());
    assert!(other
        .assemble_external_extrinsic(&explicit, &signature, None)
        .is_err());
    worker.join().unwrap();
}
