//! Dispatch and hook weights. Reference measurements supply the runtime implementation.
use frame_support::weights::Weight;

pub trait WeightInfo {
    fn open() -> Weight;
    fn close() -> Weight;
    fn set_enabled() -> Weight;
    fn collect() -> Weight;
    fn update_reference() -> Weight;
    fn settle() -> Weight;
}

/// The mock runtime has no execution budget. Production must provide measured weights.
impl WeightInfo for () {
    fn open() -> Weight {
        Weight::zero()
    }
    fn close() -> Weight {
        Weight::zero()
    }
    fn set_enabled() -> Weight {
        Weight::zero()
    }
    fn collect() -> Weight {
        Weight::zero()
    }
    fn update_reference() -> Weight {
        Weight::zero()
    }
    fn settle() -> Weight {
        Weight::zero()
    }
}
