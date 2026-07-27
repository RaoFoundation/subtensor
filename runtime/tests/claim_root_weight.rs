use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use node_subtensor_runtime::{
    BlockWeights, MaxSubtensorTransactionExtensionWeight, Runtime, RuntimeCall, TxExtension,
    check_mortality, check_nonce, sudo_wrapper,
    transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use sp_core::Get;
use sp_runtime::{AccountId32, generic::Era, traits::TransactionExtension};
use std::collections::BTreeSet;
use subtensor_runtime_common::{NetUid, TaoBalance};

#[test]
fn claim_root_with_extensions_fits_normal_extrinsic_limit() {
    let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::claim_root {
        subnets: BTreeSet::from([NetUid::from(1)]),
    });
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
    let max_extrinsic = BlockWeights::get()
        .get(DispatchClass::Normal)
        .max_extrinsic
        .expect("normal extrinsics have a configured maximum");

    assert!(
        dispatch_info.total_weight().all_lte(max_extrinsic),
        "claim_root total weight {:?} exceeds normal max extrinsic {max_extrinsic:?}",
        dispatch_info.total_weight()
    );
}

#[test]
fn unbounded_hotkey_swap_reserves_maximum_admissible_call_weight() {
    let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey {
        hotkey: AccountId32::new([1; 32]),
        new_hotkey: AccountId32::new([2; 32]),
        netuid: None,
    });
    let dispatch_info = call.get_dispatch_info();
    let enclosing_extension_weight = MaxSubtensorTransactionExtensionWeight::get();
    let dispatch_extension_weight =
        pallet_subtensor::SubtensorTransactionExtension::<Runtime>::new().weight(&call);
    let transaction_extension_weight = enclosing_extension_weight
        .checked_sub(&dispatch_extension_weight)
        .expect("dispatch-extension weight must fit within the enclosing extension weight");
    let max_extrinsic = BlockWeights::get()
        .get(DispatchClass::Normal)
        .max_extrinsic
        .expect("normal extrinsics have a configured maximum");

    assert_eq!(
        dispatch_info
            .call_weight
            .checked_add(&transaction_extension_weight)
            .expect("call and transaction-extension weights must not overflow"),
        max_extrinsic
    );
}
