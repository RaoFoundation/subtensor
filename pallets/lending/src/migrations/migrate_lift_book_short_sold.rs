//! Append `short_sold_alpha` (default 0) to stored [`LiftBook`](crate::LiftBook).
//!
//! Pre-v5.8 layouts decode as [`LiftBookBeforeShortSold`]. Empty storage is left
//! empty (`ValueQuery` still supplies [`Default`]). No mainnet positions exist
//! at the upgrade, so rebuilding from `Positions` is unnecessary — zero is
//! correct, and subsequent opens track the aggregate.
use core::marker::PhantomData;

use alloc::vec::Vec;
use codec::{Decode, DecodeAll, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::ensure;
use frame_support::{BoundedVec, storage::unhashed, traits::OnRuntimeUpgrade, weights::Weight};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

use crate::*;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_lift_book_short_sold_v1".to_vec())
}

/// Pre-v5.8 [`LiftBook`] layout (no `short_sold_alpha`).
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, Default)]
pub struct LiftBookBeforeShortSold {
    pub l_s: u64,
    pub l_l: u64,
    pub b_l: u64,
    pub held_tao: u64,
    pub held_alpha: u64,
    pub escrow_tao: u64,
    pub escrow_alpha: u64,
    pub long_collateral_alpha: u64,
    pub g_ema: u64,
    pub last: u64,
}

impl LiftBookBeforeShortSold {
    fn into_current(self) -> LiftBook {
        LiftBook {
            l_s: self.l_s,
            l_l: self.l_l,
            b_l: self.b_l,
            held_tao: self.held_tao,
            held_alpha: self.held_alpha,
            escrow_tao: self.escrow_tao,
            escrow_alpha: self.escrow_alpha,
            long_collateral_alpha: self.long_collateral_alpha,
            g_ema: self.g_ema,
            last: self.last,
            short_sold_alpha: 0,
        }
    }
}

/// Idempotent: guarded by [`HasMigrationRun`], and each row is rewritten only
/// when it still has the pre-short-sold layout.
pub fn migrate_lift_book_short_sold<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    let keys: Vec<_> = Lifts::<T>::iter_keys().collect();
    let mut undecodable = 0u32;
    for netuid in keys {
        let key = Lifts::<T>::hashed_key_for(netuid);
        weight = weight.saturating_add(T::DbWeight::get().reads(1));
        let Some(bytes) = unhashed::get_raw(&key) else {
            continue;
        };
        if let Ok(old) = LiftBookBeforeShortSold::decode_all(&mut &bytes[..]) {
            Lifts::<T>::insert(netuid, old.into_current());
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        } else if LiftBook::decode_all(&mut &bytes[..]).is_err() {
            undecodable = undecodable.saturating_add(1);
        }
    }

    if undecodable == 0 {
        HasMigrationRun::<T>::insert(&migration_name, true);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }
    weight
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_lift_book_short_sold::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let mut expected = Vec::new();
        for netuid in Lifts::<T>::iter_keys() {
            let bytes = unhashed::get_raw(&Lifts::<T>::hashed_key_for(netuid))
                .ok_or("Lifts key must have a value")?;
            let book = if let Ok(old) = LiftBookBeforeShortSold::decode_all(&mut &bytes[..]) {
                old.into_current()
            } else if let Ok(cur) = LiftBook::decode_all(&mut &bytes[..]) {
                cur
            } else {
                return Err("Lifts must decode as pre-short-sold or current layout".into());
            };
            expected.push((netuid, book));
        }
        Ok(expected.encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let expected: Vec<(NetUid, LiftBook)> =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "lift-book-short-sold migration must be stamped"
        );
        for (netuid, want) in expected {
            let got = Lifts::<T>::get(netuid);
            ensure!(got == want, "LiftBook fields must be preserved");
        }
        Ok(())
    }
}
