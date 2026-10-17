//! Owner-issued purpose keys. Epoch tombstones survive revocation and reaping.

use crate::*;
use frame_support::{
    BoundedVec, ensure,
    traits::{ConstU32, Get},
    transactional,
};
use sp_runtime::traits::SaturatedConversion;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum CapabilityRole {
    #[codec(index = 1)]
    Order,
    #[codec(index = 2)]
    Message,
}

/// A restricted grant trades only one subnet/hotkey and pays one fee recipient.
/// Buy budgets count gross TAO input (including fees); sell budgets count alpha
/// input. Separate counters prevent mixing unlike asset denominations.
#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct TradingScope {
    pub netuid: u16,
    pub hotkey: AccountId32,
    pub fee_recipient: AccountId32,
    pub max_fee_perbill: u32,
    pub max_tao_per_order: u64,
    pub max_alpha_per_order: u64,
    pub total_tao: u64,
    pub total_alpha: u64,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub enum CapabilityScope {
    /// Zero budget disables the respective buy/sell direction.
    Trading(TradingScope),
    /// Explicit owner approval for unrestricted trading, still subject to
    /// capability expiry, epoch, order terms, owner freezes and balance checks.
    UnrestrictedTrading,
    /// Application/audience bytes. HTTP verifiers must bind the same audience,
    /// method, path, body hash, challenge, expiry and epoch in their payload.
    Message {
        audience: BoundedVec<u8, ConstU32<128>>,
    },
}

impl CapabilityScope {
    pub fn role(&self) -> CapabilityRole {
        match self {
            Self::Trading(_) | Self::UnrestrictedTrading => CapabilityRole::Order,
            Self::Message { .. } => CapabilityRole::Message,
        }
    }

    fn is_valid(&self) -> bool {
        match self {
            Self::Trading(scope) => {
                scope.netuid != 0
                    && scope.max_fee_perbill <= 1_000_000_000
                    && ((scope.max_tao_per_order > 0 && scope.total_tao > 0)
                        || (scope.max_alpha_per_order > 0 && scope.total_alpha > 0))
            }
            Self::UnrestrictedTrading => true,
            Self::Message { audience } => !audience.is_empty(),
        }
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct CapabilityRecord {
    pub epoch: u64,
    pub key_commitment: [u8; 64],
    /// Inclusive block-number deadline; order timestamps remain milliseconds.
    pub expires_at: u64,
    pub scope: CapabilityScope,
    pub spent_tao: u128,
    pub spent_alpha: u128,
}

impl<T: Config> Pallet<T> {
    #[transactional]
    pub fn do_install_capability(
        owner: &AccountId32,
        expected_epoch: u64,
        key_commitment: [u8; 64],
        expires_at: u64,
        scope: CapabilityScope,
    ) -> DispatchResult {
        ensure!(T::Enabled::get(), Error::<T>::Disabled);
        ensure!(
            Self::authority_meets(owner, SecurityFloor::PqRequired),
            Error::<T>::NotRegistered
        );
        ensure!(
            key_commitment != [0; 64] && scope.is_valid(),
            Error::<T>::CapabilityInvalid
        );
        let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into();
        ensure!(expires_at > now, Error::<T>::CapabilityExpired);
        let role = scope.role();
        let epoch = CapabilityEpochs::<T>::get(owner, role)
            .checked_add(1)
            .ok_or(Error::<T>::GenerationExhausted)?;
        ensure!(expected_epoch == epoch, Error::<T>::CapabilityEpochMismatch);
        CapabilityEpochs::<T>::insert(owner, role, epoch);
        ActiveCapabilities::<T>::insert(
            owner,
            role,
            CapabilityRecord {
                epoch,
                key_commitment,
                expires_at,
                scope,
                spent_tao: 0,
                spent_alpha: 0,
            },
        );
        Self::deposit_event(Event::CapabilityInstalled {
            owner: owner.clone(),
            role,
            epoch,
        });
        Ok(())
    }

    /// Revocation remains available when authorization is disabled or the epoch
    /// counter is exhausted. The permanent tombstone is never removed/reset.
    #[transactional]
    pub fn do_revoke_capability(owner: &AccountId32, role: CapabilityRole) -> DispatchResult {
        ensure!(
            Self::authority_meets(owner, SecurityFloor::PqRequired),
            Error::<T>::NotRegistered
        );
        let epoch = CapabilityEpochs::<T>::get(owner, role).saturating_add(1);
        CapabilityEpochs::<T>::insert(owner, role, epoch);
        ActiveCapabilities::<T>::remove(owner, role);
        Self::deposit_event(Event::CapabilityRevoked {
            owner: owner.clone(),
            role,
            epoch,
        });
        Ok(())
    }

    pub fn checked_capability(
        owner: &AccountId32,
        role: CapabilityRole,
        epoch: u64,
        public_key: &subtensor_hashed::mldsa::PublicKeyBytes,
    ) -> Result<CapabilityRecord, sp_runtime::DispatchError> {
        ensure!(T::Enabled::get(), Error::<T>::Disabled);
        let record =
            ActiveCapabilities::<T>::get(owner, role).ok_or(Error::<T>::CapabilityMissing)?;
        ensure!(
            record.epoch == epoch && record.scope.role() == role,
            Error::<T>::CapabilityEpochMismatch
        );
        let now: u64 = frame_system::Pallet::<T>::block_number().saturated_into();
        ensure!(now <= record.expires_at, Error::<T>::CapabilityExpired);
        ensure!(
            subtensor_hashed::mldsa::commitment(public_key) == record.key_commitment,
            Error::<T>::WrongCommitment
        );
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check_order_capability(
        owner: &AccountId32,
        epoch: u64,
        public_key: &subtensor_hashed::mldsa::PublicKeyBytes,
        netuid: u16,
        hotkey: &AccountId32,
        fee_recipient: &AccountId32,
        fee_perbill: u32,
        is_buy: bool,
        full_amount: u64,
        fill_amount: u64,
    ) -> DispatchResult {
        let record = Self::checked_capability(owner, CapabilityRole::Order, epoch, public_key)?;
        if let CapabilityScope::Trading(scope) = &record.scope {
            ensure!(
                scope.netuid == netuid
                    && &scope.hotkey == hotkey
                    && &scope.fee_recipient == fee_recipient
                    && fee_perbill <= scope.max_fee_perbill,
                Error::<T>::CapabilityScopeViolation
            );
            let (per_order, total, spent) = if is_buy {
                (scope.max_tao_per_order, scope.total_tao, record.spent_tao)
            } else {
                (
                    scope.max_alpha_per_order,
                    scope.total_alpha,
                    record.spent_alpha,
                )
            };
            ensure!(
                full_amount > 0
                    && full_amount <= per_order
                    && spent
                        .checked_add(u128::from(fill_amount))
                        .is_some_and(|next| next <= u128::from(total)),
                Error::<T>::CapabilityScopeViolation
            );
        }
        Ok(())
    }

    /// Called inside the order's storage transaction, so swap failures cannot
    /// consume budget and accounting failure cannot leave a successful swap.
    pub fn consume_order_capability(
        owner: &AccountId32,
        epoch: u64,
        is_buy: bool,
        fill_amount: u64,
    ) -> DispatchResult {
        ActiveCapabilities::<T>::try_mutate(owner, CapabilityRole::Order, |maybe| {
            let record = maybe.as_mut().ok_or(Error::<T>::CapabilityMissing)?;
            ensure!(record.epoch == epoch, Error::<T>::CapabilityEpochMismatch);
            let spent = if is_buy {
                &mut record.spent_tao
            } else {
                &mut record.spent_alpha
            };
            *spent = spent
                .checked_add(u128::from(fill_amount))
                .ok_or(Error::<T>::CapabilityScopeViolation)?;
            if let CapabilityScope::Trading(scope) = &record.scope {
                let total = if is_buy {
                    scope.total_tao
                } else {
                    scope.total_alpha
                };
                ensure!(
                    *spent <= u128::from(total),
                    Error::<T>::CapabilityScopeViolation
                );
            }
            Ok(())
        })
    }
}
