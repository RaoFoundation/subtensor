//! Benchmarking setup for pallet-subtensor-swap
#![allow(clippy::unwrap_used)]
#![allow(clippy::multiple_bound_locations)]

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use subtensor_runtime_common::NetUid;

use crate::pallet::{Call, Config, Pallet};

#[benchmarks(where T: Config)]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn set_fee_rate() {
        let netuid = NetUid::from(1);
        let rate: u16 = 100;

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, rate);
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
