//! Append [`Position`](crate::Position) terms fields for versioned loan rules.
//!
//! Existing rows (post-hotkey-drop layout) receive `terms_version = 1` and the
//! current [`Config::InterestPeriod`] / [`Config::BlocksPerYear`] snapshots so
//! live loans finish under today's rules. New opens read
//! [`CurrentTermsVersion`](crate::CurrentTermsVersion).
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
    BoundedVec::truncate_from(b"migrate_position_terms_version_v1".to_vec())
}

/// Pre-terms [`Position`] layout (no `terms_version` / cadence snapshots).
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug)]
pub(crate) struct PositionBeforeTermsVersion<AccountId, BlockNumber> {
    pub side: Side,
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

impl<AccountId, BlockNumber> PositionBeforeTermsVersion<AccountId, BlockNumber> {
    fn into_position<T: Config>(self) -> Position<T::AccountId, BlockNumberFor<T>>
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
    PositionBeforeTermsVersion<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>;

#[cfg(feature = "try-runtime")]
fn decode_either<T: Config>(bytes: &[u8]) -> Option<PositionOf<T>> {
    if let Ok(old) = OldPositionOf::<T>::decode_all(&mut &bytes[..]) {
        return Some(old.into_position::<T>());
    }
    PositionOf::<T>::decode_all(&mut &bytes[..]).ok()
}

/// Idempotent: guarded by [`HasMigrationRun`], and each row is rewritten only
/// when it still has the pre-terms layout.
pub fn migrate_position_terms_version<T: Config>() -> Weight {
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

    // Ensure the current-open version defaults to v1 when unset.
    if CurrentTermsVersion::<T>::get() == 0 {
        CurrentTermsVersion::<T>::put(TERMS_VERSION_V1);
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
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
        migrate_position_terms_version::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let mut expected = Vec::new();
        for (owner, netuid) in Positions::<T>::iter_keys() {
            let bytes = unhashed::get_raw(&Positions::<T>::hashed_key_for(&owner, netuid))
                .ok_or("Positions key must have a value")?;
            let position = decode_either::<T>(&bytes)
                .ok_or("Positions must decode as pre-terms or current layout")?;
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
            "position-terms-version migration must be stamped"
        );
        ensure!(
            CurrentTermsVersion::<T>::get() >= TERMS_VERSION_V1,
            "CurrentTermsVersion must be at least v1"
        );
        ensure!(
            Positions::<T>::iter().count() == expected.len(),
            "migration must not add or drop positions"
        );
        for (owner, netuid, position) in expected {
            ensure!(
                Positions::<T>::get(&owner, netuid).as_ref() == Some(&position),
                "migration must preserve every Position field"
            );
            ensure!(
                position.terms_version == TERMS_VERSION_V1,
                "migrated positions must be terms v1"
            );
        }
        Ok(())
    }
}
