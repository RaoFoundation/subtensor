//! Unmeasured fail-closed weights. Replace using reference-hardware benchmarks
//! before production activation; these are intentionally not operating estimates.
use frame_support::weights::Weight;

pub trait WeightInfo {
    fn register() -> Weight;
    fn authorize(call_len: u32) -> Weight;
}

pub struct Uncalibrated;

impl WeightInfo for Uncalibrated {
    fn register() -> Weight {
        Weight::MAX
    }
    fn authorize(_: u32) -> Weight {
        Weight::MAX
    }
}

impl WeightInfo for () {
    fn register() -> Weight {
        Uncalibrated::register()
    }
    fn authorize(call_len: u32) -> Weight {
        Uncalibrated::authorize(call_len)
    }
}
