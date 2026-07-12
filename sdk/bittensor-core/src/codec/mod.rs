//! SCALE encode/decode against a [`crate::runtime::Runtime`].
//!
//! The decoded-value shapes are cyscale's, pinned by the shape corpus; the
//! encoder accepts the same lenient inputs the SDK has always fed the codec
//! (ss58 strings for account ids, 0x-hex for byte arrays, `{"Variant": ...}`
//! dicts for enums, `None` for Option).

pub mod batch;
pub mod decode;
pub mod encode;
pub mod extrinsic;
pub mod storage;
pub mod value;

pub use value::Value;

#[cfg(test)]
mod corpus_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use codec::Decode;

    use crate::codec::value::Value;
    use crate::runtime::type_string::TypeSpec;
    use crate::runtime::Runtime;

    fn json_u64(v: &serde_json::Value) -> u64 {
        match v {
            serde_json::Value::Number(n) => n.to_string().parse().unwrap(),
            _ => panic!("fixture value {v} is not an unsigned integer"),
        }
    }

    fn json_i64(v: &serde_json::Value) -> i64 {
        match v {
            serde_json::Value::Number(n) => n.to_string().parse().unwrap(),
            _ => panic!("fixture value {v} is not a signed integer"),
        }
    }

    fn as_u32(value: u64) -> u32 {
        u32::try_from(value).expect("fixture integer fits u32")
    }

    fn as_u16(value: u64) -> u16 {
        u16::try_from(value).expect("fixture integer fits u16")
    }

    fn as_u8(value: u64) -> u8 {
        u8::try_from(value).expect("fixture integer fits u8")
    }

    /// JSON fixture data as codec input values (objects become string-keyed
    /// dicts, matching the Python seam's params).
    fn value_from_json(v: &serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => {
                let text = n.to_string();
                if let Ok(i) = text.parse::<i128>() {
                    Value::Int(i128::from(i))
                } else if let Ok(u) = text.parse::<u128>() {
                    Value::Uint(u)
                } else {
                    panic!("fixture number {n} is not an integer")
                }
            }
            serde_json::Value::String(s) => Value::Str(s.clone()),
            serde_json::Value::Array(items) => {
                Value::List(items.iter().map(value_from_json).collect())
            }
            serde_json::Value::Object(map) => Value::Dict(
                map.iter()
                    .map(|(k, v)| (Value::Str(k.clone()), value_from_json(v)))
                    .collect(),
            ),
        }
    }

    fn golden() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../python/tests/fixtures/golden.json"
        );
        let raw = std::fs::read_to_string(path).expect("golden.json fixture exists");
        serde_json::from_str(&raw).unwrap()
    }

    fn golden_metadata_v15() -> Vec<u8> {
        let golden = golden();
        let hex_str = golden["metadata"]["v15_hex"]
            .as_str()
            .expect("golden.json has metadata.v15_hex");
        let raw = hex::decode(hex_str.trim_start_matches("0x")).unwrap();
        Option::<Vec<u8>>::decode(&mut &raw[..])
            .unwrap()
            .expect("fixture metadata is Some")
    }

    fn runtime() -> Runtime {
        let g = golden();
        Runtime::parse(
            &golden_metadata_v15(),
            as_u32(json_u64(&g["network"]["spec_version"])),
            as_u32(json_u64(&g["network"]["transaction_version"])),
            as_u16(json_u64(&g["network"]["ss58_format"])),
        )
        .expect("golden metadata parses")
    }

    /// The definition of done for the decoder: every recorded
    /// `(type id, SCALE bytes, cyscale shape)` triple reproduces exactly.
    #[test]
    fn decoder_reproduces_the_shape_corpus() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../python/tests/fixtures/shape_corpus/corpus.json"
        );
        let raw = std::fs::read_to_string(path).expect("shape corpus recorded");
        let corpus: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let rt = runtime();
        assert_eq!(
            as_u32(json_u64(&corpus["spec_version"])),
            rt.spec_version,
            "corpus and golden fixture must be recorded from the same runtime"
        );

        let mut failures: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for ty in corpus["types"].as_array().unwrap() {
            let id = as_u32(json_u64(&ty["id"]));
            let name = ty["name"].as_str().unwrap();
            for (i, sample) in ty["samples"].as_array().unwrap().iter().enumerate() {
                let scale_hex = sample["scale_hex"].as_str().unwrap();
                let data = hex::decode(scale_hex.trim_start_matches("0x")).unwrap();
                let expected = &sample["decoded"];
                checked += 1;
                match rt.decode_spec(&TypeSpec::Id(id), &data, true) {
                    Ok(value) => {
                        let got = crate::codec::value::to_corpus_json(&value);
                        if &got != expected {
                            failures.push(format!(
                                "type {id} {name} sample {i}:\n  bytes    {scale_hex}\n  expected {expected}\n  got      {got}"
                            ));
                        }
                    }
                    Err(e) => failures.push(format!(
                        "type {id} {name} sample {i}: decode error {e} (bytes {scale_hex})"
                    )),
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} corpus samples diverged (first 20):\n{}",
            failures.len(),
            checked,
            failures
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Pathological nesting must surface as a codec error, never a stack
    /// overflow: type specs derive from untrusted node metadata, and an
    /// overflow aborts the process (it is not a catchable panic).
    #[test]
    fn pathological_nesting_errors_instead_of_overflowing() {
        let rt = runtime();

        // A 1-tuple recurses into its element without consuming bytes, so
        // this spec models a self-referential metadata type. 1000 levels is
        // far past the cap but shallow enough that the spec's own recursive
        // Drop stays within the stack.
        let mut spec = TypeSpec::Primitive(crate::runtime::type_string::Primitive::U8);
        for _ in 0..1000 {
            spec = TypeSpec::Tuple(vec![spec]);
        }

        let err = rt
            .decode_spec(&spec, &[0u8], true)
            .expect_err("deep nesting must not decode");
        assert!(
            err.to_string().contains("recursion"),
            "unexpected error: {err}"
        );

        let mut value = Value::Int(0);
        for _ in 0..1000 {
            value = Value::Tuple(vec![value]);
        }
        let err = rt
            .encode_spec(&spec, &value)
            .expect_err("deep nesting must not encode");
        assert!(
            err.to_string().contains("recursion"),
            "unexpected error: {err}"
        );
    }

    /// The definition of done for compose_call: every golden call vector
    /// (including the nested Sudo / Utility.batch / Proxy ones) reproduces
    /// byte-identically.
    #[test]
    fn composed_calls_match_the_golden_vectors() {
        let g = golden();
        let rt = runtime();
        let addr = |i: usize| g["ss58"][i]["address"].as_str().unwrap().to_string();
        let (alice, bob, charlie) = (addr(0), addr(1), addr(2));

        let transfer = |dest: &str, value: i128| {
            rt.compose_call(
                "Balances",
                "transfer_keep_alive",
                &Value::record(vec![
                    ("dest".into(), Value::str(dest)),
                    ("value".into(), Value::Int(value)),
                ]),
            )
            .unwrap()
        };

        for case in g["calls"].as_array().unwrap() {
            let module = case["module"].as_str().unwrap();
            let function = case["function"].as_str().unwrap();
            let composed = match module {
                "Sudo" => {
                    let inner = rt
                        .compose_call(
                            "System",
                            "remark",
                            &Value::record(vec![("remark".into(), Value::str("0xdeadbeef"))]),
                        )
                        .unwrap();
                    rt.compose_call(
                        "Sudo",
                        "sudo",
                        &Value::record(vec![("call".into(), Value::Bytes(inner))]),
                    )
                }
                "Utility" => {
                    let t1 = transfer(&bob, 1);
                    let t2 = transfer(&charlie, 2);
                    rt.compose_call(
                        "Utility",
                        "batch",
                        &Value::record(vec![(
                            "calls".into(),
                            Value::List(vec![Value::Bytes(t1), Value::Bytes(t2)]),
                        )]),
                    )
                }
                "Proxy" => {
                    let t1 = transfer(&bob, 1);
                    rt.compose_call(
                        "Proxy",
                        "proxy",
                        &Value::record(vec![
                            ("real".into(), Value::str(alice.clone())),
                            ("force_proxy_type".into(), Value::str("Transfer")),
                            ("call".into(), Value::Bytes(t1)),
                        ]),
                    )
                }
                _ => rt.compose_call(module, function, &value_from_json(&case["params"])),
            }
            .unwrap_or_else(|e| panic!("{module}.{function} failed to compose: {e}"));
            let expected = case["data_hex"].as_str().unwrap();
            assert_eq!(
                format!("0x{}", hex::encode(&composed)),
                expected,
                "{module}.{function} encoding diverged"
            );
        }
    }

    /// Every golden storage-key vector (plain, single map, double map,
    /// account-keyed) reproduces byte-identically.
    #[test]
    fn storage_keys_match_the_golden_vectors() {
        let g = golden();
        let rt = runtime();
        for case in g["storage_keys"].as_array().unwrap() {
            let pallet = case["pallet"].as_str().unwrap();
            let name = case["storage_function"].as_str().unwrap();
            let entry = rt
                .storage_entry(pallet, name)
                .unwrap_or_else(|| panic!("{pallet}.{name} not found"));
            let params: Vec<Value> = case["params"]
                .as_array()
                .unwrap()
                .iter()
                .map(value_from_json)
                .collect();
            let key = rt
                .storage_key(entry, &params)
                .unwrap_or_else(|e| panic!("{pallet}.{name} key failed: {e}"));
            assert_eq!(
                format!("0x{}", hex::encode(&key)),
                case["key_hex"].as_str().unwrap(),
                "{pallet}.{name} storage key diverged"
            );
        }
    }

    /// Map keys recover from raw storage keys the way the query_map fixtures
    /// recorded them.
    #[test]
    fn map_keys_recover_from_the_golden_query_maps() {
        let g = golden();
        let rt = runtime();
        for case in g["query_maps"].as_array().unwrap() {
            let pallet = case["pallet"].as_str().unwrap();
            let name = case["storage_function"].as_str().unwrap();
            let entry = rt.storage_entry(pallet, name).unwrap();
            let fixed = case["params"].as_array().unwrap().len();
            let free = entry.key_types.len() - fixed;
            for (raw_key, expected_pair) in case["raw_keys"]
                .as_array()
                .unwrap()
                .iter()
                .zip(case["pairs"].as_array().unwrap())
            {
                let key = hex::decode(raw_key.as_str().unwrap().trim_start_matches("0x")).unwrap();
                let recovered = rt
                    .decode_storage_key_params(entry, &key, fixed)
                    .unwrap_or_else(|e| panic!("{pallet}.{name} key recovery failed: {e}"));
                assert_eq!(recovered.len(), free);
                // pairs are [key, value]; multi-component keys record a list.
                let expected_key = &expected_pair[0];
                let got = if free == 1 {
                    crate::codec::value::to_corpus_json(&recovered[0])
                } else {
                    crate::codec::value::to_corpus_json(&Value::List(recovered))
                };
                assert_eq!(&got, expected_key, "{pallet}.{name} recovered key diverged");
            }
        }
    }

    fn h256(hex_str: &str) -> [u8; 32] {
        hex::decode(hex_str.trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap()
    }

    fn golden_transfer(rt: &Runtime, g: &serde_json::Value) -> Vec<u8> {
        rt.compose_call(
            "Balances",
            "transfer_keep_alive",
            &Value::record(vec![
                (
                    "dest".into(),
                    Value::str(g["ss58"][1]["address"].as_str().unwrap()),
                ),
                ("value".into(), Value::Int(12345)),
            ]),
        )
        .unwrap()
    }

    /// Both golden signature payloads (immortal and mortal) reproduce
    /// byte-identically, era phase computed from the current block.
    #[test]
    fn signature_payloads_match_the_golden_vectors() {
        let g = golden();
        let rt = runtime();
        let call = golden_transfer(&rt, &g);
        let genesis = h256(g["network"]["genesis_hash"].as_str().unwrap());

        let immortal = &g["signature_payloads"][0];
        let payload = rt
            .signature_payload(
                &call,
                &crate::codec::extrinsic::TxParams {
                    era: Value::str("00"),
                    nonce: json_u64(&immortal["nonce"]),
                    tip: 0,
                    tip_asset_id: None,
                    genesis_hash: genesis,
                    era_block_hash: genesis,
                    metadata_hash: None,
                },
            )
            .unwrap();
        assert_eq!(
            format!("0x{}", hex::encode(&payload)),
            immortal["payload_hex"].as_str().unwrap()
        );

        let mortal = &g["signature_payloads"][1];
        let payload = rt
            .signature_payload(
                &call,
                &crate::codec::extrinsic::TxParams {
                    era: Value::record(vec![
                        (
                            "period".into(),
                            Value::Int(i128::from(json_i64(&mortal["era"]["period"]))),
                        ),
                        (
                            "current".into(),
                            Value::Int(i128::from(json_i64(&mortal["era"]["current"]))),
                        ),
                    ]),
                    nonce: json_u64(&mortal["nonce"]),
                    tip: 0,
                    tip_asset_id: None,
                    genesis_hash: genesis,
                    era_block_hash: h256(mortal["era_birth_block_hash"].as_str().unwrap()),
                    metadata_hash: None,
                },
            )
            .unwrap();
        assert_eq!(
            format!("0x{}", hex::encode(&payload)),
            mortal["payload_hex"].as_str().unwrap()
        );
    }

    /// Signed extrinsic assembly reproduces the golden extrinsic bytes and
    /// hashes exactly.
    #[test]
    fn signed_extrinsics_match_the_golden_vectors() {
        let g = golden();
        let rt = runtime();
        let call = golden_transfer(&rt, &g);
        for case in g["extrinsics"].as_array().unwrap() {
            let era = if case["era"] == "00" {
                Value::str("00")
            } else {
                value_from_json(&case["era"])
            };
            let signature = hex::decode(
                case["signature_hex"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("0x"),
            )
            .unwrap();
            let (data, hash) = rt
                .encode_signed_extrinsic(
                    &call,
                    h256(case["public_key_hex"].as_str().unwrap()),
                    &signature,
                    as_u8(json_u64(&case["signature_version"])),
                    &crate::codec::extrinsic::TxParams {
                        era,
                        nonce: json_u64(&case["nonce"]),
                        tip: json_u64(&case["tip"]).into(),
                        tip_asset_id: None,
                        genesis_hash: [0; 32],
                        era_block_hash: [0; 32],
                        metadata_hash: None,
                    },
                )
                .unwrap();
            assert_eq!(
                format!("0x{}", hex::encode(&data)),
                case["extrinsic_hex"].as_str().unwrap()
            );
            assert_eq!(
                format!("0x{}", hex::encode(hash)),
                case["extrinsic_hash"].as_str().unwrap()
            );
        }
    }

    /// Raw block extrinsics decode into the exact dicts cyscale produced.
    #[test]
    fn extrinsics_decode_like_the_golden_block() {
        let g = golden();
        let rt = runtime();
        let raws = g["block"]["raw"]["block"]["extrinsics"].as_array().unwrap();
        let olds = g["block"]["decoded_extrinsics"].as_array().unwrap();
        assert_eq!(raws.len(), olds.len());
        for (raw, old) in raws.iter().zip(olds) {
            let data = hex::decode(raw.as_str().unwrap().trim_start_matches("0x")).unwrap();
            let decoded = rt.decode_extrinsic(&data, true).unwrap();
            assert_eq!(&crate::codec::value::to_corpus_json(&decoded), old);
        }
    }

    /// Multisig account derivation matches the golden fixtures (including the
    /// unsorted-signatories case the derivation must normalize).
    #[test]
    fn multisig_accounts_match_the_golden_vectors() {
        let g = golden();
        let ss58_format = as_u16(json_u64(&g["network"]["ss58_format"]));
        for case in g["multisig"].as_array().unwrap() {
            let signatories: Vec<[u8; 32]> = case["signatories"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| crate::keys::public_key_from_ss58(s.as_str().unwrap()).unwrap())
                .collect();
            let threshold = as_u16(json_u64(&case["threshold"]));
            let (account, sorted) =
                crate::codec::extrinsic::multisig_account_id(&signatories, threshold).unwrap();
            assert_eq!(
                crate::keys::ss58_from_public(account, ss58_format),
                case["ss58_address"].as_str().unwrap()
            );
            assert_eq!(sorted.len(), signatories.len());
        }
    }

    /// Composed calls decode back into cyscale's call dict shape.
    #[test]
    fn composed_calls_decode_back() {
        let g = golden();
        let rt = runtime();
        let case = &g["calls"].as_array().unwrap()[0];
        let data =
            hex::decode(case["data_hex"].as_str().unwrap().trim_start_matches("0x")).unwrap();
        let decoded = rt.decode_spec(&TypeSpec::Call, &data, true).unwrap();
        let json = crate::codec::value::to_corpus_json(&decoded);
        assert_eq!(json["call_module"], "Balances");
        assert_eq!(json["call_function"], "transfer_keep_alive");
        assert_eq!(json["call_args"][0]["value"], g["ss58"][1]["address"]);
    }
}
