//! Diagnostic: replicate the tx-pool validation of a plain EIP-1559 transfer
//! signed by the well-known anvil deployer key, against the real runtime.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use ethereum::eip2930::TransactionSignature as Eip1559Signature;
use ethereum::{EIP1559TransactionMessage, TransactionAction};
use fp_self_contained::SelfContainedCall;
use frame_support::dispatch::GetDispatchInfo;
use node_subtensor_runtime::{
    BuildStorage, Executive, Runtime, RuntimeCall, RuntimeGenesisConfig, UncheckedExtrinsic,
};
use sp_runtime::transaction_validity::TransactionSource;
use pallet_evm::AddressMapping;
use sp_core::{H160, H256, U256};
use subtensor_runtime_common::TaoBalance;

/// Canonical anvil developer key #0 (public test key), address 0xf39F...2266.
const DEPLOYER_PK: [u8; 32] = [
    0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38, 0xff,
    0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b, 0xf4, 0xf2,
    0xff, 0x80,
];

fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
        ..Default::default()
    }
    .build_storage()
    .unwrap()
    .into();
    ext.execute_with(|| {
        frame_system::Pallet::<Runtime>::set_block_number(1);
        // Match the localnet EVM chain id.
        pallet_evm_chain_id::ChainId::<Runtime>::put(42);
    });
    ext
}

fn deployer_address() -> H160 {
    let secret = libsecp256k1::SecretKey::parse(&DEPLOYER_PK).unwrap();
    let public = libsecp256k1::PublicKey::from_secret_key(&secret);
    let hash = sp_io::hashing::keccak_256(&public.serialize()[1..]);
    H160::from_slice(&hash[12..])
}

fn signed_transfer() -> pallet_ethereum::Transaction {
    let msg = EIP1559TransactionMessage {
        chain_id: 42,
        nonce: U256::zero(),
        max_priority_fee_per_gas: U256::zero(),
        max_fee_per_gas: U256::from(20_000_000_000u64),
        gas_limit: U256::from(21_000u64),
        action: TransactionAction::Call(H160::from_low_u64_be(0xbeef)),
        value: U256::from(1_000_000_000u64),
        input: vec![],
        access_list: vec![],
    };
    let secret = libsecp256k1::SecretKey::parse(&DEPLOYER_PK).unwrap();
    let signing_message = libsecp256k1::Message::parse_slice(&msg.hash()[..]).unwrap();
    let (signature, recid) = libsecp256k1::sign(&signing_message, &secret);
    let rs = signature.serialize();
    let r = H256::from_slice(&rs[0..32]);
    let s = H256::from_slice(&rs[32..64]);
    pallet_ethereum::Transaction::EIP1559(ethereum::EIP1559Transaction {
        chain_id: msg.chain_id,
        nonce: msg.nonce,
        max_priority_fee_per_gas: msg.max_priority_fee_per_gas,
        max_fee_per_gas: msg.max_fee_per_gas,
        gas_limit: msg.gas_limit,
        action: msg.action,
        value: msg.value,
        input: msg.input.clone(),
        access_list: msg.access_list,
        signature: Eip1559Signature::new(recid.serialize() != 0, r, s).unwrap(),
    })
}

#[test]
fn anvil_deployer_transfer_passes_pool_validation() {
    new_test_ext().execute_with(|| {
        let deployer = deployer_address();
        let mirror = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(deployer);

        // Fund like the rig bootstrap: 1M TAO.
        let amount = TaoBalance::from(1_000_000_000_000_000u64);
        let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(amount);
        let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(&mirror, credit, amount).unwrap();

        let call = RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
            transaction: signed_transfer(),
        });

        let info = call.check_self_contained().expect("is self contained");
        let recovered = info.expect("signature recovers");
        assert_eq!(recovered, deployer, "recovered sender mismatch");

        let dispatch_info = call.get_dispatch_info();
        let validity = call
            .validate_self_contained(&recovered, &dispatch_info, 0)
            .expect("self contained validation ran");
        assert!(
            validity.is_ok(),
            "pool validation failed: {:?}",
            validity.unwrap_err()
        );
    });
}

/// Raw legacy tx produced by `cast mktx` with the anvil deployer key:
/// to=0x7099..79C8, value=1e9, gasPrice=20 gwei, gasLimit=21000, nonce=0, chain=42.
const CAST_RAW_TX: &str = "f868808504a817c8008252089470997970c51812dc3a010c7d01b50e0d17dc79c8843b9aca008078a0ce7c2757764bafe941e2cb493a74f92a0a9e08147434b339f1d6e158b0f1cae0a02bdb47e97e946f2d31116af519616c988b1c7c3aaa0b77b918b611bece4051d0";

#[test]
fn cast_raw_tx_passes_executive_validation() {
    use ethereum::EnvelopedDecodable;

    let raw = hex::decode(CAST_RAW_TX).unwrap();
    let tx = <pallet_ethereum::Transaction as EnvelopedDecodable>::decode(&raw).unwrap();

    new_test_ext().execute_with(|| {
        let call = RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
            transaction: tx.clone(),
        });
        let recovered = call
            .check_self_contained()
            .expect("self contained")
            .expect("recovers");
        assert_eq!(recovered, deployer_address(), "recovered sender mismatch");

        let mirror = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(recovered);
        let amount = TaoBalance::from(1_000_000_000_000_000u64);
        let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(amount);
        let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(&mirror, credit, amount).unwrap();

        let uxt = UncheckedExtrinsic::new_bare(call);

        let result = Executive::validate_transaction(
            TransactionSource::External,
            uxt,
            frame_system::Pallet::<Runtime>::block_hash(0u32),
        );
        assert!(
            result.is_ok(),
            "executive validation failed: {:?}",
            result.unwrap_err()
        );
    });
}

#[test]
fn anvil_deployer_transfer_passes_executive_validation() {
    new_test_ext().execute_with(|| {
        let deployer = deployer_address();
        let mirror = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(deployer);

        let amount = TaoBalance::from(1_000_000_000_000_000u64);
        let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(amount);
        let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(&mirror, credit, amount).unwrap();

        let call = RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
            transaction: signed_transfer(),
        });
        let uxt = UncheckedExtrinsic::new_bare(call);

        let result = Executive::validate_transaction(
            TransactionSource::External,
            uxt,
            frame_system::Pallet::<Runtime>::block_hash(0u32),
        );
        assert!(
            result.is_ok(),
            "executive validation failed: {:?}",
            result.unwrap_err()
        );
    });
}
