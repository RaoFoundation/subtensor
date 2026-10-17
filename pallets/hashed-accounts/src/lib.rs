//! Permanent, versioned authorization records for mnemonic-derived rotating keys.
//!
//! A runtime must reject legacy signatures and other signature-only authority
//! routes for registered accounts. Registration must precede receiving funds or
//! assigning privileged roles; an arbitrary unregistered AccountId32 hash can
//! also be a valid classical public-key encoding.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use subtensor_hashed::{Descriptor, Proof, RegistrationDescriptor, Scheme};
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod mldsa;
#[cfg(test)]
mod mldsa_tests;
#[cfg(test)]
mod tests;
pub mod weights;
pub use mldsa::*;
pub mod capabilities;
pub use capabilities::*;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{
    dispatch::DispatchResult,
    traits::{Currency, ReservableCurrency},
};
use scale_info::TypeInfo;
use sp_runtime::AccountId32;
use weights::WeightInfo;

pub type BalanceOf<T> = <<T as Config>::Currency as Currency<AccountId32>>::Balance;

/// Runtime-specific guard for incompatible roles or preexisting authorization.
/// It must not trust the sponsor to control the newly derived account. For
/// example, reject preexisting EVM aliases and authority assignments that would
/// provide a second way to authorize this account. Called transactionally.
pub trait OnRegister {
    fn on_register(
        account: &AccountId32,
        sponsor: &AccountId32,
        descriptor: &Descriptor,
    ) -> DispatchResult;

    /// Validate registration context, preexisting authorities, and prepare the
    /// permanent EVM alias. A runtime must implement this before enabling ML-DSA.
    fn on_register_mldsa(
        _: &AccountId32,
        _: &AccountId32,
        _: &subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        Err(sp_runtime::DispatchError::Other(
            "MlDsaRegistrationUnsupported",
        ))
    }

    /// Validate the actual derived dispatch/creation context and every existing
    /// control path. Sponsors cannot erase existing privileges to upgrade them.
    fn on_register_derived(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        Err(sp_runtime::DispatchError::Other(
            "DerivedRegistrationUnsupported",
        ))
    }

    /// Verify explicit deposit consent by the authenticated outer signer. The
    /// derived owner must independently authorize registration through its own
    /// dispatch origin; sponsorship never establishes account authority.
    fn on_sponsored_deposit(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        Err(sp_runtime::DispatchError::Other(
            "DerivedSponsorshipUnsupported",
        ))
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark(_: &AccountId32, _: &AccountId32, _: &Descriptor) -> DispatchResult {
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_mldsa(
        _: &AccountId32,
        _: &AccountId32,
        _: &subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        Err(sp_runtime::DispatchError::Other(
            "MlDsaBenchmarkUnsupported",
        ))
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_derived(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        Err(sp_runtime::DispatchError::Other(
            "DerivedBenchmarkUnsupported",
        ))
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_sponsored(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        Err(sp_runtime::DispatchError::Other(
            "DerivedSponsorshipBenchmarkUnsupported",
        ))
    }
}

impl OnRegister for () {
    fn on_register(_: &AccountId32, _: &AccountId32, _: &Descriptor) -> DispatchResult {
        Ok(())
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct AccountRecord {
    pub descriptor: Descriptor,
    pub generation: u64,
    pub commitment: [u8; 32],
}

/// An in-memory capability returned only after signature validation. The
/// extension carries it to preparation, which rechecks its state prerequisites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedProof {
    account: AccountId32,
    generation: u64,
    commitment: [u8; 32],
    next_commitment: [u8; 32],
}

#[frame_support::pallet]
#[allow(clippy::expect_used)] // FRAME-generated storage and metadata code.
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, transactional};
    use frame_system::pallet_prelude::*;

    #[pallet::config]
    pub trait Config:
        frame_system::Config<AccountId = AccountId32, RuntimeEvent: From<Event<Self>>>
    {
        type Currency: ReservableCurrency<AccountId32>;
        #[pallet::constant]
        type RegistrationDeposit: Get<BalanceOf<Self>>;
        #[pallet::constant]
        type MlDsaRegistrationDeposit: Get<BalanceOf<Self>>;
        #[pallet::constant]
        type DerivedRegistrationDeposit: Get<BalanceOf<Self>>;
        /// Master registration and authorization switch for all hashed and ML-DSA accounts.
        /// Permanent identity and authority protections survive disabling it.
        type Enabled: Get<bool>;
        type OnRegister: OnRegister;
        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Permanent even when the balance becomes zero; never reset on re-registration.
    #[pallet::storage]
    #[pallet::getter(fn accounts)]
    pub type Accounts<T: Config> = StorageMap<_, Blake2_128Concat, AccountId32, AccountRecord>;

    /// Native hashed-account EVM addresses resolve to the permanent account.
    #[pallet::storage]
    pub type EvmAliases<T: Config> = StorageMap<_, Blake2_128Concat, [u8; 20], AccountId32>;

    /// Separate storage preserves every v1 record's encoded representation.
    #[pallet::storage]
    #[pallet::getter(fn mldsa_accounts)]
    pub type MlDsaAccounts<T: Config> =
        StorageMap<_, Blake2_128Concat, AccountId32, MlDsaAccountRecord>;

    /// Immutable protection, including after a pure proxy is killed.
    #[pallet::storage]
    #[pallet::getter(fn derived_authorities)]
    pub type DerivedAuthorities<T: Config> =
        StorageMap<_, Blake2_128Concat, AccountId32, DerivedAuthorityRecord>;

    #[pallet::storage]
    pub type CapabilityEpochs<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        AccountId32,
        Twox64Concat,
        CapabilityRole,
        u64,
        ValueQuery,
    >;

    #[pallet::storage]
    pub type ActiveCapabilities<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        AccountId32,
        Twox64Concat,
        CapabilityRole,
        CapabilityRecord,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        Registered {
            account: AccountId32,
            sponsor: AccountId32,
            deposit: BalanceOf<T>,
        },
        Rotated {
            account: AccountId32,
            generation: u64,
        },
        RegisteredMlDsa {
            account: AccountId32,
            sponsor: AccountId32,
            deposit: BalanceOf<T>,
        },
        RotatedMlDsa {
            account: AccountId32,
            generation: u64,
        },
        DerivedProtected {
            account: AccountId32,
            sponsor: AccountId32,
            deposit: BalanceOf<T>,
        },
        CapabilityInstalled {
            owner: AccountId32,
            role: CapabilityRole,
            epoch: u64,
        },
        CapabilityRevoked {
            owner: AccountId32,
            role: CapabilityRole,
            epoch: u64,
        },
        /// A fixed-key ML-DSA authorization consumed its replay counter.
        #[codec(index = 8)]
        AuthorizedFixed {
            account: AccountId32,
            generation: u64,
        },
    }

    #[pallet::extra_constants]
    impl<T: Config> Pallet<T> {
        /// Supported ML-DSA account profiles: rotating and fixed-key.
        #[pallet::constant_name(MldsaAccountProfiles)]
        pub fn mldsa_account_profiles() -> [u8; 2] {
            [
                subtensor_hashed::mldsa::ALGORITHM,
                subtensor_hashed::mldsa::FIXED_ALGORITHM,
            ]
        }
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Hashed account registration or authorization is disabled in this runtime.
        Disabled,
        /// The descriptor version, scheme, or initial commitment is unsupported.
        UnsupportedDescriptor,
        /// The derived account is already registered with a different descriptor.
        DescriptorMismatch,
        /// Register the hashed account before submitting its authorization proof.
        NotRegistered,
        /// The proof does not use the account's currently active key generation.
        WrongGeneration,
        /// The revealed public key does not match the active key commitment.
        WrongCommitment,
        /// The next commitment must obey the immutable account rotation policy.
        InvalidNextCommitment,
        /// The key generation counter cannot advance any further.
        GenerationExhausted,
        /// The signature does not authorize this account and complete transaction.
        InvalidSignature,
        /// Another account already owns the derived EVM alias.
        AliasCollision,
        /// A different permanent authentication kind already owns this identity.
        AuthenticationKindConflict,
        /// A derived descriptor is invalid or its dependencies are not protected.
        InvalidDerivedAuthority,
        /// The derived authority graph exceeds a deterministic runtime bound.
        DerivedAuthorityTooComplex,
        CapabilityInvalid,
        CapabilityEpochMismatch,
        CapabilityExpired,
        CapabilityScopeViolation,
        CapabilityMissing,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::install_capability())]
        #[transactional]
        pub fn install_capability(
            origin: OriginFor<T>,
            expected_epoch: u64,
            key_commitment: [u8; 64],
            expires_at: u64,
            scope: CapabilityScope,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_install_capability(&owner, expected_epoch, key_commitment, expires_at, scope)
        }

        #[pallet::call_index(8)]
        #[pallet::weight(T::WeightInfo::revoke_capability())]
        #[transactional]
        pub fn revoke_capability(origin: OriginFor<T>, role: CapabilityRole) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_revoke_capability(&owner, role)
        }

        /// Bind an account to its hidden initial key for the selected scheme.
        /// Anyone may sponsor the permanent deposit without gaining authority.
        /// Idempotent registration never changes an existing authorization.
        #[pallet::call_index(0)]
        #[pallet::weight(match descriptor {
            RegistrationDescriptor::Classical(_) => T::WeightInfo::register(),
            RegistrationDescriptor::MlDsa65(_) => T::WeightInfo::register_mldsa65(),
        })]
        #[transactional]
        pub fn register(
            origin: OriginFor<T>,
            descriptor: RegistrationDescriptor,
        ) -> DispatchResult {
            let sponsor = ensure_signed(origin)?;
            match descriptor {
                RegistrationDescriptor::Classical(descriptor) => {
                    Self::do_register_classical(&sponsor, descriptor)
                }
                RegistrationDescriptor::MlDsa65(descriptor) => {
                    Self::do_register_mldsa(&sponsor, descriptor)
                }
            }
        }

        /// Check a recipient's registration without reserving funds or creating
        /// an account. An atomic payment must fail if a reorg removed its setup.
        #[pallet::call_index(1)]
        #[pallet::weight(match descriptor {
            RegistrationDescriptor::Classical(_) => T::WeightInfo::check_registered(),
            RegistrationDescriptor::MlDsa65(_) => T::WeightInfo::check_registered_mldsa65(),
        })]
        pub fn check_registered(
            origin: OriginFor<T>,
            descriptor: RegistrationDescriptor,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            match descriptor {
                RegistrationDescriptor::Classical(descriptor) => {
                    Self::do_check_registered_classical(descriptor)
                }
                RegistrationDescriptor::MlDsa65(descriptor) => {
                    Self::do_check_registered_mldsa65(descriptor)
                }
            }
        }

        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::register_derived())]
        #[transactional]
        pub fn register_derived(
            origin: OriginFor<T>,
            descriptor: DerivedDescriptor,
        ) -> DispatchResult {
            let sponsor = ensure_signed(origin)?;
            Self::register_derived_with_sponsor(&sponsor, descriptor)
        }

        /// Register through the derived account's own authority while an
        /// explicitly consenting outer signer pays the permanent deposit.
        #[pallet::call_index(9)]
        #[pallet::weight(T::WeightInfo::register_derived_sponsored())]
        #[transactional]
        pub fn register_derived_sponsored(
            origin: OriginFor<T>,
            descriptor: DerivedDescriptor,
            deposit_payer: AccountId32,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            ensure!(
                owner == descriptor.account_id(),
                Error::<T>::InvalidDerivedAuthority
            );
            Self::register_derived_with_authorization(&owner, &deposit_payer, descriptor)
        }

        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::check_derived())]
        pub fn check_derived(
            origin: OriginFor<T>,
            descriptor: DerivedDescriptor,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            let account = descriptor.account_id();
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            let record = DerivedAuthorities::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
            ensure!(
                record.descriptor == descriptor,
                Error::<T>::DescriptorMismatch
            );
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        fn do_register_classical(sponsor: &AccountId32, descriptor: Descriptor) -> DispatchResult {
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            ensure!(descriptor.is_supported(), Error::<T>::UnsupportedDescriptor);
            let account = AccountId32::new(subtensor_hashed::account_id(&descriptor));
            if let Some(record) = Accounts::<T>::get(&account) {
                ensure!(
                    record.descriptor == descriptor,
                    Error::<T>::DescriptorMismatch
                );
                return Ok(());
            }
            ensure!(
                !MlDsaAccounts::<T>::contains_key(&account)
                    && !DerivedAuthorities::<T>::contains_key(&account),
                Error::<T>::AuthenticationKindConflict
            );
            let alias = Self::evm_alias(&account);
            ensure!(
                EvmAliases::<T>::get(alias).is_none_or(|existing| existing == account),
                Error::<T>::AliasCollision
            );
            T::OnRegister::on_register(&account, sponsor, &descriptor)?;
            let deposit = T::RegistrationDeposit::get();
            T::Currency::reserve(sponsor, deposit)?;
            frame_system::Pallet::<T>::inc_providers(&account);
            // Keep the EVM-visible account nonce nonzero (EOA, never CREATE
            // destination); key generation remains independent of that nonce.
            if frame_system::Pallet::<T>::account_nonce(&account) == Default::default() {
                frame_system::Pallet::<T>::inc_account_nonce(&account);
            }
            Accounts::<T>::insert(
                &account,
                AccountRecord {
                    descriptor,
                    generation: 0,
                    commitment: descriptor.initial_commitment,
                },
            );
            EvmAliases::<T>::insert(alias, &account);
            Self::deposit_event(Event::Registered {
                account,
                sponsor: sponsor.clone(),
                deposit,
            });
            Ok(())
        }

        fn do_check_registered_classical(descriptor: Descriptor) -> DispatchResult {
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            ensure!(descriptor.is_supported(), Error::<T>::UnsupportedDescriptor);
            let account = AccountId32::new(subtensor_hashed::account_id(&descriptor));
            let record = Accounts::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
            ensure!(
                record.descriptor == descriptor,
                Error::<T>::DescriptorMismatch
            );
            Ok(())
        }

        fn do_check_registered_mldsa65(
            descriptor: subtensor_hashed::mldsa::Descriptor,
        ) -> DispatchResult {
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            ensure!(descriptor.is_supported(), Error::<T>::UnsupportedDescriptor);
            let account = AccountId32::new(subtensor_hashed::mldsa::account_id(&descriptor));
            let record = MlDsaAccounts::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
            ensure!(
                record.descriptor == descriptor,
                Error::<T>::DescriptorMismatch
            );
            Ok(())
        }

        pub fn evm_alias(account: &AccountId32) -> [u8; 20] {
            let bytes: &[u8; 32] = account.as_ref();
            let mut alias = [0; 20];
            for (target, source) in alias.iter_mut().zip(bytes.iter()) {
                *target = *source;
            }
            alias
        }

        pub fn is_registered(account: &AccountId32) -> bool {
            Accounts::<T>::contains_key(account)
        }

        /// Read-only transaction validation. This never advances the key.
        /// Check the already authenticated v4 signer against the hidden commitment.
        pub fn check_authorization(
            account: &AccountId32,
            scheme: Scheme,
            public_key: &[u8; 32],
            generation: u64,
            next_commitment: [u8; 32],
        ) -> Result<ValidatedProof, DispatchError> {
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            let record = Accounts::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
            ensure!(
                record.descriptor.is_supported() && record.descriptor.scheme == scheme,
                Error::<T>::UnsupportedDescriptor
            );
            ensure!(
                record.generation != u64::MAX,
                Error::<T>::GenerationExhausted
            );
            ensure!(record.generation == generation, Error::<T>::WrongGeneration);
            ensure!(
                record.commitment == subtensor_hashed::key_commitment(scheme, public_key),
                Error::<T>::WrongCommitment
            );
            ensure!(
                next_commitment != [0; 32] && next_commitment != record.commitment,
                Error::<T>::InvalidNextCommitment
            );
            Ok(ValidatedProof {
                account: account.clone(),
                generation,
                commitment: record.commitment,
                next_commitment,
            })
        }

        /// Called by the authorization extension in `prepare`, after all
        /// validation succeeds and before dispatch. The advance must remain
        /// consumed if the inner call fails. Never call from the inner call's
        /// transactional scope, and never roll it back in post-dispatch.
        pub fn advance(validated: &ValidatedProof) -> DispatchResult {
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            let generation = validated
                .generation
                .checked_add(1)
                .ok_or(Error::<T>::GenerationExhausted)?;
            Accounts::<T>::try_mutate(&validated.account, |maybe_record| -> DispatchResult {
                let record = maybe_record.as_mut().ok_or(Error::<T>::NotRegistered)?;
                ensure!(
                    record.generation == validated.generation,
                    Error::<T>::WrongGeneration
                );
                ensure!(
                    record.commitment == validated.commitment,
                    Error::<T>::WrongCommitment
                );
                record.generation = generation;
                record.commitment = validated.next_commitment;
                Ok(())
            })?;
            Self::deposit_event(Event::Rotated {
                account: validated.account.clone(),
                generation,
            });
            Ok(())
        }
    }
}
