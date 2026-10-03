#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]

use crate::{
    Precompiles, hash,
    mock::{Runtime, new_test_ext},
};
use fp_evm::{Context, ExitError, ExitSucceed, PrecompileFailure};
use pallet_evm::Runner;
use pallet_evm::{IsPrecompileResult, PrecompileSet};
use precompile_utils::prelude::{Address, UnboundedBytes};
use precompile_utils::solidity::{decode_return_value, encode_with_selector};
use precompile_utils::testing::MockHandle;
use serde_json::Value;
use sp_core::U256;

fn solidity_fixture() -> Value {
    serde_json::from_str(include_str!("../tests/fixtures/bls12381-solidity.json")).unwrap()
}

fn evm_call(target: u64, input: Vec<u8>) -> fp_evm::CallInfo {
    <Runtime as pallet_evm::Config>::Runner::call(
        hash(0xbeef),
        hash(target),
        input,
        U256::zero(),
        5_000_000,
        None,
        None,
        None,
        vec![],
        vec![],
        false,
        false,
        None,
        None,
        <Runtime as pallet_evm::Config>::config(),
    )
    .unwrap()
}

#[test]
fn eip2537_solidity_verifies_bls_signatures_and_rejects_wrong_messages_and_infinity() {
    new_test_ext().execute_with(|| {
        let fixture = solidity_fixture();
        pallet_evm::AccountCodes::<Runtime>::insert(
            hash(0xcafe),
            hex::decode(fixture.get("verifier").unwrap().as_str().unwrap()).unwrap(),
        );
        crate::mock::fund_account(&crate::mock::mapped_account(hash(0xbeef)), 1_000_000_000);
        let verify = |public_key: Vec<u8>, signature: Vec<u8>, message: Vec<u8>| {
            let input = encode_with_selector(
                crate::mock::selector_u32("verify(bytes,bytes,bytes)"),
                (
                    UnboundedBytes::from(public_key),
                    UnboundedBytes::from(signature),
                    UnboundedBytes::from(message),
                ),
            );
            let result = evm_call(0xcafe, input);
            assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
            decode_return_value::<bool>(&result.value).unwrap()
        };
        for vector in fixture.get("vectors").unwrap().as_array().unwrap() {
            let public_key = hex::decode(vector["publicKey"].as_str().unwrap()).unwrap();
            let signature = hex::decode(vector["signature"].as_str().unwrap()).unwrap();
            let message = hex::decode(vector["message"].as_str().unwrap()).unwrap();
            assert!(verify(
                public_key.clone(),
                signature.clone(),
                message.clone()
            ));
            let mut wrong_message = message.clone();
            wrong_message.push(0xff);
            assert!(!verify(
                public_key.clone(),
                signature.clone(),
                wrong_message
            ));
            assert!(!verify(vec![0; 128], signature.clone(), message.clone()));
            assert!(!verify(public_key.clone(), vec![0; 256], message.clone()));
            assert!(!verify(
                public_key,
                signature.get(..96).unwrap().to_vec(),
                message
            ));
        }
    });
}

#[test]
fn eip2537_evm_call_modes_and_exceptional_gas_burning() {
    new_test_ext().execute_with(|| {
        let fixture = solidity_fixture();
        pallet_evm::AccountCodes::<Runtime>::insert(
            hash(0xcafe),
            hex::decode(fixture.get("harness").unwrap().as_str().unwrap()).unwrap(),
        );
        crate::mock::fund_account(&crate::mock::mapped_account(hash(0xbeef)), 1_000_000_000);
        let call = |address, data, limit, mode| {
            let input = encode_with_selector(
                crate::mock::selector_u32("invoke(address,bytes,uint256,uint256)"),
                (
                    Address(hash(address)),
                    UnboundedBytes::from(data),
                    U256::from(limit),
                    U256::from(mode),
                ),
            );
            let result = evm_call(0xcafe, input);
            assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
            let mut reader = precompile_utils::solidity::codec::Reader::new(&result.value);
            (
                reader.read::<bool>().unwrap(),
                reader.read::<UnboundedBytes>().unwrap(),
                reader.read::<U256>().unwrap(),
            )
        };
        for (address, length, output_length) in [
            (0x0b, 256, 128),
            (0x0c, 160, 128),
            (0x0d, 512, 256),
            (0x0e, 288, 256),
            (0x0f, 384, 32),
            (0x10, 64, 128),
            (0x11, 128, 256),
        ] {
            for mode in 0u64..4 {
                let (ok, output, _) = call(address, vec![0; length], 100_000u64, mode);
                assert!(ok, "address {address:#x}, mode {mode}");
                assert_eq!(output.as_bytes().len(), output_length);
                // Malformed calldata causes an exceptional halt, consuming all forwarded gas.
                let (ok, output, spent) = call(address, vec![0; length - 1], 100_000u64, mode);
                assert!(!ok);
                assert!(output.as_bytes().is_empty());
                assert!(spent >= U256::from(100_000u64));
                // Insufficient gas also fails without returning an apparently valid point.
                let (ok, output, spent) = call(address, vec![0; length], 1u64, mode);
                assert!(!ok);
                assert!(output.as_bytes().is_empty());
                assert!(spent >= U256::one());
            }
        }
    });
}

fn handle(address: u64, input: Vec<u8>, gas: u64) -> MockHandle {
    let mut handle = MockHandle::new(
        hash(address),
        Context {
            address: hash(address),
            caller: hash(0xbeef),
            apparent_value: U256::zero(),
        },
    );
    handle.input = input;
    handle.gas_limit = gas;
    handle
}

// The same pinned Ethereum fixtures are tested through Subtensor's real router,
// including exact gas and a limit one gas below the required charge.
fn success_vectors(address: u64, json: &str) {
    new_test_ext().execute_with(|| {
        let precompiles = Precompiles::<Runtime>::new();
        let tests: Value = serde_json::from_str(json).unwrap();
        for test in tests.as_array().unwrap() {
            let input = hex::decode(test["Input"].as_str().unwrap()).unwrap();
            let expected = hex::decode(test["Expected"].as_str().unwrap()).unwrap();
            let gas = test["Gas"].as_u64().unwrap();
            let mut handle = handle(address, input, gas);
            let result = precompiles
                .execute(&mut handle)
                .unwrap()
                .unwrap_or_else(|err| panic!("{}: {err:?}", test["Name"]));
            assert_eq!(
                result.exit_status,
                ExitSucceed::Returned,
                "{}",
                test["Name"]
            );
            assert_eq!(result.output, expected, "{}", test["Name"]);
            assert_eq!(handle.gas_used, gas, "{}", test["Name"]);

            handle.gas_used = 0;
            handle.gas_limit = gas - 1;
            assert_eq!(
                precompiles.execute(&mut handle),
                Some(Err(PrecompileFailure::Error {
                    exit_status: ExitError::OutOfGas,
                })),
                "{}",
                test["Name"]
            );
        }
    });
}

fn failure_vectors(address: u64, json: &str) {
    new_test_ext().execute_with(|| {
        let tests: Value = serde_json::from_str(json).unwrap();
        for test in tests.as_array().unwrap() {
            let input = hex::decode(test["Input"].as_str().unwrap()).unwrap();
            let mut handle = handle(address, input, u64::MAX);
            assert!(
                matches!(
                    Precompiles::<Runtime>::new().execute(&mut handle),
                    Some(Err(PrecompileFailure::Error { .. }))
                ),
                "{}",
                test["Name"]
            );
        }
    });
}

macro_rules! vectors {
    ($name:ident, $address:expr, $file:literal) => {
        #[test]
        fn $name() {
            success_vectors(
                $address,
                include_str!(concat!(
                    "../../vendor/frontier/frame/evm/precompile/bls12381/testdata/",
                    $file,
                    ".json"
                )),
            );
            failure_vectors(
                $address,
                include_str!(concat!(
                    "../../vendor/frontier/frame/evm/precompile/bls12381/testdata/fail-",
                    $file,
                    ".json"
                )),
            );
        }
    };
}

vectors!(eip2537_g1_add, 0x0b, "blsG1Add");
vectors!(eip2537_g1_msm, 0x0c, "blsG1MultiExp");
vectors!(eip2537_g2_add, 0x0d, "blsG2Add");
vectors!(eip2537_g2_msm, 0x0e, "blsG2MultiExp");
vectors!(eip2537_pairing, 0x0f, "blsPairing");
vectors!(eip2537_map_g1, 0x10, "blsMapG1");
vectors!(eip2537_map_g2, 0x11, "blsMapG2");

#[test]
fn eip2537_addresses_are_registered_and_allow_static_and_foreign_frames() {
    new_test_ext().execute_with(|| {
        let precompiles = Precompiles::<Runtime>::new();
        for (address, input_len, output_len, gas) in [
            (0x0b, 256, 128, 375),
            (0x0c, 160, 128, 12_000),
            (0x0d, 512, 256, 600),
            (0x0e, 288, 256, 22_500),
            (0x0f, 384, 32, 70_300),
            (0x10, 64, 128, 5_500),
            (0x11, 128, 256, 23_800),
        ] {
            assert!(Precompiles::<Runtime>::used_addresses().contains(&hash(address)));
            assert!(matches!(
                precompiles.is_precompile(hash(address), 0),
                IsPrecompileResult::Answer {
                    is_precompile: true,
                    extra_cost: 0
                }
            ));
            for is_static in [false, true] {
                for foreign_frame in [false, true] {
                    let mut handle = handle(address, vec![0; input_len], gas);
                    handle.is_static = is_static;
                    if foreign_frame {
                        handle.context.address = hash(0xcafe);
                    }
                    let output = precompiles.execute(&mut handle).unwrap().unwrap();
                    assert_eq!(output.output.len(), output_len);
                    assert_eq!(handle.gas_used, gas);
                }
            }
        }
        for unused in [0x0a, 0x12] {
            assert!(!Precompiles::<Runtime>::used_addresses().contains(&hash(unused)));
        }
    });
}
