//! Drop the stored loan hotkey from [`Position`](crate::Position) and clear the
//! retired `LoanHotkeys` / `OpenByHotkey` indexes.
//!
//! Hotkeys are now per-call: long opens name a source hotkey, close / repay
//! name a destination hotkey. Nothing on chain is keyed by hotkey any more.
use core::marker::PhantomData;

use alloc::vec::Vec;
use codec::{Decode, DecodeAll, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::ensure;
use frame_support::{
    BoundedVec,
    storage::{migration::clear_storage_prefix, unhashed},
    traits::{OnRuntimeUpgrade, PalletInfoAccess},
    weights::Weight,
};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;

use crate::*;

/// Storage items removed alongside `Position.hotkey`.
pub(crate) const RETIRED_HOTKEY_INDEXES: [&[u8]; 2] = [b"LoanHotkeys", b"OpenByHotkey"];

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_drop_position_hotkey_v1".to_vec())
}

/// Pre-drop [`Position`] layout (`hotkey` after `side`).
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug)]
pub(crate) struct PositionBeforeHotkeyDrop<AccountId, BlockNumber> {
    pub side: Side,
    pub hotkey: AccountId,
    pub custody: AccountId,
    pub principal: u64,
    pub collateral: u64,
    pub proceeds: u64,
    pub escrow_alpha: u64,
    pub lift_tao: u64,
    pub lift_alpha: u64,
    pub annual_interest: u64,
    pub last_accrued: BlockNumber,
    pub interest_remainder: u64,
    pub due: BlockNumber,
    pub expires_at: BlockNumber,
    pub expiry_due: BlockNumber,
}

impl<AccountId, BlockNumber> PositionBeforeHotkeyDrop<AccountId, BlockNumber> {
    /// Drop the hotkey field and seed terms snapshots. Callers that know
    /// `Config` should prefer filling cadence from the runtime constants; the
    /// subsequent [`super::migrate_position_terms_version`] rewrite is a no-op
    /// once the new layout is already present.
    pub(crate) fn into_position<T: Config>(
        self,
    ) -> Position<T::AccountId, BlockNumberFor<T>>
    where
        AccountId: Into<T::AccountId>,
        BlockNumber: Into<BlockNumberFor<T>>,
    {
        Position {
            side: self.side,
            custody: self.custody.into(),
            principal: self.principal,
            collateral: self.collateral,
            proceeds: self.proceeds,
            escrow_alpha: self.escrow_alpha,
            lift_tao: self.lift_tao,
            lift_alpha: self.lift_alpha,
            annual_interest: self.annual_interest,
            last_accrued: self.last_accrued.into(),
            interest_remainder: self.interest_remainder,
            due: self.due.into(),
            expires_at: self.expires_at.into(),
            expiry_due: self.expiry_due.into(),
            terms_version: TERMS_VERSION_V1,
            interest_period: T::InterestPeriod::get(),
            blocks_per_year: T::BlocksPerYear::get(),
        }
    }
}

type PositionOf<T> = Position<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>;
type OldPositionOf<T> =
    PositionBeforeHotkeyDrop<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>;

/// Decode a raw `Positions` value as either layout. The layouts differ by one
/// fixed-width `AccountId`, so `DecodeAll` cannot accept one as the other.
#[cfg(feature = "try-runtime")]
fn decode_either<T: Config>(bytes: &[u8]) -> Option<PositionOf<T>> {
    if let Ok(old) = OldPositionOf::<T>::decode_all(&mut &bytes[..]) {
        return Some(old.into_position::<T>());
    }
    // Post-hotkey-drop / pre-terms rows may still be on chain when this
    // migration is already stamped and the terms migration has not run yet.
    if let Ok(mid) = super::PositionBeforeTermsVersion::<
        <T as frame_system::Config>::AccountId,
        BlockNumberFor<T>,
    >::decode_all(&mut &bytes[..])
    {
        return Some(mid.into_position::<T>());
    }
    PositionOf::<T>::decode_all(&mut &bytes[..]).ok()
}

/// Idempotent: guarded by [`HasMigrationRun`], and each row is rewritten only
/// when it still has the old layout.
///
/// A row that decodes as neither layout is left in place and the guard is not
/// stamped, so a later upgrade can retry rather than orphan its custody funds.
pub fn migrate_drop_position_hotkey<T: Config>() -> Weight {
    let migration_name = migration_key();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        return weight;
    }

    let keys: Vec<_> = Positions::<T>::iter_keys().collect();
    let mut undecodable = 0u32;
    for (owner, netuid) in keys {
        let key = Positions::<T>::hashed_key_for(&owner, netuid);
        weight = weight.saturating_add(T::DbWeight::get().reads(1));
        let Some(bytes) = unhashed::get_raw(&key) else {
            continue;
        };
        if let Ok(old) = OldPositionOf::<T>::decode_all(&mut &bytes[..]) {
            Positions::<T>::insert(&owner, netuid, old.into_position::<T>());
            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        } else if PositionOf::<T>::decode_all(&mut &bytes[..]).is_err() {
            undecodable = undecodable.saturating_add(1);
        }
    }

    let pallet = <Pallet<T> as PalletInfoAccess>::name().as_bytes();
    for item in RETIRED_HOTKEY_INDEXES {
        let removed = clear_storage_prefix(pallet, item, b"", None, None);
        weight = weight.saturating_add(
            T::DbWeight::get().reads_writes(u64::from(removed.loops), u64::from(removed.unique)),
        );
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
        migrate_drop_position_hotkey::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let mut expected = Vec::new();
        for (owner, netuid) in Positions::<T>::iter_keys() {
            let bytes = unhashed::get_raw(&Positions::<T>::hashed_key_for(&owner, netuid))
                .ok_or("Positions key must have a value")?;
            let position = decode_either::<T>(&bytes)
                .ok_or("Positions must decode as pre-drop or current layout")?;
            expected.push((owner, netuid, position));
        }
        Ok(expected.encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let expected: Vec<(T::AccountId, NetUid, PositionOf<T>)> =
            Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "drop-position-hotkey migration must be stamped"
        );
        ensure!(
            Positions::<T>::iter().count() == expected.len(),
            "migration must not add or drop positions"
        );
        for (owner, netuid, position) in expected {
            ensure!(
                Positions::<T>::get(&owner, netuid).as_ref() == Some(&position),
                "migration must preserve every non-hotkey Position field"
            );
        }
        let pallet = <Pallet<T> as PalletInfoAccess>::name().as_bytes();
        for item in RETIRED_HOTKEY_INDEXES {
            let prefix = frame_support::storage::storage_prefix(pallet, item);
            ensure!(
                !unhashed::contains_prefixed_key(&prefix),
                "retired hotkey index must be empty"
            );
        }
        Ok(())
    }
}
