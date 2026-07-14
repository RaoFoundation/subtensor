use alloc::vec::Vec;
use frame_support::weights::Weight;
use substrate_fixed::types::U64F64;

use super::*;
use crate::root_registered::{EmaState, EmaValueProvider, InFlightEmaSample, SampleStep};

/// EMA mixing constant numerator (alpha = 2/100 = 0.02).
const EMA_ALPHA_NUM: u64 = 2;
const EMA_ALPHA_DEN: u64 = 100;

impl<T: Config> Pallet<T> {
    /// Advances the root-registered EMA sampler by one provider step.
    pub fn tick_root_registered_ema() -> Weight {
        let (sample, mut weight) = Self::load_current_sample();
        let Some((cursor, coldkey, in_flight)) = sample else {
            return weight;
        };

        let has_ema = RootRegisteredEma::<T>::contains_key(&coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        if !has_ema {
            return weight.saturating_add(Self::skip_missing_sample(cursor));
        }

        let progress = Self::resume_progress(&coldkey, in_flight);

        let (step, step_weight) = T::EmaValueProvider::step(&coldkey, progress);
        weight.saturating_accrue(step_weight);

        weight.saturating_add(match step {
            SampleStep::Continue { progress } => Self::store_progress(cursor, coldkey, progress),
            SampleStep::Complete { sample } => Self::complete_sample(cursor, coldkey, sample),
        })
    }

    fn load_current_sample() -> (
        Option<(u32, T::AccountId, Option<InFlightEmaSampleOf<T>>)>,
        Weight,
    ) {
        let db = T::DbWeight::get();
        let (mut cursor, mut in_flight) = EmaSamplerState::<T>::get();
        let mut members = CurrentCycleMembers::<T>::get();
        let mut weight = db.reads(2);

        // Cursor wrap starts a new fixed snapshot. Keeping the snapshot
        // stable avoids mid-cycle joins reshuffling the round-robin order.
        if (cursor as usize) >= members.len() {
            let collected: Vec<T::AccountId> =
                RootRegisteredEma::<T>::iter().map(|(k, _)| k).collect();
            weight.saturating_accrue(db.reads(collected.len() as u64));

            members = BoundedVec::try_from(collected).unwrap_or_default();
            cursor = 0;
            in_flight = None;

            CurrentCycleMembers::<T>::put(&members);
            EmaSamplerState::<T>::put((cursor, None::<InFlightEmaSampleOf<T>>));
            weight.saturating_accrue(db.writes(2));
        }

        let sample = members
            .get(cursor as usize)
            .map(|coldkey| (cursor, coldkey.clone(), in_flight));
        (sample, weight)
    }

    fn resume_progress(
        coldkey: &T::AccountId,
        in_flight: Option<InFlightEmaSampleOf<T>>,
    ) -> <T::EmaValueProvider as EmaValueProvider<T::AccountId>>::Progress {
        // Progress is only reusable for the exact coldkey at the current
        // cursor. Otherwise start a fresh provider sample.
        match in_flight {
            Some(p) if &p.coldkey == coldkey => p.progress,
            _ => <T::EmaValueProvider as EmaValueProvider<T::AccountId>>::Progress::default(),
        }
    }

    fn skip_missing_sample(cursor: u32) -> Weight {
        // A coldkey can disappear from storage while it is still present
        // in the fixed cycle snapshot. Skip it and let the next cycle
        // rebuild without it.
        EmaSamplerState::<T>::put((cursor.saturating_add(1), None::<InFlightEmaSampleOf<T>>));
        T::DbWeight::get().writes(1)
    }

    fn store_progress(
        cursor: u32,
        coldkey: T::AccountId,
        progress: <T::EmaValueProvider as EmaValueProvider<T::AccountId>>::Progress,
    ) -> Weight {
        EmaSamplerState::<T>::put((cursor, Some(InFlightEmaSample { coldkey, progress })));
        T::DbWeight::get().writes(1)
    }

    fn complete_sample(cursor: u32, coldkey: T::AccountId, sample: U64F64) -> Weight {
        RootRegisteredEma::<T>::mutate(&coldkey, |state| {
            *state = EmaState {
                ema: blend(sample, *state),
                samples: state.samples.saturating_add(1),
            };
        });
        EmaSamplerState::<T>::put((cursor.saturating_add(1), None::<InFlightEmaSampleOf<T>>));
        T::DbWeight::get().reads_writes(1, 2)
    }

    /// Seeds a fresh EMA slot at zero. The zero value enforces a
    /// warmup window before the EMA carries meaningful weight.
    pub(crate) fn init_root_registered_ema(coldkey: &T::AccountId) {
        RootRegisteredEma::<T>::insert(coldkey, EmaState::default());
    }

    pub(crate) fn clear_root_registered_ema(coldkey: &T::AccountId) {
        RootRegisteredEma::<T>::remove(coldkey);
        EmaSamplerState::<T>::mutate(|(_, progress)| {
            if progress
                .as_ref()
                .is_some_and(|in_flight| &in_flight.coldkey == coldkey)
            {
                *progress = None;
            }
        });
    }
}

fn blend(sample: U64F64, previous: EmaState) -> U64F64 {
    let alpha = U64F64::saturating_from_num(EMA_ALPHA_NUM)
        .saturating_div(U64F64::saturating_from_num(EMA_ALPHA_DEN));
    let one_minus_alpha = U64F64::saturating_from_num(1).saturating_sub(alpha);
    alpha
        .saturating_mul(sample)
        .saturating_add(one_minus_alpha.saturating_mul(previous.ema))
}
