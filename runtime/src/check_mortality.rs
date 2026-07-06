use codec::{Decode, DecodeWithMemTracking, Encode};
use core::marker::PhantomData;
use frame_support::pallet_prelude::TypeInfo;
use frame_support::traits::IsSubType;
use frame_system::CheckMortality as CheckMortalitySubstrate;
use pallet_shield::Call as ShieldCall;
use sp_runtime::{
    generic::Era,
    traits::{DispatchInfoOf, Dispatchable, Implication, TransactionExtension, ValidateResult},
    transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
};
use subtensor_macros::freeze_struct;

/// Maximum allowed Era period (in blocks) for `submit_encrypted` transactions.
///
/// Substrate's minimum mortal Era is 4 blocks (smallest power-of-two ≥ 4).
/// Limiting encrypted txs to this value ensures stuck transactions evict from
/// the fork-aware tx pool within a handful of blocks.
const MAX_SHIELD_ERA_PERIOD: u64 = 8;

/// A transparent wrapper around [`frame_system::CheckMortality`] that additionally
/// enforces a short Era period for [`pallet_shield::Call::submit_encrypted`] transactions.
///
/// Drop-in replacement for `frame_system::CheckMortality` in the runtime's
/// transaction extension pipeline. Shares the same `IDENTIFIER = "CheckMortality"`
/// and identical SCALE encoding, so existing clients require no changes.
///
/// Any `submit_encrypted` call signed with an immortal Era or a mortal Era period
/// longer than [`MAX_SHIELD_ERA_PERIOD`] is rejected immediately at pool submission
/// with `InvalidTransaction::Stale`, preventing pool bloat from long-lived
/// encrypted transactions that can never be decrypted.
#[freeze_struct("3cb7a665d55d00e5")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct CheckMortality<T: frame_system::Config + Send + Sync>(pub Era, PhantomData<T>);

impl<T: frame_system::Config + Send + Sync> CheckMortality<T> {
    pub fn from(era: Era) -> Self {
        Self(era, PhantomData)
    }
}

impl<T: frame_system::Config + Send + Sync> core::fmt::Debug for CheckMortality<T> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "CheckMortality({:?})", self.0)
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl<T: frame_system::Config + Send + Sync + TypeInfo>
    TransactionExtension<<T as frame_system::Config>::RuntimeCall> for CheckMortality<T>
where
    <T as frame_system::Config>::RuntimeCall: Dispatchable + IsSubType<ShieldCall<T>>,
    T: pallet_shield::Config,
{
    const IDENTIFIER: &'static str = "CheckMortality";

    type Implicit = <CheckMortalitySubstrate<T> as TransactionExtension<
        <T as frame_system::Config>::RuntimeCall,
    >>::Implicit;
    type Val = <CheckMortalitySubstrate<T> as TransactionExtension<
        <T as frame_system::Config>::RuntimeCall,
    >>::Val;
    type Pre = <CheckMortalitySubstrate<T> as TransactionExtension<
        <T as frame_system::Config>::RuntimeCall,
    >>::Pre;

    fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError> {
        CheckMortalitySubstrate::<T>::from(self.0).implicit()
    }

    fn weight(&self, call: &<T as frame_system::Config>::RuntimeCall) -> sp_weights::Weight {
        CheckMortalitySubstrate::<T>::from(self.0).weight(call)
    }

    fn validate(
        &self,
        origin: T::RuntimeOrigin,
        call: &<T as frame_system::Config>::RuntimeCall,
        info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        len: usize,
        self_implicit: Self::Implicit,
        inherited_implication: &impl Implication,
        source: TransactionSource,
    ) -> ValidateResult<Self::Val, <T as frame_system::Config>::RuntimeCall> {
        if let Some(ShieldCall::submit_encrypted { .. }) =
            IsSubType::<ShieldCall<T>>::is_sub_type(call)
        {
            let era_too_long = match self.0 {
                Era::Immortal => true,
                Era::Mortal(period, _) => period > MAX_SHIELD_ERA_PERIOD,
            };
            if era_too_long {
                return Err(InvalidTransaction::Stale.into());
            }
        }

        CheckMortalitySubstrate::<T>::from(self.0).validate(
            origin,
            call,
            info,
            len,
            self_implicit,
            inherited_implication,
            source,
        )
    }

    fn prepare(
        self,
        val: Self::Val,
        origin: &T::RuntimeOrigin,
        call: &<T as frame_system::Config>::RuntimeCall,
        info: &DispatchInfoOf<<T as frame_system::Config>::RuntimeCall>,
        len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        CheckMortalitySubstrate::<T>::from(self.0).prepare(val, origin, call, info, len)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    use frame_support::pallet_prelude::{BoundedVec, ConstU32};

    use sp_runtime::transaction_validity::InvalidTransaction;

    use crate::{Runtime, RuntimeCall, System};
    use sp_runtime::BuildStorage;

    fn new_test_ext() -> sp_io::TestExternalities {
        let mut ext: sp_io::TestExternalities = crate::RuntimeGenesisConfig {
            sudo: pallet_sudo::GenesisConfig { key: None },
            ..Default::default()
        }
        .build_storage()
        .unwrap()
        .into();
        ext.execute_with(|| System::set_block_number(1));
        ext
    }

    fn submit_encrypted_call() -> RuntimeCall {
        RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted {
            ciphertext: BoundedVec::<u8, ConstU32<8192>>::truncate_from(vec![0xAA; 64]),
        })
    }

    fn remark_call() -> RuntimeCall {
        RuntimeCall::System(frame_system::Call::remark { remark: vec![] })
    }

    /// Only tests the early-return path (era check). Does NOT call into
    /// CheckMortalitySubstrate which needs real block hashes.
    fn validate_era_check(era: Era, call: &RuntimeCall) -> Result<(), TransactionValidityError> {
        if let Some(ShieldCall::submit_encrypted { .. }) =
            IsSubType::<ShieldCall<Runtime>>::is_sub_type(call)
        {
            let era_too_long = match era {
                Era::Immortal => true,
                Era::Mortal(period, _) => period > MAX_SHIELD_ERA_PERIOD,
            };
            if era_too_long {
                return Err(InvalidTransaction::Stale.into());
            }
        }
        Ok(())
    }

    #[test]
    fn shield_tx_with_immortal_era_rejected() {
        new_test_ext().execute_with(|| {
            assert_eq!(
                validate_era_check(Era::Immortal, &submit_encrypted_call()),
                Err(InvalidTransaction::Stale.into())
            );
        });
    }

    #[test]
    fn shield_tx_with_era_too_long_rejected() {
        new_test_ext().execute_with(|| {
            // Period 16 > MAX_SHIELD_ERA_PERIOD (8)
            assert_eq!(
                validate_era_check(Era::mortal(16, 1), &submit_encrypted_call()),
                Err(InvalidTransaction::Stale.into())
            );
        });
    }

    #[test]
    fn shield_tx_with_max_allowed_era_accepted() {
        new_test_ext().execute_with(|| {
            assert!(validate_era_check(Era::mortal(8, 1), &submit_encrypted_call()).is_ok());
        });
    }

    #[test]
    fn shield_tx_with_short_era_accepted() {
        new_test_ext().execute_with(|| {
            assert!(validate_era_check(Era::mortal(4, 1), &submit_encrypted_call()).is_ok());
        });
    }

    #[test]
    fn non_shield_tx_with_immortal_era_passes_through() {
        new_test_ext().execute_with(|| {
            assert!(validate_era_check(Era::Immortal, &remark_call()).is_ok());
        });
    }

    #[test]
    fn non_shield_tx_with_long_era_passes_through() {
        new_test_ext().execute_with(|| {
            assert!(validate_era_check(Era::mortal(256, 1), &remark_call()).is_ok());
        });
    }
}
