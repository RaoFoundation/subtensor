//! Transaction extension that rejects sudo calls signed by anyone but the
//! configured sudo key.
//!
//! Drop-in check in the runtime's [`CustomTxExtension`](crate::CustomTxExtension)
//! pipeline. Non-sudo calls and unsigned origins pass through unchanged.
//! `IDENTIFIER` and SCALE layout are frozen — do not rename the type or alter
//! its encoding without a client migration.

use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::dispatch::{DispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::Config;
use pallet_sudo::Call as SudoCall;
use scale_info::TypeInfo;
use sp_runtime::impl_tx_ext_default;
use sp_runtime::traits::{
    AsSystemOriginSigner, DispatchInfoOf, Dispatchable, Implication, TransactionExtension,
    ValidateResult,
};
use sp_runtime::transaction_validity::{InvalidTransaction, TransactionSource};
use sp_std::marker::PhantomData;
use subtensor_macros::freeze_struct;

/// Rejects `pallet_sudo` calls whose signer is not the on-chain sudo key.
///
/// Returns `InvalidTransaction::BadSigner` when no sudo key is configured or
/// when the signer differs. Never alters origin; sudo dispatch still enforces
/// its own checks.
#[freeze_struct("290a37b979472932")]
#[derive(Default, Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
pub struct SudoTransactionExtension<T: Config + Send + Sync + TypeInfo>(pub PhantomData<T>);

impl<T: Config + Send + Sync + TypeInfo> sp_std::fmt::Debug for SudoTransactionExtension<T> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "SudoTransactionExtension",)
    }
    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl<T: Config + Send + Sync + TypeInfo> SudoTransactionExtension<T> {
    pub fn new() -> Self {
        Self(Default::default())
    }
}

impl<T: Config + Send + Sync + TypeInfo + pallet_sudo::Config>
    TransactionExtension<<T as Config>::RuntimeCall> for SudoTransactionExtension<T>
where
    <T as Config>::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <T as Config>::RuntimeOrigin: AsSystemOriginSigner<T::AccountId> + Clone,
    <T as Config>::RuntimeCall: IsSubType<SudoCall<T>>,
{
    const IDENTIFIER: &'static str = "SudoTransactionExtension";

    type Implicit = ();
    type Val = ();
    type Pre = ();

    impl_tx_ext_default!(<T as Config>::RuntimeCall; weight prepare);

    fn validate(
        &self,
        origin: <T as Config>::RuntimeOrigin,
        call: &<T as Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as Config>::RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Implication,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, <T as Config>::RuntimeCall> {
        // Ensure the transaction is signed, else we just skip the extension.
        let Some(who) = origin.as_system_origin_signer() else {
            return Ok((Default::default(), (), origin));
        };

        // Check validity of the signer for sudo call
        if let Some(_sudo_call) = IsSubType::<pallet_sudo::Call<T>>::is_sub_type(call) {
            let sudo_key = pallet_sudo::pallet::Key::<T>::get();

            // No sudo key configured → reject
            let Some(expected_who) = sudo_key else {
                return Err(InvalidTransaction::BadSigner.into());
            };

            // Signer does not match the sudo key → reject
            if *who != expected_who {
                return Err(InvalidTransaction::BadSigner.into());
            }
        }

        Ok((Default::default(), (), origin))
    }
}
