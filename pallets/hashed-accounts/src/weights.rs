//! Unmeasured fail-closed weights. Replace using reference-hardware benchmarks
//! before production activation; these are intentionally not operating estimates.
use frame_support::weights::Weight;

pub trait WeightInfo {
    fn register() -> Weight;
    fn check_registered() -> Weight;
    fn authorize(call_len: u32) -> Weight;
    fn authorize_mldsa(call_len: u32) -> Weight;
    fn authorize_ed25519(call_len: u32) -> Weight;
}

pub struct Uncalibrated;

impl WeightInfo for Uncalibrated {
    fn authorize_ed25519(_: u32) -> Weight {
        Weight::MAX
    }
    fn authorize_mldsa(_: u32) -> Weight {
        Weight::MAX
    }
    fn check_registered() -> Weight {
        Weight::MAX
    }
    fn register() -> Weight {
        Weight::MAX
    }
    fn authorize(_: u32) -> Weight {
        Weight::MAX
    }
}

impl WeightInfo for () {
    fn authorize_ed25519(call_len: u32) -> Weight {
        Uncalibrated::authorize_ed25519(call_len)
    }
    fn authorize_mldsa(call_len: u32) -> Weight {
        Uncalibrated::authorize_mldsa(call_len)
    }
    fn check_registered() -> Weight {
        Uncalibrated::check_registered()
    }
    fn register() -> Weight {
        Uncalibrated::register()
    }
    fn authorize(call_len: u32) -> Weight {
        Uncalibrated::authorize(call_len)
    }
}
