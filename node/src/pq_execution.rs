//! Recognize PQ authentication work without decoding historical runtime calls.
//!
//! The production verifier is reached only through signed-v4 extrinsics. Runtime
//! APIs decode their complete argument containers before executing them, so an
//! incomplete container cannot verify even an otherwise complete early proof.
//! This parser only borrows slices and never allocates from untrusted lengths.

use sp_runtime::codec::{Compact, Decode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Work {
    pub proofs: usize,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnsupportedPqBenchmark;

impl Work {
    fn add(self, other: Self) -> Self {
        Self {
            proofs: self.proofs.saturating_add(other.proofs),
            bytes: self.bytes.saturating_add(other.bytes),
        }
    }
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    let (head, tail) = input.split_at_checked(len)?;
    *input = tail;
    Some(head)
}

fn byte(input: &mut &[u8]) -> Option<u8> {
    take(input, 1)?.first().copied()
}

fn len(input: &mut &[u8]) -> Option<usize> {
    usize::try_from(Compact::<u32>::decode(input).ok()?.0).ok()
}

fn bytes<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let size = len(input)?;
    take(input, size)
}

fn extrinsic(input: &mut &[u8]) -> Option<Work> {
    let before = input.len();
    let mut body = bytes(input)?;
    let encoded_size = before.saturating_sub(input.len());
    if byte(&mut body) != Some(0x84) || byte(&mut body) != Some(2) {
        return Some(Work::default());
    }
    let Some(proof_len) = len(&mut body) else {
        return Some(Work::default());
    };
    if node_subtensor_runtime::hashed_extrinsic::is_post_quantum_proof_len(proof_len)
        && body.len() >= proof_len
    {
        // Include the complete PQ extrinsic, whose call is hashed by verification.
        // Profile and carrier validity remain the runtime's responsibility.
        Some(Work {
            proofs: 1,
            bytes: encoded_size,
        })
    } else {
        Some(Work::default())
    }
}

fn extrinsics(input: &mut &[u8]) -> Option<Work> {
    let count = len(input)?;
    if count > input.len() {
        return None;
    }
    let mut total = Work::default();
    for _ in 0..count {
        total = total.add(extrinsic(input)?);
    }
    Some(total)
}

fn header(input: &mut &[u8]) -> Option<()> {
    // Header<u32, BlakeTwo256>: parent, compact number, roots, digest items.
    take(input, 32)?;
    Compact::<u32>::decode(input).ok()?;
    take(input, 64)?;
    let logs = len(input)?;
    if logs > input.len() {
        return None;
    }
    for _ in 0..logs {
        match byte(input)? {
            0 => {
                bytes(input)?;
            }
            4..=6 => {
                take(input, 4)?; // Consensus engine ID.
                bytes(input)?;
            }
            8 => {} // RuntimeEnvironmentUpdated.
            _ => return None,
        }
    }
    Some(())
}

fn block(input: &mut &[u8]) -> Option<Work> {
    header(input)?;
    extrinsics(input)
}

fn try_block(input: &mut &[u8]) -> Option<Work> {
    let work = block(input)?;
    bool::decode(input).ok()?; // State-root check.
    bool::decode(input).ok()?; // Signature check; hashed accounts always check.
    // TryStateSelect: None, All, RoundRobin(u32), Only(Vec<Vec<u8>>).
    match byte(input)? {
        0 | 1 => {}
        2 => {
            take(input, 4)?;
        }
        3 => {
            let names = len(input)?;
            if names > input.len() {
                return None;
            }
            for _ in 0..names {
                bytes(input)?;
            }
        }
        _ => return None,
    }
    Some(work)
}

fn wallet_benchmark(input: &mut &[u8]) -> Option<()> {
    let pallet = bytes(input)?;
    let instance = bytes(input)?;
    bytes(input)?; // Benchmark name; all benchmarks in this new pallet are local.
    if pallet != b"pallet_hashed_accounts" || instance != b"HashedAccounts" {
        return None;
    }
    let components = len(input)?;
    if components > input.len().checked_div(5)? {
        return None;
    }
    for _ in 0..components {
        if byte(input)? > 25 {
            return None;
        }
        take(input, 4)?;
    }
    if byte(input)? > 1 {
        return None;
    }
    take(input, 4)?; // Internal repeats.
    // New wallet benchmarks include key generation and authority graph setup,
    // even when the measured block does no verification. Configurable repeats
    // are not bounded by a proof count. Keep this new pallet's benchmarks local;
    // trusted benchmark executors bypass admission before classification.
    Some(())
}

/// Work introduced by PQ envelopes in known authentication entry points.
/// New wallet benchmarks require the trusted local benchmark executor.
/// Ordinary APIs, classical envelopes and malformed containers retain the
/// runtime's own behavior. Trailing bytes are intentionally left to the runtime
/// ABI, whose historical versions need not enforce DecodeAll.
pub(crate) fn work(method: &str, mut data: &[u8]) -> Result<Work, UnsupportedPqBenchmark> {
    if method == "Benchmark_dispatch_benchmark" {
        return if wallet_benchmark(&mut data).is_some() {
            Err(UnsupportedPqBenchmark)
        } else {
            Ok(Work::default())
        };
    }
    let detected = match method {
        "TaggedTransactionQueue_validate_transaction" => {
            if !matches!(byte(&mut data), Some(0..=2)) {
                return Ok(Work::default());
            }
            extrinsic(&mut data).filter(|_| take(&mut data, 32).is_some())
        }
        "BlockBuilder_apply_extrinsic" => extrinsic(&mut data),
        "Core_execute_block" => block(&mut data),
        "TryRuntime_execute_block" => try_block(&mut data),
        "EthereumRuntimeRPCApi_pending_block" => extrinsics(&mut data),
        _ => None,
    };
    Ok(detected.unwrap_or_default())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use sp_runtime::{
        codec::Encode,
        generic::{Digest, DigestItem, Header},
        traits::BlakeTwo256,
    };

    fn work(method: &str, data: &[u8]) -> Work {
        super::work(method, data).unwrap()
    }

    fn envelope(raw_len: Option<usize>, call_len: usize) -> Vec<u8> {
        let mut body = vec![0x84];
        if let Some(raw_len) = raw_len {
            body.push(2);
            Compact(raw_len as u32).encode_to(&mut body);
            body.resize(body.len() + raw_len, 0);
        } else {
            body.push(0);
            body.extend_from_slice(&[0; 32]);
        }
        body.resize(body.len() + call_len, 0);
        body.encode()
    }

    fn batch(extrinsics: &[Vec<u8>]) -> Vec<u8> {
        let mut output = Compact(extrinsics.len() as u32).encode();
        for xt in extrinsics {
            output.extend_from_slice(xt);
        }
        output
    }

    fn block_bytes(extrinsics: &[Vec<u8>], logs: Vec<DigestItem>) -> Vec<u8> {
        let mut output = Header::<u32, BlakeTwo256> {
            parent_hash: Default::default(),
            number: 16_384,
            state_root: Default::default(),
            extrinsics_root: Default::default(),
            digest: Digest { logs },
        }
        .encode();
        output.extend(batch(extrinsics));
        output
    }

    fn validation(xt: &[u8]) -> Vec<u8> {
        let mut data = vec![2];
        data.extend_from_slice(xt);
        data.extend_from_slice(&[0; 32]);
        data
    }

    #[test]
    fn classifier_matches_actual_runtime_scale_argument_types() {
        use node_subtensor_runtime::{
            Address, Block, Runtime, RuntimeCall, TxExtension, UncheckedExtrinsic,
        };
        use sp_runtime::{AccountId32, MultiSignature, transaction_validity::TransactionSource};

        let call = RuntimeCall::System(frame_system::Call::<Runtime>::remark {
            remark: vec![42; 200],
        });
        // Decode the ordinary immortal/zero-nonce/zero-tip extension; its actual
        // runtime type supplies the encoding when constructing each extrinsic.
        let mut extension_bytes = &[0u8; 4][..];
        let extension = TxExtension::decode(&mut extension_bytes).unwrap();
        assert!(extension_bytes.is_empty());
        let signature = MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw([0x4d; 64]));
        let classical = UncheckedExtrinsic::new_signed(
            call.clone(),
            Address::Id(AccountId32::new([0; 32])),
            signature.clone(),
            extension.clone(),
        );
        let mut extrinsics = vec![UncheckedExtrinsic::new_bare(call.clone()), classical];
        let mut expected = Work::default();
        for (size, profile) in [
            (5367, 4),
            (5367, 5),
            (1669, 6),
            (1669, 7),
            (3179, 8),
            (3179, 9),
        ] {
            // Correct schema with deliberately synthetic keys/signatures. No
            // cryptography, state validation or dispatch runs in this test.
            let mut raw = vec![0; size];
            raw[32] = 2;
            raw[33] = profile;
            let xt = UncheckedExtrinsic::new_signed(
                call.clone(),
                Address::Raw(raw),
                signature.clone(),
                extension.clone(),
            );
            let encoded = xt.encode();
            assert_eq!(
                UncheckedExtrinsic::decode(&mut encoded.as_slice()).unwrap(),
                xt
            );
            let single = Work {
                proofs: 1,
                bytes: encoded.len(),
            };
            assert_eq!(work("BlockBuilder_apply_extrinsic", &encoded), single);
            assert_eq!(
                work(
                    "TaggedTransactionQueue_validate_transaction",
                    &(TransactionSource::External, &xt, sp_core::H256::zero()).encode()
                ),
                single
            );
            expected = expected.add(single);
            extrinsics.push(xt);
        }
        assert_eq!(
            work("EthereumRuntimeRPCApi_pending_block", &extrinsics.encode()),
            expected
        );
        let block = Block {
            header: node_subtensor_runtime::Header {
                parent_hash: Default::default(),
                number: 100_000,
                state_root: Default::default(),
                extrinsics_root: Default::default(),
                digest: Digest {
                    logs: vec![DigestItem::PreRuntime(*b"BABE", vec![1; 20])],
                },
            },
            extrinsics,
        };
        assert_eq!(work("Core_execute_block", &block.encode()), expected);
        #[cfg(feature = "try-runtime")]
        assert_eq!(
            work(
                "TryRuntime_execute_block",
                &(
                    block,
                    false,
                    false,
                    frame_support::traits::TryStateSelect::All
                )
                    .encode()
            ),
            expected
        );
    }

    #[test]
    fn all_pq_sizes_use_every_production_authentication_entry() {
        for proof_len in [5367, 1669, 3179] {
            let xt = envelope(Some(proof_len), 100);
            let expected = Work {
                proofs: 1,
                bytes: xt.len(),
            };
            for (method, data) in [
                ("BlockBuilder_apply_extrinsic", xt.clone()),
                (
                    "TaggedTransactionQueue_validate_transaction",
                    validation(&xt),
                ),
                (
                    "Core_execute_block",
                    block_bytes(std::slice::from_ref(&xt), vec![]),
                ),
                (
                    "EthereumRuntimeRPCApi_pending_block",
                    batch(std::slice::from_ref(&xt)),
                ),
            ] {
                assert_eq!(work(method, &data), expected, "{method}, {proof_len}");
            }
            let mut data = block_bytes(&[xt], vec![]);
            data.extend_from_slice(&[0, 0, 0]);
            // Signature-check=false still verifies hashed account envelopes.
            assert_eq!(work("TryRuntime_execute_block", &data), expected);
        }
    }

    #[test]
    fn classical_work_exceeds_previous_limits_without_spending_pq_quota() {
        let small = envelope(None, 20);
        let many = vec![small; 1_025];
        let huge = envelope(None, 16 * 1024 * 1024 + 1);
        for data in [batch(&many), batch(std::slice::from_ref(&huge))] {
            assert_eq!(
                work("EthereumRuntimeRPCApi_pending_block", &data),
                Work::default()
            );
        }
        assert_eq!(
            work("Core_execute_block", &block_bytes(&many, vec![])),
            Work::default()
        );
        assert_eq!(work("BlockBuilder_apply_extrinsic", &huge), Work::default());
    }

    #[test]
    fn mixed_batches_charge_only_complete_pq_extrinsics() {
        let first = envelope(Some(5367), 500);
        let last = envelope(Some(1669), 700);
        let data = batch(&[
            envelope(None, 500_000),
            first.clone(),
            envelope(Some(169), 1_000),
            last.clone(),
        ]);
        assert_eq!(
            work("EthereumRuntimeRPCApi_pending_block", &data),
            Work {
                proofs: 2,
                bytes: first.len() + last.len()
            }
        );
    }

    #[test]
    fn digest_variants_are_skipped_without_decoding_runtime_calls() {
        let xt = envelope(Some(3179), 1);
        let logs = vec![
            DigestItem::Other(vec![1; 300]),
            DigestItem::Consensus(*b"BABE", vec![2; 300]),
            DigestItem::Seal(*b"aura", vec![3; 100]),
            DigestItem::PreRuntime(*b"BABE", vec![4; 200]),
            DigestItem::RuntimeEnvironmentUpdated,
        ];
        assert_eq!(
            work(
                "Core_execute_block",
                &block_bytes(std::slice::from_ref(&xt), logs)
            ),
            Work {
                proofs: 1,
                bytes: xt.len()
            }
        );
    }

    #[test]
    fn try_runtime_decodes_all_control_fields_before_any_verification() {
        let xt = envelope(Some(3179), 1);
        for select in [
            vec![0],
            vec![1],
            (2u8, 17u32).encode(),
            (3u8, vec![b"HashedAccounts".to_vec(), b"System".to_vec()]).encode(),
        ] {
            for signature_check in [false, true] {
                let mut data = block_bytes(std::slice::from_ref(&xt), vec![]);
                let controls_offset = data.len();
                (false, signature_check).encode_to(&mut data);
                data.extend_from_slice(&select);
                assert_eq!(
                    work("TryRuntime_execute_block", &data),
                    Work {
                        proofs: 1,
                        bytes: xt.len()
                    }
                );
                for cut in controls_offset..data.len() {
                    assert_eq!(
                        work("TryRuntime_execute_block", &data[..cut]),
                        Work::default()
                    );
                }
            }
        }
    }

    #[test]
    fn malformed_containers_cannot_verify_an_early_proof() {
        let xt = envelope(Some(5367), 100);
        for (method, data) in [
            ("BlockBuilder_apply_extrinsic", xt.clone()),
            (
                "TaggedTransactionQueue_validate_transaction",
                validation(&xt),
            ),
            (
                "Core_execute_block",
                block_bytes(std::slice::from_ref(&xt), vec![]),
            ),
            (
                "EthereumRuntimeRPCApi_pending_block",
                batch(&[xt.clone(), xt]),
            ),
        ] {
            for cut in 0..data.len() {
                assert_eq!(
                    work(method, &data[..cut]),
                    Work::default(),
                    "{method}:{cut}"
                );
            }
        }
    }

    #[test]
    fn raw_lengths_and_extrinsic_framing_match_runtime_envelopes() {
        for length in [0, 169, 1668, 1670, 3178, 3180, 5366, 5368] {
            assert_eq!(
                work("BlockBuilder_apply_extrinsic", &envelope(Some(length), 5)),
                Work::default()
            );
        }
        let mut body = &envelope(Some(5367), 5)[..];
        let mut body = bytes(&mut body).unwrap().to_vec();
        for version in [4, 5, 0x45, 0x85] {
            body[0] = version;
            assert_eq!(
                work("BlockBuilder_apply_extrinsic", &body.encode()),
                Work::default()
            );
        }
        body[0] = 0x84;
        for address_tag in [0, 1, 3, 4, 255] {
            body[1] = address_tag;
            assert_eq!(
                work("BlockBuilder_apply_extrinsic", &body.encode()),
                Work::default()
            );
        }
        body[1] = 2;
        // Unsupported profile/carrier bytes remain bounded until runtime rejection.
        assert_eq!(
            work("BlockBuilder_apply_extrinsic", &body.encode()).proofs,
            1
        );
        body.truncate(10);
        assert_eq!(
            work("BlockBuilder_apply_extrinsic", &body.encode()),
            Work::default()
        );
    }

    #[test]
    fn ordinary_and_unknown_exports_never_spend_pq_quota() {
        let data = envelope(Some(5367), 100);
        for method in [
            "Core_initialize_block",
            "BlockBuilder_finalize_block",
            "BlockBuilder_check_inherents",
            "BlockBuilder_inherent_extrinsics",
            "EthereumRuntimeRPCApi_initialize_pending_block",
            "EthereumRuntimeRPCApi_call",
            "EthereumRuntimeRPCApi_create",
            "ContractsApi_call",
            "ShieldApi_try_decode_shielded_tx",
            "ShieldApi_try_unshield_tx",
            "SubnetInfoRuntimeApi_get_selective_metagraph",
            "SubnetInfoRuntimeApi_get_selective_mechagraph",
            "GrandpaApi_submit_report_equivocation_unsigned_extrinsic",
            "BabeApi_submit_report_equivocation_unsigned_extrinsic",
            "AnyOtherApi_legacy_call",
        ] {
            assert_eq!(work(method, &data), Work::default(), "{method}");
        }
    }

    #[test]
    fn only_new_wallet_benchmarks_require_local_execution() {
        let mut config = frame_benchmarking::BenchmarkConfig {
            pallet: b"pallet_hashed_accounts".to_vec(),
            instance: b"HashedAccounts".to_vec(),
            benchmark: b"authorize_mldsa65".to_vec(),
            selected_components: vec![(frame_benchmarking::BenchmarkParameter::n, 1_000)],
            verify: true,
            internal_repeats: 10,
        };
        let data = config.encode();
        assert_eq!(
            super::work("Benchmark_dispatch_benchmark", &data),
            Err(UnsupportedPqBenchmark)
        );
        for cut in 0..data.len() {
            assert_eq!(
                work("Benchmark_dispatch_benchmark", &data[..cut]),
                Work::default()
            );
        }
        for benchmark in [
            "authorize",
            "register_mldsa65",
            "check_registered_mldsa65",
            "register_derived",
            "register_derived_sponsored",
            "check_derived",
            "register_falcon",
            "authorize_falcon1024",
        ] {
            config.benchmark = benchmark.as_bytes().to_vec();
            assert_eq!(
                super::work("Benchmark_dispatch_benchmark", &config.encode()),
                Err(UnsupportedPqBenchmark)
            );
        }
        config.pallet = b"pallet_balances".to_vec();
        config.benchmark = b"transfer_keep_alive".to_vec();
        assert_eq!(
            work("Benchmark_dispatch_benchmark", &config.encode()),
            Work::default()
        );
    }

    #[test]
    fn runtime_benchmark_selectors_are_exact_not_cli_wildcards() {
        let config = frame_benchmarking::BenchmarkConfig {
            pallet: b"pallet_hashed_accounts".to_vec(),
            instance: b"HashedAccounts".to_vec(),
            benchmark: b"authorize_falcon1024".to_vec(),
            ..Default::default()
        };
        for selector in [b"".as_slice(), b"*", b"ALL"] {
            for field in 0..3 {
                let mut changed = config.clone();
                match field {
                    0 => changed.pallet = selector.to_vec(),
                    1 => changed.instance = selector.to_vec(),
                    _ => changed.benchmark = selector.to_vec(),
                }
                // Pallet/instance selection is exact, with CLI wildcards expanded
                // locally. All function selectors on the new pallet stay local.
                assert_eq!(
                    super::work("Benchmark_dispatch_benchmark", &changed.encode()),
                    if field == 2 {
                        Err(UnsupportedPqBenchmark)
                    } else {
                        Ok(Work::default())
                    }
                );
            }
        }
    }
}
