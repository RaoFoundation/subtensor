//! Rewrite live positions onto terms v2 with a hard [`Config::MaxLoanDuration`].
//!
//! Mainnet has no lending positions at this upgrade; the migration still
//! handles any test / clone rows: set `terms_version = 2`,
//! `expires_at = migration_block + MaxLoanDuration`, seed
//! `last_exposure_added_at` from the pre-rewrite open clock when missing,
//! rebuild the expiry schedule entry, and clear stale band-deferral stamps.
//! Idempotent via [`HasMigrationRun`].
//!
//! try-runtime pre/post uses count + twox fingerprint, not full position
//! vectors (audit M3).
use core::marker::PhantomData;

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;
use codec::{Decode, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::ensure;
use frame_support::{BoundedVec, traits::OnRuntimeUpgrade, weights::Weight};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;
use sp_runtime::traits::{SaturatedConversion, Saturating};

use crate::*;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_position_max_duration_v1".to_vec())
}

/// Pre-`last_exposure_added_at` [`Position`] layout (terms v1/v2 fields present).
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug)]
pub struct PositionBeforeExposure<AccountId, BlockNumber> {
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
    pub terms_version: u16,
    pub interest_period: BlockNumber,
    pub blocks_per_year: u64,
}

impl<AccountId, BlockNumber> PositionBeforeExposure<AccountId, BlockNumber> {
    pub fn into_position<T: Config>(self) -> Position<T::AccountId, BlockNumberFor<T>>
    where
        AccountId: Into<T::AccountId>,
        BlockNumber: Into<BlockNumberFor<T>>,
    {
        let expires_at: BlockNumberFor<T> = self.expires_at.into();
        let term = match self.terms_version {
            TERMS_VERSION_V2 => T::MaxLoanDuration::get(),
            _ => self.blocks_per_year.saturated_into(),
        };
        let last_exposure_added_at = expires_at.saturating_sub(term);
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
            expires_at,
            expiry_due: self.expiry_due.into(),
            terms_version: self.terms_version,
            interest_period: self.interest_period.into(),
            blocks_per_year: self.blocks_per_year,
            last_exposure_added_at,
        }
    }
}

#[cfg(feature = "try-runtime")]
fn positions_fingerprint<T: Config>() -> (u64, [u8; 32]) {
    use sp_io::hashing::twox_256;
    let mut count = 0u64;
    let mut acc = [0u8; 32];
    for (owner, netuid, position) in Positions::<T>::iter() {
        count = count.saturating_add(1);
        let chunk = twox_256(&(owner, netuid, position).encode());
        for (a, b) in acc.iter_mut().zip(chunk.iter()) {
            *a ^= *b;
        }
    }
    (count, acc)
}

/// Idempotent: guarded by [`HasMigrationRun`].
pub fn migrate_position_max_duration<T: Config>() -> Weight {
    Pallet::<T>::migrate_position_max_duration(migration_key())
}

pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_position_max_duration::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
        let now = frame_system::Pallet::<T>::block_number();
        let (count, fingerprint) = positions_fingerprint::<T>();
        // Economic field sample for preservation (xor of principals / sides).
        let mut econ = 0u64;
        for (_o, _n, p) in Positions::<T>::iter() {
            econ = econ
                .wrapping_add(p.principal)
                .wrapping_add(p.collateral)
                .wrapping_add(p.proceeds)
                .wrapping_add(p.escrow_alpha)
                .wrapping_add(p.lift_tao)
                .wrapping_add(p.lift_alpha)
                .wrapping_add(match p.side {
                    Side::Short => 1,
                    Side::Long => 2,
                })
                .wrapping_add(TryInto::<u64>::try_into(p.last_exposure_added_at).unwrap_or(0));
        }
        Ok((now, count, fingerprint, econ).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (mig_block, count_before, _fp_before, econ_before): (
            BlockNumberFor<T>,
            u64,
            [u8; 32],
            u64,
        ) = Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "position-max-duration migration must be stamped"
        );
        ensure!(
            CurrentTermsVersion::<T>::get() >= TERMS_VERSION_V2,
            "CurrentTermsVersion must be at least v2"
        );
        let (count_after, _) = positions_fingerprint::<T>();
        ensure!(
            count_after == count_before,
            "migration must not add or drop positions"
        );
        let want_expires = mig_block.saturating_add(T::MaxLoanDuration::get());
        let mut econ_after = 0u64;
        for (owner, netuid, after) in Positions::<T>::iter() {
            ensure!(
                after.terms_version == TERMS_VERSION_V2,
                "migrated positions must be terms v2"
            );
            ensure!(
                after.expires_at == want_expires && after.expiry_due == want_expires,
                "migrated expires_at must be migration_block + MaxLoanDuration"
            );
            ensure!(
                Expiry::<T>::contains_key(want_expires, (&owner, netuid)),
                "migrated position must sit on the rebuilt expiry schedule"
            );
            ensure!(
                BandDeferralSince::<T>::get(&owner, netuid).is_none(),
                "band-deferral stamps must be cleared"
            );
            ensure!(
                LastDeferral::<T>::get(&owner, netuid).is_none(),
                "last-deferral stamps must be cleared"
            );
            econ_after = econ_after
                .wrapping_add(after.principal)
                .wrapping_add(after.collateral)
                .wrapping_add(after.proceeds)
                .wrapping_add(after.escrow_alpha)
                .wrapping_add(after.lift_tao)
                .wrapping_add(after.lift_alpha)
                .wrapping_add(match after.side {
                    Side::Short => 1,
                    Side::Long => 2,
                })
                .wrapping_add(TryInto::<u64>::try_into(after.last_exposure_added_at).unwrap_or(0));
        }
        ensure!(
            econ_after == econ_before,
            "migration must preserve economic Position fields and exposure clock"
        );
        Ok(())
    }
}
