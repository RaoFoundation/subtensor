//! Standard v4 signatures authenticate the rotating key; this wire-transparent
//! adapter applies nonce, fee and call policy to its permanent account.
use crate::{AccountId, Runtime, RuntimeCall, RuntimeOrigin, TxExtension};
use alloc::{boxed::Box, vec::Vec};
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
    fn register_mldsa65() -> Weight {
        TestVerificationWeight::get()
    }
    fn check_registered_mldsa65() -> Weight {
        TestVerificationWeight::get()
    }
    fn register_derived() -> Weight {
        TestVerificationWeight::get()
    }
    fn register_derived_sponsored() -> Weight {
        TestVerificationWeight::get()
    }
    fn check_derived() -> Weight {
        TestVerificationWeight::get()
    }
    fn install_capability() -> Weight {
        TestVerificationWeight::get()
    }
    fn revoke_capability() -> Weight {
        TestVerificationWeight::get()
    }
    fn authorize_mldsa65(_: u32) -> Weight {
        TestVerificationWeight::get()
    }
    fn authorize_mldsa65_invalid(_: u32) -> Weight {
        TestVerificationWeight::get()
    }
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

/// Wire-independent context, reconstructed exclusively from the signer address.
/// The large ML-DSA key is boxed to keep transaction-extension stack frames small.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AccountAuthorization {
    V1(subtensor_hashed::SignerAddress),
    MlDsa(Box<subtensor_hashed::mldsa::Header>),
}

impl AccountAuthorization {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, codec::Error> {
        match bytes.len() {
            subtensor_hashed::SignerAddress::ENCODED_LEN => {
                subtensor_hashed::SignerAddress::from_bytes(bytes).map(Self::V1)
            }
            subtensor_hashed::mldsa::SignerAddress::ENCODED_LEN => {
                let address = subtensor_hashed::mldsa::SignerAddress::from_bytes(bytes)?;
                Ok(Self::MlDsa(Box::new(address.header())))
            }
            _ => Err("Invalid hashed signer address length".into()),
        }
    }

    pub(crate) fn account(&self) -> AccountId {
        AccountId::new(match self {
            Self::V1(address) => address.account,
            Self::MlDsa(address) => address.account,
        })
    }

    fn generation(&self) -> u64 {
        match self {
            Self::V1(address) => address.generation,
            Self::MlDsa(address) => address.generation,
        }
    }
}

pub enum ValidatedAuthorization {
    V1(pallet_hashed_accounts::ValidatedProof),
    MlDsa(pallet_hashed_accounts::ValidatedMlDsaProof),
}

/// SCALE-transparent: the signed extension fields and metadata are unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking)]
pub struct AccountTxExtension(
    pub TxExtension,
    #[codec(skip)] pub(crate) Option<AccountAuthorization>,
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
        Option<ValidatedAuthorization>,
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
            let auth = match address {
                AccountAuthorization::V1(subtensor_hashed::SignerAddress {
                    scheme: subtensor_hashed::Scheme::Sr25519, ..
                }) => {
                    <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize(len)
                }
                AccountAuthorization::V1(subtensor_hashed::SignerAddress {
                    scheme: subtensor_hashed::Scheme::Ed25519, ..
                }) => {
                    <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize_ed25519(len)
                }
                AccountAuthorization::MlDsa(_) => {
                    <Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize_mldsa65(len)
                        .max(<Runtime as pallet_hashed_accounts::Config>::WeightInfo::authorize_mldsa65_invalid(len))
                }
            };
            self.0.weight(call).saturating_add(auth)
        } else {
            // Preserve Frontier's ordinary signed-v4 fee policy.
            <Runtime as frame_system::Config>::DbWeight::get().reads(3)
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
            let account = address.account();
            ensure!(
                frame_system::ensure_signed(origin.clone()).ok().as_ref() == Some(&account),
                InvalidTransaction::BadSigner
            );
            let ticket = match address {
                AccountAuthorization::V1(address) => crate::HashedAccounts::check_authorization(
                    &account,
                    address.scheme,
                    &address.public_key,
                    address.generation,
                    address.next_commitment,
                )
                .map(ValidatedAuthorization::V1),
                AccountAuthorization::MlDsa(address) => {
                    crate::HashedAccounts::check_mldsa_authorization(
                        &account,
                        address.algorithm,
                        &address.public_key,
                        address.generation,
                        address.next_commitment,
                    )
                    .map(ValidatedAuthorization::MlDsa)
                }
            }
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
                .push((b"hashed-generation", &account, address.generation()).encode());
            Ok((valid, (val, Some(ticket)), origin))
        } else {
            if let Ok(signer) = frame_system::ensure_signed(origin.clone()) {
                ensure!(
                    !crate::HashedAccounts::is_protected(&signer),
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
            match ticket {
                ValidatedAuthorization::V1(ticket) => crate::HashedAccounts::advance(&ticket),
                ValidatedAuthorization::MlDsa(ticket) => {
                    crate::HashedAccounts::advance_mldsa(&ticket)
                }
            }
            .map_err(|_| InvalidTransaction::Stale)?;
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

/// Reject, rather than silently erase, alternate control routes when installing
/// a new PQ identity. Every existing delegate must already have a permanent floor.
fn prepare_pq_authority(account: &AccountId) -> DispatchResult {
    use pallet_hashed_accounts::SecurityFloor;
    ensure!(
        pallet_subtensor::Owner::<Runtime>::try_get(account)
            .ok()
            .is_none_or(|owner| crate::HashedAccounts::authority_meets(
                &owner,
                SecurityFloor::PqRequired
            )),
        DispatchError::Other("MlDsaHotkeyRequiresPqOwner")
    );
    let (proxies, _) = crate::pallet_proxy::Proxies::<Runtime>::get(account);
    ensure!(
        proxies
            .iter()
            .all(|proxy| crate::HashedAccounts::authority_meets(
                &proxy.delegate,
                SecurityFloor::PqRequired
            )),
        DispatchError::Other("MlDsaRequiresPqDelegates")
    );
    ensure!(
        !pallet_subtensor::ColdkeySwapAnnouncements::<Runtime>::contains_key(account)
            && !pallet_subtensor::ColdkeySwapDisputes::<Runtime>::contains_key(account),
        DispatchError::Other("MlDsaPendingColdkeySwap")
    );
    ensure!(
        pallet_multisig::Multisigs::<Runtime>::iter_key_prefix(account)
            .next()
            .is_none(),
        DispatchError::Other("MlDsaPendingMultisig")
    );
    // A sponsor cannot erase another account's pending authorizations.
    for proxy in &proxies {
        ensure!(
            crate::Proxy::announcements(proxy.delegate.clone())
                .0
                .iter()
                .all(|announcement| announcement.real() != account),
            DispatchError::Other("MlDsaPendingProxyAnnouncement")
        );
    }
    crate::evm_origin::prepare_hashed_alias(account)
}

impl pallet_hashed_accounts::OnRegister for OnHashedRegistered {
    fn on_sponsored_deposit(
        account: &AccountId,
        payer: &AccountId,
        descriptor: &pallet_hashed_accounts::DerivedDescriptor,
    ) -> DispatchResult {
        use sp_runtime::generic::Preamble;
        let invalid = DispatchError::Other("InvalidDerivedDepositConsent");
        let index = crate::System::extrinsic_index().unwrap_or_default();
        // Check the encoded storage length before allocating a caller's entire
        // extrinsic. Sponsored setup has a separate, bounded consent envelope.
        use frame_support::{StorageHasher, traits::PalletInfoAccess};
        let mut key = frame_support::storage::storage_prefix(
            crate::System::name().as_bytes(),
            b"ExtrinsicData",
        )
        .to_vec();
        key.extend_from_slice(&frame_support::Twox64Concat::hash(&index.encode()));
        ensure!(
            sp_io::storage::read(&key, &mut [], 0).is_some_and(|len| len <= 64 * 1024 + 4),
            invalid
        );
        let encoded = crate::System::extrinsic_data(index);
        ensure!(encoded.len() <= 64 * 1024, invalid);
        let mut input = encoded.as_slice();
        let outer = crate::UncheckedExtrinsic::decode(&mut input).map_err(|_| invalid)?;
        ensure!(input.is_empty(), invalid);
        let crate::UncheckedExtrinsic::Legacy(outer) = outer;
        let Preamble::Signed(address, _, _) = outer.0.preamble else {
            return Err(invalid);
        };
        let outer_account = match address {
            crate::Address::Raw(bytes) => AccountAuthorization::from_bytes(&bytes)
                .map_err(|_| invalid)?
                .account(),
            address => {
                <Runtime as frame_system::Config>::Lookup::lookup(address).map_err(|_| invalid)?
            }
        };
        ensure!(
            &outer_account == payer
                && &descriptor.account_id() == account
                && commits_sponsored_registration(&outer.0.function, payer, descriptor, 0),
            invalid
        );
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_sponsored(
        _: &AccountId,
        payer: &AccountId,
        descriptor: &pallet_hashed_accounts::DerivedDescriptor,
    ) -> DispatchResult {
        let mut registration =
            RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register_derived_sponsored {
                descriptor: descriptor.clone(),
                deposit_payer: payer.clone(),
            });
        // Place the match at the final permitted depth and after the maximum
        // practical number of tiny calls. A single large remark would measure
        // decoding bytes but omit the recursive consent scan.
        for _ in 0..6 {
            registration = RuntimeCall::Utility(crate::pallet_utility::Call::batch {
                calls: alloc::vec![registration],
            });
        }
        let count = crate::Utility::batched_calls_limit()
            .saturating_sub(2)
            .min(18_000);
        let mut calls = alloc::vec![RuntimeCall::System(frame_system::Call::remark {
            remark: Vec::new(),
        }); count as usize];
        calls.push(RuntimeCall::System(frame_system::Call::remark {
            remark: Vec::new(),
        }));
        calls.push(registration);
        let extension = crate::TxExtension::<false>::decode(&mut &[0u8; 64][..])
            .map_err(|_| DispatchError::Other("DerivedBenchmarkExtension"))?;
        let make_outer = |calls| {
            crate::UncheckedExtrinsic::new_signed(
                RuntimeCall::Utility(crate::pallet_utility::Call::batch { calls }),
                payer.clone().into(),
                crate::Signature::Sr25519(sp_core::sr25519::Signature::from_raw([1; 64])),
                extension.clone(),
            )
        };
        let base = make_outer(calls.clone()).encoded_size();
        let padding = (64usize * 1024)
            .checked_sub(base.saturating_add(3))
            .ok_or(DispatchError::Other("DerivedBenchmarkEnvelopeTooLarge"))?;
        if let Some(RuntimeCall::System(frame_system::Call::remark { remark })) =
            calls.get_mut(count as usize)
        {
            *remark = alloc::vec![0; padding];
        }
        let outer = make_outer(calls);
        ensure!(
            outer.encoded_size() <= 64 * 1024 && outer.encoded_size() >= 64 * 1024 - 3,
            DispatchError::Other("DerivedBenchmarkEnvelopeSize")
        );
        frame_support::storage::unhashed::put(
            sp_core::storage::well_known_keys::EXTRINSIC_INDEX,
            &0u32,
        );
        crate::System::note_extrinsic(outer.encode());
        Ok(())
    }
    fn on_register_mldsa(
        account: &AccountId,
        sponsor: &AccountId,
        _: &subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        ensure!(
            account != sponsor,
            DispatchError::Other("InvalidMlDsaSponsor")
        );
        // FRAME authenticated the sponsor, including a normal multisig or proxy
        // origin. The descriptor determines the target; the sponsor gains no key.
        prepare_pq_authority(account)
    }

    fn on_register_derived(
        account: &AccountId,
        sponsor: &AccountId,
        descriptor: &pallet_hashed_accounts::DerivedDescriptor,
    ) -> DispatchResult {
        use pallet_hashed_accounts::DerivedDescriptor;
        // System account reaping cannot prove an identity is unused: stake,
        // contracts and application state may outlive its balance and nonce.
        let consent = match descriptor {
            DerivedDescriptor::Multisig { .. } => account == sponsor,
            DerivedDescriptor::Derivative { parent, .. } => account == sponsor || parent == sponsor,
            DerivedDescriptor::PureProxy { spawner, .. } => {
                account == sponsor || spawner == sponsor
            }
        };
        ensure!(
            consent,
            DispatchError::Other("DerivedProtectionRequiresOwner")
        );
        if let pallet_hashed_accounts::DerivedDescriptor::PureProxy {
            spawner,
            proxy_type,
            height,
            extrinsic_index,
            ..
        } = descriptor
        {
            let mut input = proxy_type.as_slice();
            let decoded = crate::ProxyType::decode(&mut input)
                .map_err(|_| DispatchError::Other("InvalidProtectedProxyType"))?;
            ensure!(
                input.is_empty() && decoded.encode().as_slice() == proxy_type.as_slice(),
                DispatchError::Other("InvalidProtectedProxyType")
            );
            // Sponsored pure identities must come from this extrinsic's actual
            // creation. An existing pure account can opt in through its own origin.
            if account != sponsor {
                ensure!(
                    spawner == sponsor
                        && *height == crate::System::block_number()
                        && *extrinsic_index == crate::System::extrinsic_index().unwrap_or_default()
                        && crate::Proxy::proxies(account.clone())
                            .0
                            .iter()
                            .any(|proxy| &proxy.delegate == spawner && proxy.proxy_type == decoded),
                    DispatchError::Other("InvalidProtectedProxyCreation")
                );
            }
        }
        prepare_pq_authority(account)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_mldsa(
        account: &AccountId,
        _: &AccountId,
        _: &subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        setup_pq_registration_benchmark(account)
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_derived(
        account: &AccountId,
        _: &AccountId,
        _: &pallet_hashed_accounts::DerivedDescriptor,
    ) -> DispatchResult {
        setup_pq_registration_benchmark(account)
    }

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
            crate::Address::Raw(bytes) => AccountAuthorization::from_bytes(&bytes)
                .map_err(|_| invalid)?
                .account(),
            address => {
                <Runtime as frame_system::Config>::Lookup::lookup(address).map_err(|_| invalid)?
            }
        };
        let call = outer.0.function;
        ensure!(&outer_account == sponsor, invalid);
        let matches_registration = |call: &RuntimeCall| {
            matches!(call,
                RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register { descriptor: subtensor_hashed::RegistrationDescriptor::Classical(descriptor) })
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
            descriptor: (*descriptor).into(),
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

fn commits_sponsored_registration(
    call: &RuntimeCall,
    payer: &AccountId,
    descriptor: &pallet_hashed_accounts::DerivedDescriptor,
    depth: u8,
) -> bool {
    if depth >= 8 {
        return false;
    }
    let nested =
        |call: &RuntimeCall| commits_sponsored_registration(call, payer, descriptor, depth + 1);
    match call {
        RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register_derived_sponsored {
            descriptor: committed,
            deposit_payer,
        }) => committed == descriptor && deposit_payer == payer,
        RuntimeCall::Utility(
            crate::pallet_utility::Call::batch { calls }
            | crate::pallet_utility::Call::batch_all { calls }
            | crate::pallet_utility::Call::force_batch { calls },
        ) => calls.iter().any(nested),
        RuntimeCall::Utility(
            crate::pallet_utility::Call::as_derivative { call, .. }
            | crate::pallet_utility::Call::as_protected_derivative { call, .. },
        ) => nested(call),
        RuntimeCall::Proxy(
            crate::pallet_proxy::Call::proxy { call, .. }
            | crate::pallet_proxy::Call::proxy_announced { call, .. },
        ) => nested(call),
        RuntimeCall::Multisig(
            pallet_multisig::Call::as_multi { call, .. }
            | pallet_multisig::Call::as_multi_threshold_1 { call, .. },
        ) => nested(call),
        _ => false,
    }
}

/// Alternative authorities must also use committed, rotating keys.
/// Ordinary accounts retain their existing ownership and proxy behavior.
pub(crate) fn compatible_authority(account: &AccountId, authority: &AccountId) -> bool {
    use pallet_hashed_accounts::{AuthenticationKind, SecurityFloor};
    let floor = match crate::HashedAccounts::authentication_kind(account) {
        AuthenticationKind::Ordinary => SecurityFloor::Unrestricted,
        AuthenticationKind::HashedV1 => SecurityFloor::Committed,
        AuthenticationKind::MlDsa | AuthenticationKind::DerivedPq => SecurityFloor::PqRequired,
    };
    crate::HashedAccounts::authority_meets(authority, floor)
}

fn authority_satisfies_scheme(_: subtensor_hashed::Scheme, authority: &AccountId) -> bool {
    crate::HashedAccounts::authority_meets(
        authority,
        pallet_hashed_accounts::SecurityFloor::Committed,
    )
}

/// Prevent weaker delegates from bypassing a protected account's policy.
pub struct HashedProxyPolicy;
impl frame_support::traits::Contains<(AccountId, AccountId)> for HashedProxyPolicy {
    fn contains((real, delegate): &(AccountId, AccountId)) -> bool {
        compatible_authority(real, delegate)
    }
}
impl crate::pallet_proxy::ProxyAccountPolicy<AccountId> for HashedProxyPolicy {
    fn requires_protection(account: &AccountId) -> bool {
        crate::HashedAccounts::authority_meets(
            account,
            pallet_hashed_accounts::SecurityFloor::PqRequired,
        )
    }
    fn on_create_protected_pure(
        pure: &AccountId,
        spawner: &AccountId,
        proxy_type_encoded: &[u8],
        index: u16,
        height_encoded: &[u8],
        extrinsic_index: u32,
    ) -> DispatchResult {
        let mut height_input = height_encoded;
        let height = u32::decode(&mut height_input)
            .map_err(|_| DispatchError::Other("InvalidProtectedProxyHeight"))?;
        ensure!(
            height_input.is_empty(),
            DispatchError::Other("InvalidProtectedProxyHeight")
        );
        let descriptor = pallet_hashed_accounts::DerivedDescriptor::PureProxy {
            spawner: spawner.clone(),
            proxy_type: proxy_type_encoded
                .to_vec()
                .try_into()
                .map_err(|_| DispatchError::Other("InvalidProtectedProxyType"))?,
            index,
            height,
            extrinsic_index,
        };
        ensure!(
            &descriptor.account_id() == pure,
            DispatchError::Other("InvalidProtectedProxyAccount")
        );
        crate::HashedAccounts::register_derived_with_sponsor(spawner, descriptor)
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn setup_protected_benchmark(account: &AccountId) -> Result<AccountId, DispatchError> {
        setup_pq_benchmark_graph(account)
    }
    fn weight() -> Weight {
        <Runtime as frame_system::Config>::DbWeight::get().reads(6)
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
        if crate::HashedAccounts::is_protected(real) {
            crate::Proxy::invalidate_announcements(real, delegate);
        }
    }
}

/// Utility descendants keep the parent's immutable PQ security floor.
pub struct HashedDerivedPolicy;
impl crate::pallet_utility::DerivedAccountPolicy<AccountId> for HashedDerivedPolicy {
    fn requires_protection(account: &AccountId) -> bool {
        crate::HashedAccounts::authority_meets(
            account,
            pallet_hashed_accounts::SecurityFloor::PqRequired,
        )
    }
    fn weight() -> Weight {
        crate::HashedAccounts::protected_query_weight()
    }
    fn on_protected_derivative(
        parent: &AccountId,
        child: &AccountId,
        index: u16,
    ) -> DispatchResult {
        let descriptor = pallet_hashed_accounts::DerivedDescriptor::Derivative {
            parent: parent.clone(),
            index,
        };
        ensure!(
            &descriptor.account_id() == child,
            DispatchError::Other("InvalidProtectedDerivative")
        );
        crate::HashedAccounts::register_derived_with_sponsor(parent, descriptor)
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn setup_protected_benchmark(parent: &AccountId) -> Result<AccountId, DispatchError> {
        setup_pq_benchmark_graph(parent)
    }
}

/// A valid depth-15, 1023-node parent makes a new child reach both protocol
/// bounds. Every cached record is derived from an already established PQ leaf.
#[cfg(feature = "runtime-benchmarks")]
fn setup_pq_benchmark_graph(marker: &AccountId) -> Result<AccountId, DispatchError> {
    use frame_support::traits::Currency;
    use pallet_hashed_accounts::{DerivedAuthorities, DerivedAuthorityRecord, DerivedDescriptor};
    let seed = sp_io::hashing::blake2_256(&(b"pq/benchmark/leaf", marker).encode());
    let public =
        subtensor_hashed::mldsa::public_key_from_seed(&seed).map_err(DispatchError::Other)?;
    let descriptor = subtensor_hashed::mldsa::Descriptor {
        version: 2,
        algorithm: 4,
        initial_commitment: subtensor_hashed::mldsa::commitment(&public),
    };
    let leaf = AccountId::new(subtensor_hashed::mldsa::account_id(&descriptor));
    pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);

    pallet_hashed_accounts::MlDsaAccounts::<Runtime>::insert(
        &leaf,
        pallet_hashed_accounts::MlDsaAccountRecord {
            descriptor,
            generation: 0,
            commitment: descriptor.initial_commitment,
        },
    );
    let mut members = Vec::new();
    for member in 0u16..100 {
        let mut parent = leaf.clone();
        let length = if member == 99 {
            14u8
        } else if member < 17 {
            10
        } else {
            9
        };
        for depth in 1u8..=length {
            let child = DerivedDescriptor::Derivative {
                parent: parent.clone(),
                index: member.saturating_mul(16).saturating_add(u16::from(depth)),
            };
            let account = child.account_id();
            let dependencies = alloc::vec![parent]
                .try_into()
                .map_err(|_| DispatchError::Other("BenchmarkProofBound"))?;
            DerivedAuthorities::<Runtime>::insert(
                &account,
                DerivedAuthorityRecord {
                    descriptor: child,
                    pq_dependencies: dependencies,
                    depth,
                    nodes: u32::from(depth).saturating_add(1),
                },
            );
            parent = account;
        }
        members.push(parent);
    }
    members.sort();
    let members: pallet_hashed_accounts::DerivedSignatories = members
        .try_into()
        .map_err(|_| DispatchError::Other("BenchmarkMembersBound"))?;
    let descriptor = DerivedDescriptor::Multisig {
        threshold: 100,
        signatories: members.clone(),
    };
    let parent = descriptor.account_id();
    DerivedAuthorities::<Runtime>::insert(
        &parent,
        DerivedAuthorityRecord {
            descriptor,
            pq_dependencies: members,
            depth: 15,
            nodes: 1023,
        },
    );
    let _ = crate::Balances::make_free_balance_be(
        &parent,
        crate::TaoBalance::new(1_000_000_000_000_000),
    );
    Ok(parent)
}

/// Maximum valid alternate-authority scans and the nonempty alias sweep. The
/// announcements intentionally target another real account; target-specific
/// pending approvals would correctly make registration fail instead.
#[cfg(feature = "runtime-benchmarks")]
fn setup_pq_registration_benchmark(account: &AccountId) -> DispatchResult {
    use frame_support::traits::Currency;
    use pallet_evm::AddressMapping;
    use pallet_hashed_accounts::{DerivedAuthorities, DerivedDescriptor};
    let owner = setup_pq_benchmark_graph(account)?;
    pallet_subtensor::Owner::<Runtime>::insert(account, &owner);
    let _ = crate::Balances::make_free_balance_be(
        account,
        crate::TaoBalance::new(1_000_000_000_000_000),
    );
    let template = DerivedAuthorities::<Runtime>::get(&owner)
        .ok_or(DispatchError::Other("BenchmarkGraphMissing"))?;
    for index in 0..crate::MaxProxies::get() {
        let mut record = template.clone();
        if let DerivedDescriptor::Multisig { threshold, .. } = &mut record.descriptor {
            *threshold = u16::try_from(index.saturating_add(1))
                .map_err(|_| DispatchError::Other("BenchmarkProxyBound"))?;
        }
        let delegate = record.descriptor.account_id();
        DerivedAuthorities::<Runtime>::insert(&delegate, record);
        let _ = crate::Balances::make_free_balance_be(
            &delegate,
            crate::TaoBalance::new(1_000_000_000_000_000),
        );
        crate::Proxy::add_proxy_delegate(account, delegate.clone(), crate::ProxyType::Any, 1)?;
        let unrelated = AccountId::new(sp_io::hashing::blake2_256(
            &(b"pq/benchmark/announcement", account, index).encode(),
        ));
        let _ = crate::Balances::make_free_balance_be(
            &unrelated,
            crate::TaoBalance::new(1_000_000_000_000_000),
        );
        crate::Proxy::add_proxy_delegate(&unrelated, delegate.clone(), crate::ProxyType::Any, 1)?;
        for pending in 0..crate::MaxPending::get() {
            let hash = sp_core::H256::from(sp_io::hashing::blake2_256(&(index, pending).encode()));
            crate::Proxy::announce(
                RuntimeOrigin::signed(delegate.clone()),
                unrelated.clone().into(),
                hash,
            )?;
        }
    }
    let alias = sp_core::H160::from(crate::HashedAccounts::evm_alias(account));
    let backing =
        pallet_evm::HashedAddressMapping::<sp_runtime::traits::BlakeTwo256>::into_account_id(alias);
    let _ = crate::Balances::make_free_balance_be(&backing, crate::TaoBalance::new(1_000_000_000));
    Ok(())
}

#[cfg(feature = "runtime-benchmarks")]
pub(crate) fn setup_pq_benchmark_parent(account: &AccountId) -> DispatchResult {
    use frame_support::traits::Currency;
    let _ = crate::Balances::make_free_balance_be(
        account,
        crate::TaoBalance::new(1_000_000_000_000_000),
    );
    let descriptor = subtensor_hashed::mldsa::Descriptor {
        version: 2,
        algorithm: 4,
        initial_commitment: [7; 64],
    };
    pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);

    pallet_hashed_accounts::MlDsaAccounts::<Runtime>::insert(
        account,
        pallet_hashed_accounts::MlDsaAccountRecord {
            descriptor,
            generation: 0,
            commitment: descriptor.initial_commitment,
        },
    );
    Ok(())
}
