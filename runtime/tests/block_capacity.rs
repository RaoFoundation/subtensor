#![allow(clippy::expect_used)]

use codec::Encode;
use frame_support::{
    assert_ok,
    dispatch::{DispatchClass, DispatchInfo, GetDispatchInfo},
    weights::{Weight, constants::WEIGHT_REF_TIME_PER_SECOND},
};
use node_subtensor_runtime::{
    BlockGasLimit, BlockWeights, BuildStorage, MaximumSchedulerWeight, Runtime, RuntimeCall,
    RuntimeGenesisConfig, System, TransactionPayment, TxExtension, WeightPerGas, check_mortality,
    check_nonce, check_nonzero_sender, sudo_wrapper,
    transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use pallet_evm::GasWeightMapping;
use sp_core::U256;
use sp_runtime::{Perbill, generic::Era, traits::TransactionExtension};
use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, TaoBalance};

fn previous_block_weight() -> Weight {
    Weight::from_parts(4_000_000_000_000, u64::MAX)
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis storage builds")
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn dispatch_info_with_extensions(call: &RuntimeCall) -> DispatchInfo {
    let extensions: TxExtension = (
        (
            check_nonzero_sender::CheckNonZeroSender::<Runtime>::new(),
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
    DispatchInfo {
        extension_weight: extensions.weight(call),
        ..call.get_dispatch_info()
    }
}

#[test]
fn sixty_five_stake_transfers_fit_without_changing_the_batch() {
    new_test_ext().execute_with(|| {
        let transfers: Vec<_> = (1_u8..=65)
            .map(|recipient| {
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake {
                    destination_coldkey: AccountId::from([recipient; 32]),
                    hotkey: AccountId::from([100; 32]),
                    origin_netuid: NetUid::ROOT,
                    destination_netuid: NetUid::ROOT,
                    alpha_amount: AlphaBalance::new(1_000_000_000),
                })
            })
            .collect();
        let previous = frame_system::limits::BlockWeights::with_sensible_defaults(
            previous_block_weight(),
            Perbill::from_percent(75),
        );
        let previous_limit = previous
            .get(DispatchClass::Normal)
            .max_extrinsic
            .expect("normal extrinsics have a maximum");
        for call in [
            RuntimeCall::Utility(pallet_subtensor_utility::Call::batch {
                calls: transfers.clone(),
            }),
            RuntimeCall::Utility(pallet_subtensor_utility::Call::batch_all {
                calls: transfers.clone(),
            }),
            RuntimeCall::Utility(pallet_subtensor_utility::Call::force_batch { calls: transfers }),
        ] {
            let info = dispatch_info_with_extensions(&call);
            assert!(info.total_weight().ref_time() > previous_limit.ref_time());
            let new_limit = BlockWeights::get()
                .get(DispatchClass::Normal)
                .max_extrinsic
                .expect("normal extrinsics have a maximum");
            assert!(info.total_weight().all_lte(new_limit));
            assert_ok!(frame_system::CheckWeight::<Runtime>::do_validate(
                &info,
                call.encoded_size()
            ));
        }
    });
}

#[test]
fn larger_blocks_preserve_evm_execution_pricing_and_scheduler_budget() {
    new_test_ext().execute_with(|| {
        assert_eq!(BlockWeights::get().max_block.ref_time(), 12_000_000_000_000);
        assert_eq!(
            MaximumSchedulerWeight::get(),
            Perbill::from_percent(80) * previous_block_weight(),
        );
        assert_eq!(BlockGasLimit::get(), U256::from(225_000_000_u64));
        assert_eq!(
            WeightPerGas::get().ref_time(),
            (Perbill::from_percent(75) * previous_block_weight())
                .checked_div(75_000_000)
                .expect("nonzero gas limit")
                .ref_time(),
        );
        assert_eq!(
            WeightPerGas::get().proof_size(),
            (Perbill::from_percent(75) * previous_block_weight())
                .proof_size()
                .checked_div(225_000_000)
                .expect("nonzero gas limit"),
        );
    });
}

#[test]
fn evm_transaction_above_previous_gas_limit_fits_both_weight_dimensions() {
    new_test_ext().execute_with(|| {
        // A representative large payout needs more than the previous 75M ceiling.
        // This checks admission only, not the execution of the actual payout.
        let weight =
            <Runtime as pallet_evm::Config>::GasWeightMapping::gas_to_weight(150_000_000, true);
        let info = DispatchInfo {
            call_weight: weight,
            ..Default::default()
        };
        assert_ok!(frame_system::CheckWeight::<Runtime>::do_validate(
            &info, 1_000
        ));
        let limit = BlockWeights::get()
            .get(DispatchClass::Normal)
            .max_extrinsic
            .expect("normal extrinsics have a maximum");
        assert!(weight.all_lte(limit));

        // The block gas ceiling is not a promise that one transaction can consume
        // the entire block: initialization and operational reserves still apply.
        let entire_block =
            <Runtime as pallet_evm::Config>::GasWeightMapping::gas_to_weight(225_000_000, true);
        assert!(entire_block.ref_time() > limit.ref_time());
    });
}

#[test]
fn larger_block_capacity_does_not_raise_the_weight_fee_rate() {
    new_test_ext().execute_with(|| {
        assert_eq!(
            TransactionPayment::weight_to_fee(Weight::from_parts(WEIGHT_REF_TIME_PER_SECOND, 0)),
            TaoBalance::new(500_000_000),
        );
    });
}
