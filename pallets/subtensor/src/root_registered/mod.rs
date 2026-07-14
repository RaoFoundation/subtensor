use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{pallet_prelude::Parameter, weights::Weight};
use scale_info::TypeInfo;
use substrate_fixed::types::U64F64;

pub mod ema;
pub mod ref_count;
#[cfg(any(feature = "try-runtime", test))]
pub mod try_state;

/// Per-coldkey EMA state.
#[freeze_struct("f4bb10f7c2fb2cc1")]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct EmaState {
    /// Current EMA value.
    pub ema: U64F64,
    /// Samples folded in so far.
    pub samples: u32,
}

/// In-flight EMA sample for the coldkey at the current cursor.
/// The provider owns the inner progress shape; the root-registered EMA
/// engine only ties it to the coldkey being sampled.
#[freeze_struct("f9307bf115ed1bae")]
#[derive(
    Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo,
)]
pub struct InFlightEmaSample<AccountId, Progress> {
    /// Coldkey whose sample is in progress. Used to discard stale
    /// progress if the cursor moves or the account leaves mid-sample.
    pub coldkey: AccountId,
    /// Provider-owned progress for the current sample.
    pub progress: Progress,
}

/// Result of one provider sampling step.
pub enum SampleStep<Progress> {
    /// More work remains for this coldkey; persist `progress` and resume
    /// on a later tick.
    Continue { progress: Progress },
    /// The current sample is complete and ready to be folded into the EMA.
    Complete { sample: U64F64 },
}

/// Provides the raw sample value over which the root-registered EMA is
/// computed. The EMA engine owns blending and sample counters; providers
/// only own how to incrementally measure one current value.
pub trait EmaValueProvider<AccountId> {
    /// Opaque in-flight progress for a single sample.
    type Progress: Parameter + MaxEncodedLen + Default;

    /// Process one chunk of work for `coldkey`.
    fn step(coldkey: &AccountId, progress: Self::Progress) -> (SampleStep<Self::Progress>, Weight);

    /// Worst-case weight of `step`.
    fn step_weight() -> Weight;
}

/// Zero-valued provider for runtimes / test mocks that do not compute EMAs.
impl<AccountId> EmaValueProvider<AccountId> for () {
    type Progress = ();

    fn step(_: &AccountId, _: Self::Progress) -> (SampleStep<Self::Progress>, Weight) {
        let sample = U64F64::saturating_from_num(0u64);
        (SampleStep::Complete { sample }, Weight::zero())
    }

    fn step_weight() -> Weight {
        Weight::zero()
    }
}

/// Hook for coldkey root-registration transitions. Callers accrue
/// `on_added_weight` / `on_removed_weight` when a 0↔1 transition is
/// possible.
pub trait OnRootRegistrationChange<AccountId> {
    /// Called when `coldkey` enters the root-registered set.
    fn on_added(coldkey: &AccountId);
    /// Called when `coldkey` leaves the root-registered set.
    fn on_removed(coldkey: &AccountId);
    /// Worst-case weight of `on_added`.
    fn on_added_weight() -> Weight;
    /// Worst-case weight of `on_removed`.
    fn on_removed_weight() -> Weight;
}

impl<AccountId> OnRootRegistrationChange<AccountId> for () {
    fn on_added(_: &AccountId) {}
    fn on_removed(_: &AccountId) {}
    fn on_added_weight() -> Weight {
        Weight::zero()
    }
    fn on_removed_weight() -> Weight {
        Weight::zero()
    }
}

/// Snapshot of the root-registered coldkey set.
pub trait RootRegisteredInspector<AccountId> {
    /// Returns the current snapshot, or `None` if unavailable.
    fn members() -> Option<alloc::vec::Vec<AccountId>>;
}

impl<AccountId> RootRegisteredInspector<AccountId> for () {
    fn members() -> Option<alloc::vec::Vec<AccountId>> {
        None
    }
}
