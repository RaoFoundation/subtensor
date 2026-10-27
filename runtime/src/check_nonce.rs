// Customized from the original implementation in the Polkadot SDK.
// https://github.com/paritytech/polkadot-sdk/blob/b600af050d6b6c8da59ae2a2a793ee2d8827ab1e/substrate/frame/system/src/extensions/check_nonce.rs

use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    RuntimeDebugNoBound,
    dispatch::{DispatchInfo, Pays},
    traits::Get,
};
use frame_system::Config;
use scale_info::TypeInfo;
use sp_runtime::{
    DispatchResult, Saturating, Weight,
    traits::{
        AsSystemOriginSigner, DispatchInfoOf, Dispatchable, One, PostDispatchInfoOf,
        TransactionExtension, ValidateResult, Zero,
    },
    transaction_validity::{
        InvalidTransaction, TransactionLongevity, TransactionSource, TransactionValidityError,
        ValidTransaction,
    },
};
use sp_std::vec;
use subtensor_macros::freeze_struct;

/// Nonce check and increment to give replay protection for transactions.
///
/// # Transaction Validity
///
/// This extension affects `requires` and `provides` tags of validity, but DOES NOT
/// set the `priority` field. Make sure that AT LEAST one of the transaction extension sets
/// some kind of priority upon validating transactions.
///
/// The preparation step assumes that the nonce information has not changed since the validation
/// step. This means that other extensions ahead of `CheckNonce` in the pipeline must not alter the
/// nonce during their own preparation step, or else the transaction may be rejected during dispatch
/// or lead to an inconsistent account state..
#[freeze_struct("cc77e8303313108b")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct CheckNonce<T: Config>(#[codec(compact)] pub T::Nonce);

impl<T: Config> CheckNonce<T> {
    /// utility constructor. Used only in client/factory code.
    pub fn from(nonce: T::Nonce) -> Self {
        Self(nonce)
    }
}

impl<T: Config + pallet_subtensor::Config> CheckNonce<T> {
    /// Whether `who` holds alpha stake on any hotkey.
    ///
    /// A coldkey that received stake (e.g. via `transfer_stake`) but never held
    /// TAO has no provider or sufficient reference, yet the transaction-fee
    /// handler can charge its fee by unstaking alpha (`fees_in_alpha`). Without
    /// this escape hatch such an account is stuck: every signed extrinsic —
    /// including the `remove_stake` that would give it TAO — dies here with
    /// `Payment` before fee logic even runs.
    ///
    /// `StakingHotkeys` may retain hotkeys whose stake has since dropped to
    /// zero, so this can over-approximate. That is safe: passing this guard
    /// only admits the transaction to fee validation, where an account that
    /// cannot actually pay (in TAO or alpha) is still rejected before any
    /// nonce storage is written.
    fn holds_alpha_stake(who: &<T as Config>::AccountId) -> bool {
        pallet_subtensor::StakingHotkeys::<T>::decode_len(who).unwrap_or(0) > 0
    }
}

impl<T: Config> sp_std::fmt::Debug for CheckNonce<T> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        write!(f, "CheckNonce({})", self.0)
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
        Ok(())
    }
}

/// Operation to perform from `validate` to `prepare` in [`CheckNonce`] transaction extension.
#[derive(RuntimeDebugNoBound)]
pub enum Val<T: Config> {
    /// Account and its nonce to check for.
    CheckNonce((T::AccountId, T::Nonce)),
    /// Weight to refund.
    Refund(Weight),
}

/// Operation to perform from `prepare` to `post_dispatch_details` in [`CheckNonce`] transaction
/// extension.
#[derive(RuntimeDebugNoBound)]
pub enum Pre {
    /// The transaction extension weight should not be refunded.
    NonceChecked,
    /// The transaction extension weight should be refunded.
    Refund(Weight),
}

impl<T: Config + pallet_subtensor::Config> TransactionExtension<<T as Config>::RuntimeCall>
    for CheckNonce<T>
where
    <T as Config>::RuntimeCall: Dispatchable<Info = DispatchInfo>,
    <<T as Config>::RuntimeCall as Dispatchable>::RuntimeOrigin:
        AsSystemOriginSigner<<T as Config>::AccountId> + Clone,
{
    const IDENTIFIER: &'static str = "CheckNonce";
    type Implicit = ();
    type Val = Val<T>;
    type Pre = Pre;

    fn weight(&self, _: &<T as Config>::RuntimeCall) -> Weight {
        // Account for the account-nonce storage ops the extension performs on
        // signed transactions: one `Account::get` read in `validate`, plus one
        // `Account::mutate` (read + write) in `prepare` to bump the nonce, plus
        // the worst-case `StakingHotkeys` length read for reference-less
        // signers. Non-signed calls refund this weight in full via
        // `Val::Refund`.
        <T as Config>::DbWeight::get().reads_writes(3, 1)
    }

    fn validate(
        &self,
        origin: <T as Config>::RuntimeOrigin,
        call: &<T as Config>::RuntimeCall,
        info: &DispatchInfoOf<<T as Config>::RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Encode,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, <T as Config>::RuntimeCall> {
        let Some(who) = origin.as_system_origin_signer() else {
            return Ok((Default::default(), Val::Refund(self.weight(call)), origin));
        };
        let account = frame_system::Account::<T>::get(who);
        if info.pays_fee == Pays::Yes
            && account.providers.is_zero()
            && account.sufficients.is_zero()
            && !Self::holds_alpha_stake(who)
        {
            // Nonce storage not paid for
            return Err(InvalidTransaction::Payment.into());
        }
        if self.0 < account.nonce {
            return Err(InvalidTransaction::Stale.into());
        }

        let provides = vec![Encode::encode(&(&who, self.0))];
        let requires = if account.nonce < self.0 {
            vec![Encode::encode(&(&who, self.0.saturating_sub(One::one())))]
        } else {
            vec![]
        };

        let validity = ValidTransaction {
            priority: 0,
            requires,
            provides,
            longevity: TransactionLongevity::MAX,
            propagate: true,
        };

        Ok((
            validity,
            Val::CheckNonce((who.clone(), account.nonce)),
            origin,
        ))
    }

    fn prepare(
        self,
        val: Self::Val,
        _origin: &<T as Config>::RuntimeOrigin,
        _call: &<T as Config>::RuntimeCall,
        _info: &DispatchInfoOf<<T as Config>::RuntimeCall>,
        _len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        let (who, mut nonce) = match val {
            Val::CheckNonce((who, nonce)) => (who, nonce),
            Val::Refund(weight) => return Ok(Pre::Refund(weight)),
        };

        // `self.0 < nonce` already checked in `validate`.
        if self.0 > nonce {
            return Err(InvalidTransaction::Future.into());
        }
        nonce += <T as Config>::Nonce::one();
        frame_system::Account::<T>::mutate(&who, |account| {
            if account.providers.is_zero() && account.sufficients.is_zero() {
                // A signer admitted without a provider reference (`Pays::No` call or
                // alpha-paid fee) would otherwise hold its nonce in an account that any
                // balance write can remove, resetting the nonce and making its earlier
                // signed extrinsics valid again. Hold a self-sufficient reference so the
                // account, and with it the nonce, survives every balance change.
                account.sufficients = One::one();
                frame_system::Pallet::<T>::on_created_account(who.clone(), account);
            }
            account.nonce = nonce;
        });
        Ok(Pre::NonceChecked)
    }

    fn post_dispatch_details(
        pre: Self::Pre,
        _info: &DispatchInfo,
        _post_info: &PostDispatchInfoOf<<T as Config>::RuntimeCall>,
        _len: usize,
        _result: &DispatchResult,
    ) -> Result<Weight, TransactionValidityError> {
        match pre {
            Pre::NonceChecked => Ok(Weight::zero()),
            Pre::Refund(weight) => Ok(weight),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{Balances, BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig};
    use frame_support::assert_ok;
    use frame_support::dispatch::GetDispatchInfo;
    use frame_support::traits::fungible::Mutate;
    use frame_support::traits::tokens::Preservation;
    use frame_system::RawOrigin;
    use sp_runtime::traits::{DispatchTransaction, Zero};
    use subtensor_runtime_common::{AccountId, TaoBalance, Token};

    fn new_test_ext() -> sp_io::TestExternalities {
        let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
            .build_storage()
            .unwrap()
            .into();
        ext.execute_with(|| frame_system::Pallet::<Runtime>::set_block_number(1));
        ext
    }

    fn account_state(who: &AccountId) -> (bool, u32, u32, u32) {
        let exists = frame_system::Account::<Runtime>::contains_key(who);
        let account = frame_system::Account::<Runtime>::get(who);
        (
            exists,
            account.nonce,
            account.providers,
            account.sufficients,
        )
    }

    fn validate_and_prepare(
        who: &AccountId,
        nonce: u32,
        call: &RuntimeCall,
        info: &DispatchInfo,
    ) -> Result<(), TransactionValidityError> {
        CheckNonce::<Runtime>::from(nonce)
            .validate_and_prepare(RawOrigin::Signed(who.clone()).into(), call, info, 0, 0)
            .map(|_| ())
    }

    #[test]
    fn reference_less_signer_keeps_its_nonce_through_a_full_balance_drain() {
        new_test_ext().execute_with(|| {
            let signer = AccountId::from([7_u8; 32]);
            let sink = AccountId::from([8_u8; 32]);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            let mut info = call.get_dispatch_info();
            info.pays_fee = Pays::No;
            assert_eq!(account_state(&signer), (false, 0, 0, 0));

            // A `Pays::No` call from a key with no references is admitted; the nonce
            // write must leave the account with a reference of its own.
            assert_ok!(validate_and_prepare(&signer, 0, &call, &info));
            assert_eq!(account_state(&signer), (true, 1, 0, 1));

            // Receiving TAO and then moving all of it out (the shape of a dispatch that
            // credits and drains the signer) no longer removes the account.
            let ed = <Runtime as pallet_balances::Config>::ExistentialDeposit::get();
            let amount = TaoBalance::from(ed.to_u64() * 10);
            assert_ok!(Balances::mint_into(&signer, amount));
            assert_eq!(account_state(&signer), (true, 1, 1, 1));
            assert_ok!(Balances::transfer(
                &signer,
                &sink,
                amount,
                Preservation::Expendable
            ));
            assert_eq!(Balances::free_balance(&signer), TaoBalance::ZERO);
            assert_eq!(account_state(&signer), (true, 1, 0, 1));

            // The identical signed extrinsic is stale.
            assert_eq!(
                validate_and_prepare(&signer, 0, &call, &info),
                Err(InvalidTransaction::Stale.into())
            );
            assert_ok!(validate_and_prepare(&signer, 1, &call, &info));
            assert_eq!(account_state(&signer), (true, 2, 0, 1));
        });
    }

    #[test]
    fn funded_signer_does_not_gain_an_extra_reference() {
        new_test_ext().execute_with(|| {
            let signer = AccountId::from([9_u8; 32]);
            let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
            let info = call.get_dispatch_info();
            let ed = <Runtime as pallet_balances::Config>::ExistentialDeposit::get();
            assert_ok!(Balances::mint_into(
                &signer,
                TaoBalance::from(ed.to_u64() * 10)
            ));
            assert_eq!(account_state(&signer), (true, 0, 1, 0));

            assert_ok!(validate_and_prepare(&signer, 0, &call, &info));
            assert_eq!(account_state(&signer), (true, 1, 1, 0));
        });
    }

    #[test]
    fn check_nonce_weight_accounts_for_account_storage_ops() {
        let ext = CheckNonce::<Runtime>::from(<<Runtime as frame_system::Config>::Nonce>::zero());
        let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
        // validate performs one `Account::get` read plus, for reference-less
        // signers, one `StakingHotkeys` length read; prepare performs one
        // `Account::mutate` (read + write). The declared extension weight must
        // reflect those ops, not zero.
        let expected = <Runtime as frame_system::Config>::DbWeight::get().reads_writes(3, 1);
        assert_eq!(ext.weight(&call), expected);
        assert!(!ext.weight(&call).is_zero());
    }
}
