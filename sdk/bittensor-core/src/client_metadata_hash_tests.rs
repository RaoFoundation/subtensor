use super::*;
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
use ml_kem::{kem::Decapsulate, EncodedSizeUser, KemCore, MlKem768};

fn signing_replies(
    mode: &MetadataHashMode,
    mortal: bool,
) -> Vec<(&'static str, Option<JsonValue>)> {
    let mut replies = vec![
        ("state_getRuntimeVersion", Some(version())),
        ("chain_getHeader", Some(json!({"number":"0x0"}))),
        ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
    ];
    if mortal {
        replies.push(("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))));
    }
    replies.push(("state_getRuntimeVersion", Some(version())));
    if matches!(mode, MetadataHashMode::Auto) {
        replies.extend([
            ("state_getRuntimeVersion", Some(version())),
            ("system_properties", Some(json!({}))),
        ]);
    }
    replies
}

fn key_and_call(runtime: &Runtime) -> (Keypair, Vec<u8>) {
    let key = Keypair::from_seed(&[19; 32], crate::keys::CRYPTO_ED25519).unwrap();
    let call = runtime
        .compose_call(
            "System",
            "remark",
            &Value::record(vec![("remark".into(), Value::Bytes(vec![42]))]),
        )
        .unwrap();
    (key, call)
}

fn params(runtime: &Runtime, mode: &MetadataHashMode, nonce: u64, mortal: bool) -> TxParams {
    TxParams {
        era: if mortal {
            stored_mortal_era(MEV_SHIELD_ERA_PERIOD, 0).0
        } else {
            Value::str("00")
        },
        nonce,
        tip: 0,
        tip_asset_id: None,
        genesis_hash: [1; 32],
        era_block_hash: [1; 32],
        metadata_hash: match mode {
            MetadataHashMode::Disabled => None,
            MetadataHashMode::Explicit(hash) => Some(*hash),
            MetadataHashMode::Auto => Some(
                metadata_digest(
                    &runtime.metadata_bytes,
                    &ChainInfo {
                        spec_version: 419,
                        spec_name: "node-subtensor".into(),
                        base58_prefix: 42,
                        decimals: 9,
                        token_symbol: "TAO".into(),
                    },
                )
                .unwrap(),
            ),
        },
    }
}

fn encoded(runtime: &Runtime, key: &Keypair, call: &[u8], params: &TxParams) -> (Vec<u8>, String) {
    let signature = key
        .sign(&runtime.signature_payload(call, params).unwrap())
        .unwrap();
    let (bytes, hash) = runtime
        .encode_signed_extrinsic(
            call,
            key.public_key_bytes(),
            &signature,
            key.crypto_type(),
            params,
        )
        .unwrap();
    (bytes, hex_prefixed(&hash))
}

// PROTECTED_REGRESSION: native keypair signing must support runtimes without an embedded metadata hash without weakening default binding; changes require explicit human approval.
#[test]
fn native_signing_respects_disabled_explicit_and_default_metadata_modes() {
    for mode in [
        MetadataHashMode::Auto,
        MetadataHashMode::Disabled,
        MetadataHashMode::Explicit([6; 32]),
    ] {
        let (endpoint, worker) = server(signing_replies(&mode, false));
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let runtime = client.runtime().unwrap();
        let (key, call) = key_and_call(&runtime);
        let expected = encoded(&runtime, &key, &call, &params(&runtime, &mode, 7, false));
        let actual = if matches!(mode, MetadataHashMode::Auto) {
            client.sign_extrinsic(&call, &key, 7, None)
        } else {
            client.sign_extrinsic_with_metadata_hash(&call, &key, 7, None, mode)
        }
        .unwrap();
        assert_eq!(actual, expected);
        worker.join().unwrap();
    }
}

// PROTECTED_REGRESSION: default-development runtimes must accept native submission when metadata binding is explicitly disabled; changes require explicit human approval.
#[test]
fn development_runtime_accepts_explicit_opt_out_and_rejects_auto() {
    let template = test_client(vec![], HashMap::new());
    let runtime = template.runtime().unwrap();
    let (key, call) = key_and_call(&runtime);
    for mode in [MetadataHashMode::Auto, MetadataHashMode::Disabled] {
        let enabled = matches!(mode, MetadataHashMode::Auto);
        let (extrinsic, expected_hash) =
            encoded(&runtime, &key, &call, &params(&runtime, &mode, 7, false));
        let mut replies = signing_replies(&mode, false);
        // A default development runtime advertises CheckMetadataHash but has
        // no embedded digest. Its enabled mode therefore fails CannotLookup.
        replies.push((
            "author_submitExtrinsic",
            Some(if enabled {
                json!({"error":{"code":1010,"message":"Invalid Transaction","data":"CannotLookup"}})
            } else {
                json!(expected_hash)
            }),
        ));
        let (endpoint, worker) = inspected_server(replies, move |request| {
            if request["method"] == "author_submitExtrinsic" {
                assert_eq!(request["params"][0], hex_prefixed(&extrinsic));
            }
        });
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let options = TxSubmitOptions {
            nonce: Some(7),
            period: None,
            wait: TxWait::Submitted,
            ..Default::default()
        };
        let outcome = if enabled {
            client.submit_with_cancel(&call, &key, options)
        } else {
            client.submit_with_metadata_hash(&call, &key, options, mode)
        }
        .unwrap();
        assert_eq!(outcome.success, !enabled);
        assert_eq!(outcome.extrinsic_hash, expected_hash);
        if enabled {
            assert!(outcome.message.contains("CannotLookup"));
        }
        worker.join().unwrap();
    }
}

// PROTECTED_REGRESSION: Shield inner and carrier must share the caller-selected metadata mode; changes require explicit human approval.
#[test]
fn shield_signs_inner_and_carrier_with_the_selected_metadata_mode() {
    let template = test_client(vec![], HashMap::new());
    let runtime = template.runtime().unwrap();
    let (key, call) = key_and_call(&runtime);
    for mode in [
        MetadataHashMode::Auto,
        MetadataHashMode::Disabled,
        MetadataHashMode::Explicit([6; 32]),
    ] {
        let (decrypt, encrypt) = MlKem768::generate(&mut rand_core::OsRng);
        let info = storage_info(&runtime, "MevShield", "NextKey").unwrap();
        let mut next_key = vec![];
        runtime
            .encode_id(
                info.value_type,
                &Value::Bytes(encrypt.as_bytes().to_vec()),
                &mut next_key,
            )
            .unwrap();
        let mut replies = vec![
            ("state_getRuntimeVersion", Some(version())),
            ("state_getStorage", Some(json!(hex_prefixed(&next_key)))),
            ("system_accountNextIndex", Some(json!(7))),
        ];
        replies.extend(signing_replies(&mode, true));
        replies.extend([
            ("chain_getHeader", Some(json!({"number":"0x0"}))),
            ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
            (
                "author_submitExtrinsic",
                Some(json!({"error":{"code":1010,"message":"captured carrier"}})),
            ),
        ]);
        let submitted = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&submitted);
        let (endpoint, worker) = inspected_server(replies, move |request| {
            if request["method"] == "author_submitExtrinsic" {
                *captured.lock().unwrap() =
                    decode_hex(request["params"][0].as_str().unwrap()).unwrap();
            }
        });
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let outcome = if matches!(mode, MetadataHashMode::Auto) {
            client.submit_shielded(&call, &key, false)
        } else {
            client.submit_shielded_with_metadata_hash(&call, &key, false, mode.clone())
        }
        .unwrap();
        assert!(is_definite_rejection(&outcome));
        worker.join().unwrap();
        let outer = submitted.lock().unwrap();
        let decoded = runtime.decode_extrinsic(&outer, true).unwrap();
        let Value::List(args) = field(field(&decoded, "call").unwrap(), "call_args").unwrap()
        else {
            panic!("call args")
        };
        let ciphertext = args
            .iter()
            .find_map(|arg| {
                (field(arg, "name") == Some(&Value::str("ciphertext")))
                    .then(|| value_bytes(field(arg, "value").unwrap()).unwrap())
            })
            .unwrap();
        let blob = &ciphertext[16..];
        let kem_len = usize::from(u16::from_le_bytes([blob[0], blob[1]]));
        let kem = ml_kem::Ciphertext::<MlKem768>::try_from(&blob[2..2 + kem_len]).unwrap();
        let shared = decrypt.decapsulate(&kem).unwrap();
        let inner = XChaCha20Poly1305::new_from_slice(shared.as_ref())
            .unwrap()
            .decrypt(
                XNonce::from_slice(&blob[2 + kem_len..2 + kem_len + 24]),
                &blob[2 + kem_len + 24..],
            )
            .unwrap();
        assert_eq!(
            inner,
            encoded(&runtime, &key, &call, &params(&runtime, &mode, 8, true)).0
        );
        let carrier = runtime
            .compose_call(
                "MevShield",
                "submit_encrypted",
                &Value::record(vec![("ciphertext".into(), Value::Bytes(ciphertext))]),
            )
            .unwrap();
        assert_eq!(
            *outer,
            encoded(&runtime, &key, &carrier, &params(&runtime, &mode, 7, true)).0
        );
    }
}

// PROTECTED_REGRESSION: exposing native metadata opt-out must not bypass hardware signer proof requirements; changes require explicit human approval.
#[test]
fn metadata_mode_keeps_required_proofs_and_explicit_hashes() {
    let client = test_client(vec![], HashMap::new());
    let runtime = client.runtime().unwrap();
    assert!(
        matches!(client.metadata_hash_for_runtime(&runtime, MetadataHashMode::Disabled, true),
        Err(CoreError::Policy(message)) if message.contains("requires metadata proof"))
    );
    assert_eq!(
        client
            .metadata_hash_for_runtime(&runtime, MetadataHashMode::Explicit([6; 32]), false)
            .unwrap()
            .0,
        Some([6; 32])
    );
}
