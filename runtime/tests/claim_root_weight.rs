#![allow(clippy::expect_used, clippy::panic)]

use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use node_subtensor_runtime::{
    BlockWeights, BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System, TxExtension,
    check_mortality, check_nonce, sudo_wrapper,
    transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use sp_runtime::{generic::Era, traits::TransactionExtension};
use sp_std::collections::btree_set::BTreeSet;
use subtensor_runtime_common::{AccountId, NetUid, TaoBalance};

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis storage builds")
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn expected_root_claim_weight(limit: u32) -> frame_support::weights::Weight {
    use pallet_subtensor::weights::WeightInfo;

    pallet_subtensor::weights::SubstrateWeight::<Runtime>::claim_root(limit)
        .saturating_add(
            pallet_subtensor::weights::SubstrateWeight::<Runtime>::claim_root_scan(limit),
        )
        // FRAME folds the runtime's dispatch-extension weight into `call_weight`.
        .saturating_add(
            pallet_subtensor::weights::SubstrateWeight::<Runtime>::check_coldkey_swap_extension(),
        )
}

fn assert_call_fits_normal_limit(call: RuntimeCall) {
    let extensions: TxExtension = (
        (
            frame_system::CheckNonZeroSender::<Runtime>::new(),
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            check_mortality::CheckMortality::<Runtime>::from(Era::Immortal),
            check_nonce::CheckNonce::<Runtime>::from(0),
            frame_system::CheckWeight::<Runtime>::new(),
        ),
        (
            ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0)),
            sudo_wrapper::SudoTransactionExtension::<Runtime>::new(),
            pallet_shield::CheckShieldedTxValidity::<Runtime>::new(),
            pallet_subtensor::SubtensorTransactionExtension::<Runtime>::new(),
            pallet_drand::drand_priority::DrandPriority::<Runtime>::new(),
        ),
        frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(true),
    );

    let mut dispatch_info = call.get_dispatch_info();
    dispatch_info.extension_weight = extensions.weight(&call);
    let Some(max_extrinsic) = BlockWeights::get().get(DispatchClass::Normal).max_extrinsic else {
        panic!("normal extrinsics have a configured maximum");
    };

    assert!(
        dispatch_info.total_weight().all_lte(max_extrinsic),
        "call total weight {:?} exceeds normal max extrinsic {max_extrinsic:?}",
        dispatch_info.total_weight()
    );
}

#[test]
fn claim_root_with_extensions_fits_normal_extrinsic_limit() {
    new_test_ext().execute_with(|| {
        let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::claim_root {
            subnets: BTreeSet::from([NetUid::ROOT]),
        });
        assert_eq!(
            call.get_dispatch_info().call_weight,
            expected_root_claim_weight(pallet_subtensor::MAX_ROOT_CLAIM_WORK)
        );
        assert_call_fits_normal_limit(call);
    });
}

#[test]
fn claim_root_with_hotkey_with_extensions_fits_normal_extrinsic_limit() {
    new_test_ext().execute_with(|| {
        let hotkey = AccountId::new([1u8; 32]);
        let call =
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::claim_root_with_hotkey { hotkey });
        assert_eq!(
            call.get_dispatch_info().call_weight,
            expected_root_claim_weight(pallet_subtensor::MAX_ROOT_CLAIM_WORK)
        );
        assert_call_fits_normal_limit(call);
    });
}
