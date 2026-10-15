//! Standard v4 signatures authenticate the rotating key; this wire-transparent
//! adapter applies nonce, fee and call policy to its permanent account.
use crate::{AccountId, Runtime, RuntimeCall, RuntimeOrigin, TxExtension};
use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{
    dispatch::{DispatchResult, PostDispatchInfo},
    ensure,
    weights::Weight,
};
use pallet_hashed_accounts::weights::WeightInfo;
use scale_info::TypeInfo;
use sp_runtime::{
    DispatchError,
    traits::{
        DispatchInfoOf, Implication, StaticLookup, TransactionExtension,
        TransactionExtensionMetadata, ValidateResult,
    },
    transaction_validity::{
        InvalidTransaction, TransactionSource, TransactionValidity, TransactionValidityError,
    },
};

#[cfg(test)]
frame_support::parameter_types! { pub static TestVerificationWeight: Weight = Weight::MAX; }
#[cfg(test)]
pub struct TestWeights;
#[cfg(test)]
impl WeightInfo for TestWeights {
    fn authorize_ed25519(_: u32) -> Weight {
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

/// SCALE-transparent: the signed extension fields and metadata are unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking)]
pub struct AccountTxExtension(
    pub TxExtension,
    #[codec(skip)] pub(crate) Option<subtensor_hashed::SignerAddress>,
);
impl TypeInfo for AccountTxExtension {
    type Identity = Self;
    fn type_info() -> scale_info::Type {
        TxExtension::<false>::type_info()
    }
}
impl From<TxExtension> for AccountTxExtension {
    fn from(value: TxExtension) -> Self {
        Self(value, None)
    }
}
fn authorized_extension(extension: TxExtension) -> TxExtension<true> {
    let (system, (payment, sudo, shield, subtensor, drand), metadata) = extension;
    (
        system,
        (
            payment.with_account_authorization(),
            sudo,
            shield,
            subtensor,
            drand,
        ),
        metadata,
    )
}

impl TransactionExtension<RuntimeCall> for AccountTxExtension {
    const IDENTIFIER: &'static str = "AccountTxExtension";
    type Implicit = <TxExtension as TransactionExtension<RuntimeCall>>::Implicit;
    type Val = (
        <TxExtension as TransactionExtension<RuntimeCall>>::Val,
        Option<pallet_hashed_accounts::ValidatedProof>,
    );
    type Pre = <TxExtension as TransactionExtension<RuntimeCall>>::Pre;

    fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError> {
        self.0.implicit()
    }
    fn metadata() -> Vec<TransactionExtensionMetadata> {
        TxExtension::<false>::metadata()
    }
    fn weight(&self, call: &RuntimeCall) -> Weight {
        if let Some(address) = &self.1 {
            let len = u32::try_from(call.encoded_size()).unwrap_or(u32::MAX);
            let auth = match address.scheme {
                subtensor_hashed::Scheme::Sr25519 => {
                    <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize(len)
                }
                subtensor_hashed::Scheme::Ed25519 => {
                    <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize_ed25519(len)
                }
            };
            self.0.weight(call).saturating_add(auth)
        } else {
            // Preserve Frontier's ordinary signed-v4 fee policy.
            <Runtime as frame_system::Config>::DbWeight::get().reads(1)
        }
    }
    fn validate(
        &self,
        origin: RuntimeOrigin,
        call: &RuntimeCall,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
        implicit: Self::Implicit,
        implication: &impl Implication,
        source: TransactionSource,
    ) -> ValidateResult<Self::Val, RuntimeCall> {
        if let Some(address) = &self.1 {
            let account = AccountId::new(address.account);
            ensure!(
                frame_system::ensure_signed(origin.clone()).ok().as_ref() == Some(&account),
                InvalidTransaction::BadSigner
            );
            let ticket = crate::HashedAccounts::check_authorization(
                &account,
                address.scheme,
                &address.public_key,
                address.generation,
                address.next_commitment,
            )
            .map_err(|_| InvalidTransaction::BadProof)?;
            let (mut valid, val, origin) = authorized_extension(self.0.clone()).validate(
                origin,
                call,
                info,
                len,
                implicit,
                implication,
                source,
            )?;
            ensure!(
                frame_system::ensure_signed(origin.clone()).ok().as_ref() == Some(&account),
                InvalidTransaction::BadSigner
            );
            valid
                .provides
                .push((b"hashed-generation", &account, address.generation).encode());
            Ok((valid, (val, Some(ticket)), origin))
        } else {
            if let Ok(signer) = frame_system::ensure_signed(origin.clone()) {
                ensure!(
                    !crate::HashedAccounts::is_registered(&signer),
                    InvalidTransaction::BadSigner
                );
            }
            self.0
                .validate(origin, call, info, len, implicit, implication, source)
                .map(|(valid, val, origin)| (valid, (val, None), origin))
        }
    }
    fn prepare(
        self,
        (val, ticket): Self::Val,
        origin: &RuntimeOrigin,
        call: &RuntimeCall,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        if let Some(ticket) = ticket {
            crate::HashedAccounts::advance(&ticket).map_err(|_| InvalidTransaction::Stale)?;
            authorized_extension(self.0).prepare(val, origin, call, info, len)
        } else {
            self.0.prepare(val, origin, call, info, len)
        }
    }
    fn post_dispatch(
        pre: Self::Pre,
        info: &DispatchInfoOf<RuntimeCall>,
        post: &mut PostDispatchInfo,
        len: usize,
        result: &DispatchResult,
    ) -> Result<(), TransactionValidityError> {
        TxExtension::<false>::post_dispatch(pre, info, post, len, result)
    }
    fn bare_validate(
        call: &RuntimeCall,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> TransactionValidity {
        TxExtension::<false>::bare_validate(call, info, len)
    }
    fn bare_validate_and_prepare(
        call: &RuntimeCall,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> Result<(), TransactionValidityError> {
        TxExtension::<false>::bare_validate_and_prepare(call, info, len)
    }
    fn bare_post_dispatch(
        info: &DispatchInfoOf<RuntimeCall>,
        post: &mut PostDispatchInfo,
        len: usize,
        result: &DispatchResult,
    ) -> Result<(), TransactionValidityError> {
        TxExtension::<false>::bare_post_dispatch(info, post, len, result)
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
        let crate::UncheckedExtrinsic::Legacy(outer) = outer;
        let Preamble::Signed(address, _, _) = outer.0.preamble else {
            return Err(invalid);
        };
        let outer_account = match address {
            crate::Address::Raw(bytes) => AccountId::new(
                subtensor_hashed::SignerAddress::from_bytes(&bytes)
                    .map_err(|_| invalid)?
                    .account,
            ),
            address => {
                <Runtime as frame_system::Config>::Lookup::lookup(address).map_err(|_| invalid)?
            }
        };
        let call = outer.0.function;
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
        pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
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
        let extension = crate::TxExtension::<false>::decode(&mut &[0u8; 64][..])
            .map_err(|_| DispatchError::Other("HashedBenchmarkExtension"))?;
        let xt = crate::UncheckedExtrinsic::new_signed(
            call,
            sponsor.clone().into(),
            sp_runtime::MultiSignature::Sr25519(sp_core::sr25519::Signature::from_raw([0; 64])),
            extension,
        );
        // System's setter is std/test-only; benchmark setup also runs in Wasm.
        frame_support::storage::unhashed::put(
            sp_core::storage::well_known_keys::EXTRINSIC_INDEX,
            &0u32,
        );
        crate::System::note_extrinsic(xt.encode());
        Ok(())
    }
}

/// Alternative authorities must also use committed, rotating keys.
/// Ordinary accounts retain their existing ownership and proxy behavior.
pub(crate) fn compatible_authority(account: &AccountId, authority: &AccountId) -> bool {
    !crate::HashedAccounts::is_registered(account)
        || crate::HashedAccounts::is_registered(authority)
}

fn authority_satisfies_scheme(_: subtensor_hashed::Scheme, authority: &AccountId) -> bool {
    crate::HashedAccounts::is_registered(authority)
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
