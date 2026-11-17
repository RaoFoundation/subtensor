use super::*;
use sp_core::{blake2_256, Pair as _};

struct Signer {
    address: String,
    public_key: [u8; 32],
    crypto_type: u8,
    sign: Box<dyn Fn(&[u8]) -> Vec<u8>>,
}

fn signer(crypto_type: u8) -> Signer {
    if crypto_type == crate::keys::CRYPTO_ECDSA {
        let key = sp_core::ecdsa::Pair::from_seed(&[23; 32]);
        let account = crate::keys::account_id_from_ecdsa_public(key.public().as_ref()).unwrap();
        Signer {
            address: crate::keys::ss58_from_public(account, 42),
            public_key: account,
            crypto_type,
            sign: Box::new(move |payload| key.sign(payload).0.to_vec()),
        }
    } else {
        let key = Keypair::from_seed(&[23; 32], crypto_type).unwrap();
        Signer {
            address: key.ss58_address(),
            public_key: key.public_key_bytes(),
            crypto_type,
            sign: Box::new(move |payload| key.sign(payload).unwrap()),
        }
    }
}

fn plan(client: &Client, signer: &Signer, remark_size: usize) -> ExternalSigningPlan {
    let call = client
        .runtime()
        .unwrap()
        .compose_call(
            "System",
            "remark",
            &Value::record(vec![("remark".into(), Value::Bytes(vec![7; remark_size]))]),
        )
        .unwrap();
    client
        .external_signing_plan(
            &call,
            ExternalSigner {
                ss58_address: signer.address.clone(),
                public_key: signer.public_key,
                crypto_type: signer.crypto_type,
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

// PROTECTED_REGRESSION: transaction assembly must reject message-wrapped signatures that the chain cannot execute; changes require explicit human approval.
#[test]
fn external_assembly_requires_exact_raw_or_prehashed_transaction_payloads() {
    for crypto_type in [0, 1, 2] {
        for remark_size in [1, 1024] {
            let (endpoint, worker) = server(external_plan_replies());
            let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
            let signer = signer(crypto_type);
            let plan = plan(&client, &signer, remark_size);
            if remark_size > 256 {
                assert_eq!(
                    plan.payload.len(),
                    32,
                    "large signing payload must be hashed"
                );
            }
            let wrapped = [b"<Bytes>".as_slice(), &plan.payload, b"</Bytes>"].concat();
            let signature = (signer.sign)(&wrapped);
            assert!(
                crate::keys::verify(&plan.payload, &signature, &signer.address, crypto_type)
                    .unwrap()
            );
            assert!(!crate::keys::verify_strict(
                &plan.payload,
                &signature,
                &signer.address,
                crypto_type
            )
            .unwrap());
            assert!(client
                .assemble_external_extrinsic(&plan, &signature, None)
                .is_err());
            let raw_signature = (signer.sign)(&plan.payload);
            let (bytes, hash) = client
                .assemble_external_extrinsic(&plan, &raw_signature, None)
                .unwrap();
            assert!(!bytes.is_empty());
            assert_eq!(hash, hex_prefixed(&blake2_256(&bytes)));
            client.cancel_external_signing_plan(&plan).unwrap();
            worker.join().unwrap();
        }
    }
}

// PROTECTED_REGRESSION: a rejected wrapped transaction signature must never reach broadcast or strand its automatic nonce; changes require explicit human approval.
#[test]
fn rejected_external_submission_releases_nonce_without_broadcast() {
    for crypto_type in [0, 1, 2] {
        let mut replies = external_plan_replies();
        // If submission reaches the node, the fixture observes author_submitExtrinsic
        // instead of this next automatic nonce read and fails the test.
        replies.push(("system_accountNextIndex", Some(json!(7))));
        let (endpoint, worker) = server(replies);
        let client = test_client(vec![endpoint], HashMap::from([(0, Instant::now())]));
        let signer = signer(crypto_type);
        let plan = plan(&client, &signer, 1);
        let wrapped = [b"<Bytes>".as_slice(), &plan.payload, b"</Bytes>"].concat();
        assert!(client
            .submit_external(
                &plan,
                &(signer.sign)(&wrapped),
                None,
                TxWait::Submitted,
                Duration::from_secs(1)
            )
            .is_err());
        assert_eq!(client.reserve_nonce(&signer.address).unwrap(), 7);
        assert!(client
            .assemble_external_extrinsic(&plan, &(signer.sign)(&plan.payload), None)
            .is_err());
        worker.join().unwrap();
    }
}
