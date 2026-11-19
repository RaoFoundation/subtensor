//! Rewrite live positions onto terms v2 with a hard [`Config::MaxLoanDuration`].
//!
//! Mainnet has no lending positions at this upgrade; the migration still
//! handles any test / clone rows: set `terms_version = 2`,
//! `expires_at = migration_block + MaxLoanDuration`, rebuild the expiry
//! schedule entry, and clear stale band-deferral stamps. Idempotent via
//! [`HasMigrationRun`].
use core::marker::PhantomData;

#[cfg(feature = "try-runtime")]
use alloc::vec::Vec;
#[cfg(feature = "try-runtime")]
use codec::{Decode, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::ensure;
use frame_support::{BoundedVec, traits::OnRuntimeUpgrade, weights::Weight};
#[cfg(feature = "try-runtime")]
use sp_runtime::TryRuntimeError;
#[cfg(feature = "try-runtime")]
use sp_runtime::traits::Saturating;

use crate::*;

fn migration_key() -> BoundedVec<u8, MigrationKeyMaxLen> {
    BoundedVec::truncate_from(b"migrate_position_max_duration_v1".to_vec())
}

#[cfg(feature = "try-runtime")]
type PositionOf<T> = Position<<T as frame_system::Config>::AccountId, BlockNumberFor<T>>;

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
        let mut expected = Vec::new();
        for (owner, netuid, position) in Positions::<T>::iter() {
            expected.push((owner, netuid, position));
        }
        Ok((now, expected).encode())
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> {
        let (mig_block, expected): (
            BlockNumberFor<T>,
            Vec<(T::AccountId, NetUid, PositionOf<T>)>,
        ) = Decode::decode(&mut &state[..]).map_err(|_| "pre_upgrade state must decode")?;
        ensure!(
            HasMigrationRun::<T>::get(migration_key()),
            "position-max-duration migration must be stamped"
        );
        ensure!(
            CurrentTermsVersion::<T>::get() >= TERMS_VERSION_V2,
            "CurrentTermsVersion must be at least v2"
        );
        let want_expires = mig_block.saturating_add(T::MaxLoanDuration::get());
        ensure!(
            Positions::<T>::iter().count() == expected.len(),
            "migration must not add or drop positions"
        );
        for (owner, netuid, before) in expected {
            let after =
                Positions::<T>::get(&owner, netuid).ok_or("migrated position must remain")?;
            ensure!(
                after.terms_version == TERMS_VERSION_V2,
                "migrated positions must be terms v2"
            );
            ensure!(
                after.expires_at == want_expires && after.expiry_due == want_expires,
                "migrated expires_at must be migration_block + MaxLoanDuration"
            );
            ensure!(
                after.side == before.side
                    && after.principal == before.principal
                    && after.collateral == before.collateral
                    && after.proceeds == before.proceeds
                    && after.escrow_alpha == before.escrow_alpha
                    && after.lift_tao == before.lift_tao
                    && after.lift_alpha == before.lift_alpha,
                "migration must preserve economic Position fields"
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
        }
        Ok(())
    }
}
