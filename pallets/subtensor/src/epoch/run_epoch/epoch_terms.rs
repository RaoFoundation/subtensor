//! Per-hotkey epoch outputs ([`EpochTerms`]) and the [`collect_sorted_epoch_field`] helper macro.

use alloc::collections::BTreeMap;
use sp_std::collections::btree_map::IntoIter;
use sp_std::vec::Vec;
use subtensor_runtime_common::AlphaBalance;

/// Per-uid consensus / emission fields produced by one epoch for a single hotkey.
///
/// `dividend` / `incentive` / `consensus` / `validator_trust` / `stake_weight` are raw `u16`
/// proportions (max-upscaled); persistence wraps them in `PerU16` at the storage boundary.
/// `bond` is the sparse validator→miner bond row as `(uid, u16)` pairs.
#[derive(Debug, Default)]
pub struct EpochTerms {
    pub uid: usize,
    pub dividend: u16,
    pub incentive: u16,
    pub validator_emission: AlphaBalance,
    pub server_emission: AlphaBalance,
    pub stake_weight: u16,
    pub active: bool,
    pub emission: AlphaBalance,
    pub consensus: u16,
    pub validator_trust: u16,
    pub new_validator_permit: bool,
    pub bond: Vec<(u16, u16)>,
    pub stake: AlphaBalance,
}

/// Map of hotkey → [`EpochTerms`] returned by [`super::epoch_mechanism`].
pub struct HotkeyEpochTerms<T: frame_system::Config>(pub BTreeMap<T::AccountId, EpochTerms>);

impl<T: frame_system::Config> HotkeyEpochTerms<T> {
    pub fn as_map(&self) -> &BTreeMap<T::AccountId, EpochTerms> {
        &self.0
    }
}

impl<T> IntoIterator for HotkeyEpochTerms<T>
where
    T: frame_system::Config,
    T::AccountId: Ord,
{
    type Item = (T::AccountId, EpochTerms);
    type IntoIter = IntoIter<T::AccountId, EpochTerms>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Collect one [`EpochTerms`] field from a uid-sorted `&EpochTerms` slice into a parallel `Vec`.
#[macro_export]
macro_rules! collect_sorted_epoch_field {
    ($sorted:expr, $field:ident) => {{
        ($sorted)
            .iter()
            .copied()
            .map(|t| t.$field)
            .collect::<sp_std::vec::Vec<_>>()
    }};
}
