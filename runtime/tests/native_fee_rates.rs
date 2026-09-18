//! Pin the spec 467 native fee rates with hand arithmetic, independent of `compute_fee`.

#![allow(clippy::expect_used)]

use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use frame_support::weights::{Weight, WeightToFee};
use node_subtensor_runtime::{
    BlockWeights, BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System,
    TransactionPayment,
};
use subtensor_runtime_common::{AccountId, TaoBalance};
use subtensor_transaction_fee::{LinearLengthToFee, LinearWeightToFee};

/// Rao per ref_time unit, in parts per billion (`LinearWeightToFee`).
const WEIGHT_FEE_PPB: u128 = 250_000;
/// Rao per encoded byte, in parts per billion (`LinearLengthToFee`).
const LENGTH_FEE_PPB: u128 = 500_000_000;
const PPB: u128 = 1_000_000_000;

/// `Perbill * N` rounds to the nearest integer and breaks ties downward.
fn nearest_rao(units: u128, ppb: u128) -> u64 {
    let scaled = units.saturating_mul(ppb);
    let quotient = scaled / PPB;
    let remainder = scaled % PPB;
    let round_up = u128::from(remainder.saturating_mul(2) > PPB);
    u64::try_from(quotient.saturating_add(round_up)).expect("fee fits u64")
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis storage builds")
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

#[test]
fn weight_to_fee_charges_a_quarter_rao_per_thousand_ref_time() {
    let fee =
        |ref_time: u64| LinearWeightToFee::weight_to_fee(&Weight::from_parts(ref_time, u64::MAX));
    assert_eq!(fee(0), TaoBalance::new(0));
    // One EVM gas maps to 40_000 ref_time: 10 rao, the same as the 10 gwei default gas price.
    assert_eq!(fee(40_000), TaoBalance::new(10));
    assert_eq!(fee(1_000_000_000), TaoBalance::new(250_000));
    // `transfer_keep_alive` call weight on 467: 56_012.75 rounds to nearest.
    assert_eq!(fee(224_051_000), TaoBalance::new(56_013));
    assert_eq!(fee(u64::MAX), TaoBalance::new(4_611_686_018_427_388));
    // proof_size never prices.
    assert_eq!(
        LinearWeightToFee::weight_to_fee(&Weight::from_parts(0, u64::MAX)),
        TaoBalance::new(0)
    );
}

#[test]
fn length_to_fee_charges_half_a_rao_per_byte() {
    let fee = |len: u64| LinearLengthToFee::weight_to_fee(&Weight::from_parts(len, 0));
    assert_eq!(fee(0), TaoBalance::new(0));
    // Half a rao is a tie and rounds down.
    assert_eq!(fee(1), TaoBalance::new(0));
    assert_eq!(fee(3), TaoBalance::new(1));
    assert_eq!(fee(150), TaoBalance::new(75));
    assert_eq!(fee(151), TaoBalance::new(75));
    assert_eq!(fee(u64::MAX), TaoBalance::new(u64::MAX / 2));
}

#[test]
fn plain_transfer_fee_matches_hand_arithmetic() {
    new_test_ext().execute_with(|| {
        let call = RuntimeCall::Balances(pallet_balances::Call::<Runtime>::transfer_keep_alive {
            dest: AccountId::from([2_u8; 32]).into(),
            value: TaoBalance::new(1_000_000_000),
        });
        let info = call.get_dispatch_info();
        let len: u32 = 147;

        let base_extrinsic = BlockWeights::get()
            .get(DispatchClass::Normal)
            .base_extrinsic
            .ref_time();
        let expected = nearest_rao(u128::from(base_extrinsic), WEIGHT_FEE_PPB)
            .saturating_add(nearest_rao(u128::from(len), LENGTH_FEE_PPB))
            .saturating_add(nearest_rao(
                u128::from(info.total_weight().ref_time()),
                WEIGHT_FEE_PPB,
            ));

        let quoted = TransactionPayment::compute_fee(len, &info, TaoBalance::new(0));
        assert_eq!(quoted, TaoBalance::new(expected));
        // Roughly 0.00008 TAO: half the 0.000166 TAO a plain transfer cost on 466.
        assert!((70_000..=100_000).contains(&expected), "{expected} rao");
    });
}
