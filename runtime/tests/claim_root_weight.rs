use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use node_subtensor_runtime::{
    BlockWeights, MaxSubtensorTransactionExtensionWeight, Runtime, RuntimeCall, TxExtension,
    check_mortality, check_nonce, sudo_wrapper,
    transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use sp_core::Get;
use sp_runtime::{generic::Era, traits::TransactionExtension};
use std::collections::BTreeSet;
use subtensor_runtime_common::{NetUid, TaoBalance};

#[test]
fn post_dispatch_extensions_refund_before_fee_and_block_reclaim() {
    let identifiers = <TxExtension as TransactionExtension<RuntimeCall>>::metadata()
        .into_iter()
        .map(|metadata| metadata.identifier)
        .collect::<Vec<_>>();

    let Some(subtensor) = identifiers
        .iter()
        .position(|identifier| *identifier == "SubtensorTransactionExtension")
    else {
        panic!("Subtensor transaction extension is configured");
    };
    let Some(payment) = identifiers
        .iter()
        .position(|identifier| *identifier == "ChargeTransactionPayment")
    else {
        panic!("transaction-payment extension is configured");
    };
    let Some(reclaim) = identifiers
        .iter()
        .position(|identifier| *identifier == "WeightReclaim")
    else {
        panic!("final weight-reclaim extension is configured");
    };

    assert!(subtensor < payment);
    assert!(payment < reclaim);
    assert_eq!(reclaim, identifiers.len() - 1);
}

fn checked_add_weight(
    left: frame_support::weights::Weight,
    right: frame_support::weights::Weight,
) -> frame_support::weights::Weight {
    match left.checked_add(&right) {
        Some(weight) => weight,
        None => panic!("dispatch and transaction-extension weights must not overflow"),
    }
}

fn tx_extensions() -> TxExtension {
    (
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
            sudo_wrapper::SudoTransactionExtension::<Runtime>::new(),
            pallet_shield::CheckShieldedTxValidity::<Runtime>::new(),
            pallet_subtensor::SubtensorTransactionExtension::<Runtime>::new(),
            pallet_drand::drand_priority::DrandPriority::<Runtime>::new(),
            ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0)),
        ),
        frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(true),
        frame_system::WeightReclaim::<Runtime>::new(),
    )
}

#[test]
fn claim_root_with_extensions_fits_normal_extrinsic_limit() {
    let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::claim_root {
        subnets: BTreeSet::from([NetUid::from(1)]),
    });
    let extensions = tx_extensions();

    let mut dispatch_info = call.get_dispatch_info();
    dispatch_info.extension_weight =
        checked_add_weight(dispatch_info.extension_weight, extensions.weight(&call));
    let Some(max_extrinsic) = BlockWeights::get().get(DispatchClass::Normal).max_extrinsic else {
        panic!("normal extrinsics must have a configured maximum");
    };

    assert!(
        dispatch_info.total_weight().all_lte(max_extrinsic),
        "claim_root total weight {:?} exceeds normal max extrinsic {max_extrinsic:?}",
        dispatch_info.total_weight()
    );
}

#[test]
fn register_network_with_extensions_fits_normal_extrinsic_limit() {
    let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_network {
        hotkey: sp_runtime::AccountId32::new([1; 32]),
    });
    let extensions = tx_extensions();

    let mut dispatch_info = call.get_dispatch_info();
    dispatch_info.extension_weight =
        checked_add_weight(dispatch_info.extension_weight, extensions.weight(&call));
    let Some(max_extrinsic) = BlockWeights::get().get(DispatchClass::Normal).max_extrinsic else {
        panic!("normal extrinsics must have a configured maximum");
    };

    assert!(
        dispatch_info.total_weight().all_lte(max_extrinsic),
        "register_network total weight {:?} exceeds normal max extrinsic {max_extrinsic:?}",
        dispatch_info.total_weight()
    );
}

#[test]
fn maximum_reserve_covers_call_dependent_subtensor_extensions() {
    let call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_weights {
        netuid: NetUid::from(1),
        dests: vec![0],
        weights: vec![u16::MAX],
        version_key: 0,
    });
    let extensions = tx_extensions();
    let call_extension_weight =
        pallet_subtensor::SubtensorTransactionExtension::<Runtime>::validation_weight(&call);
    let maximum_extension_weight =
        pallet_subtensor::SubtensorTransactionExtension::<Runtime>::maximum_weight();
    let mut dispatch_info = call.get_dispatch_info();
    dispatch_info.extension_weight =
        checked_add_weight(dispatch_info.extension_weight, extensions.weight(&call));
    let Some(max_extrinsic) = BlockWeights::get().get(DispatchClass::Normal).max_extrinsic else {
        panic!("normal extrinsics must have a configured maximum");
    };

    assert!(
        call_extension_weight.all_lte(maximum_extension_weight),
        "set_weights extension weight {call_extension_weight:?} exceeds reserved maximum \
         {maximum_extension_weight:?}"
    );
    assert!(
        dispatch_info.total_weight().all_lte(max_extrinsic),
        "set_weights total weight {:?} exceeds normal max extrinsic {max_extrinsic:?}; reserve {:?}",
        dispatch_info.total_weight(),
        MaxSubtensorTransactionExtensionWeight::get(),
    );
}
