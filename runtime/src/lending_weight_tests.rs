//! Integration budgets for the bounded lending work and the initial 128-subnet rollout.
use super::{
    BlockWeights, LendingMaxFundedSubnets, LendingWeights, MAXIMUM_BLOCK_WEIGHT, Runtime,
    SwapCurveInitializationWeight,
};
use frame_support::traits::Get;
use pallet_lending::weights::WeightInfo as LendingWeightInfo;
use pallet_subtensor::weights::WeightInfo as SubtensorWeightInfo;

#[test]
fn lending_close_and_hooks_fit_runtime_block_budget() {
    // The Subtensor hook charges the larger component from either consensus mode.
    let null_weight =
        <pallet_subtensor::weights::SubstrateWeight<Runtime> as SubtensorWeightInfo>::block_step();
    let yuma_weight =
        <pallet_subtensor::weights::SubstrateWeight<Runtime> as SubtensorWeightInfo>::block_step_yuma();
    let block_step_weight = frame_support::weights::Weight::from_parts(
        null_weight.ref_time().max(yuma_weight.ref_time()),
        null_weight.proof_size().max(yuma_weight.proof_size()),
    );
    let hooks = LendingWeights::update_reference()
        .saturating_mul(u64::from(LendingMaxFundedSubnets::get()))
        .saturating_add(block_step_weight)
        .saturating_add(<pallet_subtensor::weights::SubstrateWeight<Runtime> as SubtensorWeightInfo>::fund_lending_reserves());
    let close = LendingWeights::close();
    let open = LendingWeights::open();
    let tune = LendingWeights::set_min_price_impact();
    assert!(hooks.saturating_add(close).all_lte(MAXIMUM_BLOCK_WEIGHT));
    assert!(hooks.saturating_add(open).all_lte(MAXIMUM_BLOCK_WEIGHT));
    assert!(hooks.saturating_add(tune).all_lte(MAXIMUM_BLOCK_WEIGHT));
    let Some(normal) = BlockWeights::get()
        .get(frame_support::dispatch::DispatchClass::Normal)
        .max_extrinsic
    else {
        panic!("normal dispatch needs an extrinsic budget");
    };
    assert!(close.all_lte(normal));
    assert!(open.all_lte(normal));
    assert!(tune.all_lte(normal));
}

#[test]
fn initial_pool_conversion_and_funding_fit_upgrade_budget() {
    // The initial mainnet deployment has 128 dynamic subnet slots. This is a
    // composed reference envelope, not a local benchmark measurement.
    let per_pool = SwapCurveInitializationWeight::get()
        .saturating_add(<pallet_subtensor::weights::SubstrateWeight<Runtime> as SubtensorWeightInfo>::fund_lending_reserves())
        .saturating_add(<Runtime as frame_system::Config>::DbWeight::get().reads_writes(12, 4));
    assert!(per_pool.saturating_mul(128).all_lte(MAXIMUM_BLOCK_WEIGHT));
}
