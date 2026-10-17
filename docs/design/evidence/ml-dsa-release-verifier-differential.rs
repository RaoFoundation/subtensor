//! Existing-API differential for the unchanged release Wasm and its native runtime.
//!
//! Run via ml-dsa-release-verifier-differential.py after building the release node.
//! All seeds are public test constants. Each case uses isolated externalities.
//! Sudo unchecked dispatch deliberately bypasses dispatch admission to exercise
//! the shared verifier while production activation remains gated. It changes no
//! runtime code and is not a production activation or throughput measurement.
use codec::{Decode, Encode};
use frame_support::{
    traits::{Currency, Get},
    weights::Weight,
};
use node_subtensor_runtime::{
    Balances, BuildStorage, LimitOrders, OriginCaller, Runtime, RuntimeCall, RuntimeEvent,
    RuntimeGenesisConfig, SubtensorModule, System, TxExtension, UncheckedExtrinsic,
};
use pallet_hashed_accounts::{
    ActiveCapabilities, CapabilityRecord, CapabilityRole, CapabilityScope,
};
use pallet_limit_orders::{
    MlDsaOrderProof, Order, OrderSignature, OrderType, SignedOrder, VersionedOrder,
};
use sp_core::{sr25519, Pair, H256};
use sp_runtime::{
    traits::{AccountIdConversion, TransactionExtension},
    AccountId32, Perbill,
};
use subtensor_hashed::mldsa;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};
fn setup(
    public: &mldsa::PublicKeyBytes,
    owner: &AccountId32,
    sudo: &AccountId32,
) -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        frame_system::BlockHash::<Runtime>::insert(0, H256::zero());
        pallet_sudo::Key::<Runtime>::put(sudo);
        let _ = Balances::make_free_balance_be(sudo, TaoBalance::new(1_000_000_000_000_000));
        let _ = Balances::make_free_balance_be(owner, TaoBalance::new(1_000_000_000_000));
        let netuid = NetUid::from(42u16);
        SubtensorModule::init_new_network(netuid, 100);
        pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 0);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::new(1_000_000_000_000));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            netuid,
            AlphaBalance::new(1_000_000_000_000),
        );
        SubtensorModule::create_account_if_non_existent(owner, owner).unwrap();
        let intermediary: AccountId32 =
            node_subtensor_runtime::LimitOrdersPalletId::get().into_account_truncating();
        let hotkey = node_subtensor_runtime::LimitOrdersPalletHotkey::get();
        SubtensorModule::create_account_if_non_existent(&intermediary, &hotkey).unwrap();
        let _ = Balances::make_free_balance_be(&intermediary, TaoBalance::new(1_000_000_000));
        pallet_limit_orders::LimitOrdersEnabled::<Runtime>::put(true);
        node_subtensor_runtime::AdminUtils::sudo_set_hashed_accounts_enabled(
            node_subtensor_runtime::RuntimeOrigin::root(), true,
        ).unwrap();
        pallet_hashed_accounts::MlDsaAuthorizationEnabled::<Runtime>::put(true);
        ActiveCapabilities::<Runtime>::insert(
            owner,
            CapabilityRole::Order,
            CapabilityRecord {
                epoch: 1,
                key_commitment: mldsa::commitment(public),
                expires_at: 100,
                scope: CapabilityScope::UnrestrictedTrading,
                spent_tao: 0,
                spent_alpha: 0,
            },
        );
    });
    ext
}
fn transaction(
    order: VersionedOrder<AccountId32>,
    public: mldsa::PublicKeyBytes,
    signature: Vec<u8>,
    owner: &AccountId32,
    pair: &sr25519::Pair,
) -> Vec<u8> {
    let call = RuntimeCall::LimitOrders(pallet_limit_orders::Call::execute_orders {
        orders: vec![SignedOrder {
            order: order.with_nonce(0).unwrap(),
            signature: OrderSignature::MlDsa(Box::new(MlDsaOrderProof {
                epoch: 1,
                public_key: public,
                signature: signature.try_into().unwrap(),
            })),
            partial_fill: None,
        }]
        .try_into()
        .unwrap(),
        should_fail: true,
    });
    let call = RuntimeCall::Utility(pallet_subtensor_utility::Call::dispatch_as {
        as_origin: Box::new(OriginCaller::system(frame_system::RawOrigin::Signed(
            owner.clone(),
        ))),
        call: Box::new(call),
    });
    let call = RuntimeCall::Sudo(pallet_sudo::Call::sudo_unchecked_weight {
        call: Box::new(call),
        weight: Weight::from_parts(100_000_000, 100_000),
    });
    let extra = TxExtension::decode(&mut &[0u8; 64][..]).unwrap();
    let payload = (&call, &extra, extra.implicit().unwrap()).encode();
    let signature = pair.sign(&sp_core::hashing::blake2_256(&payload));
    UncheckedExtrinsic::new_signed(
        call,
        sp_runtime::MultiAddress::Id(pair.public().into()),
        sp_runtime::MultiSignature::Sr25519(signature),
        extra,
    )
    .encode()
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let wasm = std::fs::read(&args[1]).unwrap();
    assert_eq!(
        node_subtensor_runtime::WASM_BINARY.unwrap(),
        wasm.as_slice(),
        "cached native runtime does not embed the supplied release Wasm"
    );
    let executor = sc_executor::WasmExecutor::<sp_io::SubstrateHostFunctions>::builder().build();
    let pair = sr25519::Pair::from_seed(&[87; 32]);
    let sudo: AccountId32 = pair.public().into();
    let owner = AccountId32::new([88; 32]);
    let seed = [89; 32];
    let public = mldsa::public_key_from_seed(&seed).unwrap();
    let mut ext = setup(&public, &owner, &sudo);
    let version_native =
        ext.execute_with(|| node_subtensor_runtime::api::dispatch("Core_version", &[]).unwrap());
    let version_wasm = executor
        .uncached_call(
            sc_executor_common::runtime_blob::RuntimeBlob::uncompress_if_needed(&wasm).unwrap(),
            &mut ext.ext(),
            true,
            "Core_version",
            &[],
        )
        .unwrap();
    assert_eq!(version_native, version_wasm, "runtime versions differ");
    let order = ext.execute_with(|| {
        VersionedOrder::V1(Order {
            signer: owner.clone(),
            hotkey: owner.clone(),
            netuid: 42u16.into(),
            order_type: OrderType::LimitBuy,
            amount: 1_000_000_000,
            limit_price: u64::MAX,
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient: owner.clone(),
            relayer: None,
            max_slippage: None,
            chain_id: node_subtensor_runtime::ConfigurableChainId::get(),
            partial_fills_enabled: false,
        })
    });
    let payload = ext.execute_with(|| LimitOrders::order_signing_payload(1, 0, &order));
    let sig = mldsa::sign_message(
        &seed,
        &payload,
        pallet_limit_orders::ORDER_CONTEXT,
        &[0; 32],
    )
    .unwrap();
    let mut cases = vec![
        ("valid_deterministic".to_string(), sig.to_vec(), true),
        (
            "valid_randomized".into(),
            mldsa::sign_message(
                &seed,
                &payload,
                pallet_limit_orders::ORDER_CONTEXT,
                &[91; 32],
            )
            .unwrap()
            .to_vec(),
            true,
        ),
        ("empty_signature".into(), vec![], false),
        ("short_signature".into(), sig[..3308].to_vec(), false),
        ("zero_signature".into(), vec![0; 3309], false),
        (
            "wrong_context".into(),
            mldsa::sign_message(&seed, &payload, b"wrong-context", &[0; 32])
                .unwrap()
                .to_vec(),
            false,
        ),
        (
            "wrong_message".into(),
            mldsa::sign_message(
                &seed,
                b"wrong-message",
                pallet_limit_orders::ORDER_CONTEXT,
                &[0; 32],
            )
            .unwrap()
            .to_vec(),
            false,
        ),
    ];
    for (name, index, value) in [
        ("challenge_bitflip", 0, 1),
        ("response_bitflip", 100, 1),
        ("hint_counter_out_of_range", 3308, 255),
        ("hint_duplicate", 3249, 0),
        ("nonzero_hint_padding", 3302, 255),
    ] {
        let mut s = sig.to_vec();
        if name == "hint_duplicate" {
            s[3248] = 0;
            s[3249] = 0;
            s[3303] = 2;
        } else if name.contains("bitflip") {
            s[index] ^= value;
        } else {
            s[index] = value;
        }
        cases.push((name.into(), s, false));
    }
    let mut results = vec![];
    for (name, signature, expected) in cases {
        let mut native = setup(&public, &owner, &sudo);
        let mut interpreted = setup(&public, &owner, &sudo);
        let id = native.execute_with(|| LimitOrders::derive_authorized_order_id(0, &order));
        let (balance_before, stake_before) = native.execute_with(|| {
            (
                Balances::free_balance(&owner),
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &owner,
                    &owner,
                    42u16.into(),
                ),
            )
        });
        let tx = native
            .execute_with(|| transaction(order.clone(), public, signature.clone(), &owner, &pair));
        let n = native.execute_with(|| {
            node_subtensor_runtime::api::dispatch("BlockBuilder_apply_extrinsic", &tx).unwrap()
        });
        let blob =
            sc_executor_common::runtime_blob::RuntimeBlob::uncompress_if_needed(&wasm).unwrap();
        let w = executor
            .uncached_call(
                blob,
                &mut interpreted.ext(),
                true,
                "BlockBuilder_apply_extrinsic",
                &tx,
            )
            .unwrap();
        assert_eq!(n, w, "API result mismatch {name}");
        let ne = native.execute_with(System::events);
        let we = interpreted.execute_with(System::events);
        assert_eq!(ne.encode(), we.encode(), "event mismatch {name}");
        let dispatched = ne.iter().find_map(|e| {
            if let RuntimeEvent::Utility(pallet_subtensor_utility::Event::DispatchedAs { result }) =
                &e.event
            {
                Some(*result)
            } else {
                None
            }
        });
        eprintln!("{name}: API={} dispatch={dispatched:?}", hex::encode(&n));
        let result = dispatched.expect("must reach the inner authenticated order dispatch");
        if expected {
            assert!(result.is_ok(), "valid proof failed: {name}: {result:?}");
        } else {
            assert_eq!(
                result,
                Err(pallet_limit_orders::Error::<Runtime>::InvalidSignature.into()),
                "malformed proof did not fail cryptographic verification: {name}"
            );
        }
        let (balance_after, stake_after, order_after) = native.execute_with(|| {
            (
                Balances::free_balance(&owner),
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &owner,
                    &owner,
                    42u16.into(),
                ),
                pallet_limit_orders::Orders::<Runtime>::get(id),
            )
        });
        if expected {
            assert_eq!(
                balance_after,
                balance_before - TaoBalance::new(1_000_000_000)
            );
            assert!(stake_after > stake_before);
            assert_eq!(
                order_after,
                Some(pallet_limit_orders::OrderStatus::Fulfilled)
            );
            assert!(ne.iter().any(|event| matches!(&event.event,
                RuntimeEvent::LimitOrders(pallet_limit_orders::Event::OrderExecuted { order_id, .. }) if *order_id == id)));
        } else {
            assert_eq!(balance_after, balance_before);
            assert_eq!(stake_after, stake_before);
            assert_eq!(order_after, None);
        }
        let root_n = native.execute_with(|| sp_io::storage::root(sp_runtime::StateVersion::V1));
        let root_w =
            interpreted.execute_with(|| sp_io::storage::root(sp_runtime::StateVersion::V1));
        assert_eq!(root_n, root_w, "storage root mismatch {name}");
        results.push(serde_json::json!({"case":name,"expectedValid":expected,"nativeAndWasmAgree":true,"apiResult":hex::encode(n),"dispatchResult":format!("{result:?}"),"storageRoot":hex::encode(root_n),"signatureLength":signature.len(),"ownerBalanceBefore":format!("{balance_before:?}"),"ownerBalanceAfter":format!("{balance_after:?}"),"ownerStakeBefore":format!("{stake_before:?}"),"ownerStakeAfter":format!("{stake_after:?}"),"orderStatus":format!("{order_after:?}")}));
    }
    let report = serde_json::json!({"test":"unchanged release Wasm and matching native runtime ML-DSA order verifier differential","nativeEmbeddedWasmMatches":true,"runtimeVersionBytes":hex::encode(version_native),"specVersion":node_subtensor_runtime::VERSION.spec_version,"wasmSha256":hex::encode(sp_core::hashing::sha2_256(&wasm)),"cases":results,"limitations":["Ephemeral state and existing Sudo.sudo_unchecked_weight -> Utility.dispatch_as intentionally bypass dispatch weight admission only; this is not activation or throughput evidence.","The transaction-specific streaming-mu route remains unreachable before its production Weight::MAX gate; this exercises the shared pure-mode verifier via original LimitOrders.execute_orders.","Internal differential testing does not replace independent cryptographic review."]});
    std::fs::write(
        &args[2],
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap();
}
