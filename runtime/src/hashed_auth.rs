//! Rotating authorization establishes the usual Signed origin before ordinary
//! nonce, fee and dispatch checks. The wire adapter rolls preparation back on
//! invalid transactions; included dispatch failures still consume a generation.

use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{dispatch::DispatchResult, ensure, weights::Weight};
use pallet_hashed_accounts::weights::WeightInfo;
use scale_info::TypeInfo;
use sp_runtime::{
    DispatchError,
    traits::{DispatchInfoOf, Implication, StaticLookup, TransactionExtension, ValidateResult},
    transaction_validity::{
        InvalidTransaction, TransactionSource, TransactionValidityError, ValidTransaction,
    },
};

use crate::{AccountId, Runtime, RuntimeCall, RuntimeOrigin};

#[cfg(test)]
frame_support::parameter_types! {
    pub static TestVerificationWeight: Weight = Weight::MAX;
}

#[cfg(test)]
pub struct TestWeights;
#[cfg(test)]
impl WeightInfo for TestWeights {
    fn authorize_mldsa(_: u32) -> Weight {
        TestVerificationWeight::get()
    }
    fn check_registered() -> Weight {
        TestVerificationWeight::get()
    }
    fn register() -> Weight {
        TestVerificationWeight::get()
    }
    fn authorize(_: u32) -> Weight {
        TestVerificationWeight::get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub struct AuthorizeAccount<const P: usize = 32, const S: usize = 64> {
    pub account: AccountId,
    pub proof: subtensor_hashed::AuthorizationProof<P, S>,
}

pub type AuthorizeMlDsa = AuthorizeAccount<1952, 3309>;

impl<const P: usize, const S: usize> TransactionExtension<RuntimeCall> for AuthorizeAccount<P, S> {
    const IDENTIFIER: &'static str = if P == 32 {
        "AuthorizeHashedAccount"
    } else {
        "AuthorizeMlDsaAccount"
    };
    type Implicit = ();
    type Val = pallet_hashed_accounts::ValidatedProof;
    type Pre = ();

    fn weight(&self, call: &RuntimeCall) -> Weight {
        let len = u32::try_from(call.encoded_size()).unwrap_or(u32::MAX);
        if P == 32 && S == 64 {
            <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize(len)
        } else if P == 1952 && S == 3309 {
            <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize_mldsa(len)
        } else {
            Weight::MAX
        }
    }

    fn validate(
        &self,
        origin: RuntimeOrigin,
        _: &RuntimeCall,
        _: &DispatchInfoOf<RuntimeCall>,
        _: usize,
        _: (),
        implication: &impl Implication,
        _: TransactionSource,
    ) -> ValidateResult<Self::Val, RuntimeCall> {
        frame_system::ensure_none(origin).map_err(|_| InvalidTransaction::BadSigner)?;
        let ticket =
            crate::HashedAccounts::check_proof(&self.account, &self.proof, &implication.encode())
                .map_err(|_| InvalidTransaction::BadProof)?;
        let valid = ValidTransaction {
            provides: alloc::vec![
                (b"hashed-generation", &self.account, self.proof.generation).encode()
            ],
            ..Default::default()
        };
        Ok((valid, ticket, RuntimeOrigin::signed(self.account.clone())))
    }

    fn prepare(
        self,
        ticket: Self::Val,
        _: &RuntimeOrigin,
        _: &RuntimeCall,
        _: &DispatchInfoOf<RuntimeCall>,
        _: usize,
    ) -> Result<(), TransactionValidityError> {
        crate::HashedAccounts::advance(&ticket).map_err(|_| InvalidTransaction::Stale.into())
    }
}

pub struct OnHashedRegistered;

impl pallet_hashed_accounts::OnRegister for OnHashedRegistered {
    fn on_register(
        account: &AccountId,
        sponsor: &AccountId,
        descriptor: &subtensor_hashed::Descriptor,
    ) -> DispatchResult {
        use sp_runtime::generic::Preamble;
        let invalid = DispatchError::Other("InvalidHashedRegistrationContext");
        ensure!(account != sponsor, invalid);
        let encoded =
            crate::System::extrinsic_data(crate::System::extrinsic_index().unwrap_or_default());
        ensure!(encoded.len() <= 16 * 1024, invalid);
        let mut bytes = encoded.as_slice();
        let outer = crate::UncheckedExtrinsic::decode(&mut bytes).map_err(|_| invalid)?;
        ensure!(bytes.is_empty(), invalid);
        let (outer_account, call) = match outer {
            crate::UncheckedExtrinsic::Legacy(outer) => {
                let Preamble::Signed(address, _, _) = outer.0.preamble else {
                    return Err(invalid);
                };
                (
                    <Runtime as frame_system::Config>::Lookup::lookup(address)
                        .map_err(|_| invalid)?,
                    outer.0.function,
                )
            }
            crate::UncheckedExtrinsic::Hashed(outer) => {
                let Preamble::General(1, (authorization, _)) = outer.0.preamble else {
                    return Err(invalid);
                };
                (authorization.account, outer.0.function)
            }
            crate::UncheckedExtrinsic::MlDsa(outer) => {
                let Preamble::General(2, (authorization, _)) = outer.0.preamble else {
                    return Err(invalid);
                };
                (authorization.account, outer.0.function)
            }
        };
        ensure!(&outer_account == sponsor, invalid);
        let matches_registration = |call: &RuntimeCall| {
            matches!(call,
                RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register { descriptor })
                if subtensor_hashed::account_id(descriptor) == *AsRef::<[u8;32]>::as_ref(account)
            )
        };
        let supported_followup = |call: &RuntimeCall| {
            matches!(
                call,
                RuntimeCall::Balances(_) | RuntimeCall::SubtensorModule(_)
            )
        };
        let allowed = match &call {
            RuntimeCall::HashedAccounts(_) => matches_registration(&call),
            RuntimeCall::Utility(crate::pallet_utility::Call::batch_all { calls }) => {
                match calls.as_slice() {
                    [registration, followup] => {
                        matches_registration(registration) && supported_followup(followup)
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        ensure!(allowed, invalid);
        // The account record is not written yet. Check the incoming scheme so
        // registration cannot preserve a weaker preexisting hotkey owner.
        ensure!(
            pallet_subtensor::Owner::<Runtime>::try_get(account)
                .ok()
                .is_none_or(|owner| authority_satisfies_scheme(descriptor.scheme, &owner)),
            DispatchError::Other("HashedHotkeyRequiresHashedOwner")
        );
        crate::evm_origin::prepare_hashed_alias(account)?;
        crate::Proxy::remove_all_proxy_delegates(account);
        pallet_subtensor::ColdkeySwapAnnouncements::<Runtime>::remove(account);
        pallet_subtensor::ColdkeySwapDisputes::<Runtime>::remove(account);
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark(
        account: &AccountId,
        sponsor: &AccountId,
        descriptor: &subtensor_hashed::Descriptor,
    ) -> DispatchResult {
        use frame_support::traits::Currency;
        let _ = crate::Balances::make_free_balance_be(
            account,
            crate::TaoBalance::new(1_000_000_000_000),
        );
        for index in 0..crate::MaxProxies::get() {
            let delegate = AccountId::new(sp_io::hashing::blake2_256(
                &(b"hashed-benchmark-proxy", index).encode(),
            ));
            crate::Proxy::add_proxy_delegate(account, delegate, crate::ProxyType::Any, 0)?;
        }
        let registration = RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
            descriptor: *descriptor,
        });
        // Exercise decoding a near-limit first-use batch, not just the tiny
        // register call: the registration hook also inspects its outer call.
        let call = RuntimeCall::Utility(crate::pallet_utility::Call::batch_all {
            calls: alloc::vec![
                registration,
                RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_weights {
                    netuid: 1.into(),
                    dests: alloc::vec![0; 4000],
                    weights: alloc::vec![0; 4000],
                    version_key: 0,
                }),
            ],
        });
        let extension = crate::TxExtension::decode(&mut &[0u8; 64][..])
            .map_err(|_| DispatchError::Other("HashedBenchmarkExtension"))?;
        let xt = crate::UncheckedExtrinsic::new_signed(
            call,
            sponsor.clone().into(),
            sp_runtime::MultiSignature::Sr25519(sp_core::sr25519::Signature::from_raw([0; 64])),
            extension,
        );
        crate::System::set_extrinsic_index(0);
        crate::System::note_extrinsic(xt.encode());
        Ok(())
    }
}

/// Every alternative authority must preserve the protected account's scheme.
/// ML-DSA can control either protected scheme; rotating Sr25519 cannot control
/// ML-DSA. Ordinary accounts retain their existing ownership/proxy behavior.
pub(crate) fn compatible_authority(account: &AccountId, authority: &AccountId) -> bool {
    pallet_hashed_accounts::Accounts::<Runtime>::get(account)
        .is_none_or(|record| authority_satisfies_scheme(record.descriptor.scheme, authority))
}

fn authority_satisfies_scheme(required: subtensor_hashed::Scheme, authority: &AccountId) -> bool {
    use subtensor_hashed::Scheme;
    pallet_hashed_accounts::Accounts::<Runtime>::get(authority).is_some_and(|record| {
        matches!(
            (required, record.descriptor.scheme),
            (Scheme::Sr25519, Scheme::Sr25519 | Scheme::MlDsa65)
                | (Scheme::MlDsa65, Scheme::MlDsa65)
        )
    })
}

/// Prevent weaker delegates from bypassing a protected account's policy.
pub struct HashedProxyPolicy;
impl frame_support::traits::Contains<(AccountId, AccountId)> for HashedProxyPolicy {
    fn contains((real, delegate): &(AccountId, AccountId)) -> bool {
        compatible_authority(real, delegate)
    }
}
impl crate::pallet_proxy::ProxyAccountPolicy<AccountId> for HashedProxyPolicy {
    fn weight() -> Weight {
        <Runtime as frame_system::Config>::DbWeight::get().reads(2)
    }
    fn grant_weight() -> Weight {
        use crate::pallet_proxy::WeightInfo;
        Self::weight()
            .saturating_add(<Runtime as frame_system::Config>::DbWeight::get().reads(1))
            .saturating_add(
                <Runtime as crate::pallet_proxy::Config>::WeightInfo::reject_announcement(
                    crate::MaxPending::get(),
                    crate::MaxProxies::get(),
                ),
            )
    }
    fn on_grant(real: &AccountId, delegate: &AccountId) {
        if crate::HashedAccounts::is_registered(real) {
            crate::Proxy::invalidate_announcements(real, delegate);
        }
    }
}
