use frame_support::dispatch::DispatchClass;
use node_subtensor_runtime::{
    BlockWeights, MAXIMUM_BLOCK_WEIGHT, Runtime, WEIGHT_REF_TIME_PER_SECOND,
};
use pallet_subtensor::weights::WeightInfo;
use subtensor_runtime_common::time::MILLISECS_PER_BLOCK;

#[test]
fn global_root_claim_limit_fits_the_production_block_budget() {
    assert_eq!(pallet_subtensor::MAX_ROOT_CLAIM_WORK, 1_000);
    assert_eq!(MILLISECS_PER_BLOCK, 12_000);
    assert_eq!(
        MAXIMUM_BLOCK_WEIGHT.ref_time(),
        4 * WEIGHT_REF_TIME_PER_SECOND
    );

    let limit = pallet_subtensor::MAX_ROOT_CLAIM_WORK;
    let declared = pallet_subtensor::weights::SubstrateWeight::<Runtime>::claim_root(limit)
        .saturating_add(
            pallet_subtensor::weights::SubstrateWeight::<Runtime>::claim_root_scan(limit),
        );
    let max_extrinsic = BlockWeights::get()
        .get(DispatchClass::Normal)
        .max_extrinsic
        .expect("normal extrinsics have a configured maximum");
    assert!(
        declared.all_lte(max_extrinsic),
        "1,000-unit claim {declared:?} exceeds normal max {max_extrinsic:?}"
    );
    assert!(
        declared.saturating_mul(2).all_lte(max_extrinsic),
        "1,000-unit claim {declared:?} has less than 2x headroom below normal max {max_extrinsic:?}"
    );
}
