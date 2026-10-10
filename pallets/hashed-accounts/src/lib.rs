//! Permanent, versioned authorization records for mnemonic-derived rotating keys.
//!
//! A runtime must reject legacy signatures and other signature-only authority
//! routes for registered accounts. Registration must precede receiving funds or
//! assigning privileged roles; an arbitrary unregistered AccountId32 hash can
//! also be a valid classical public-key encoding.
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
pub use subtensor_hashed::{Descriptor, Proof, Scheme};
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod tests;
pub mod weights;

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

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark(_: &AccountId32, _: &AccountId32, _: &Descriptor) -> DispatchResult {
        Ok(())
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
        /// Current registration and authorization switch, read from runtime storage.
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
        /// A fixed-key account consumed an authorization sequence number.
        Authorized {
            account: AccountId32,
            generation: u64,
        },
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
        /// The next commitment violates the account mode (rotate or retain the key).
        InvalidNextCommitment,
        /// The key generation counter cannot advance any further.
        GenerationExhausted,
        /// The signature does not authorize this account and complete transaction.
        InvalidSignature,
        /// Another account already owns the derived EVM alias.
        AliasCollision,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Bind a derived account to its hidden initial key. Anyone may sponsor
        /// the permanent storage deposit; the sponsor gains no authority.
        /// Idempotent registration never changes an existing authorization.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::register())]
        #[transactional]
        pub fn register(origin: OriginFor<T>, descriptor: Descriptor) -> DispatchResult {
            let sponsor = ensure_signed(origin)?;
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
            let alias = Self::evm_alias(&account);
            ensure!(
                EvmAliases::<T>::get(alias).is_none_or(|existing| existing == account),
                Error::<T>::AliasCollision
            );
            T::OnRegister::on_register(&account, &sponsor, &descriptor)?;
            let deposit = T::RegistrationDeposit::get();
            T::Currency::reserve(&sponsor, deposit)?;
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
                sponsor,
                deposit,
            });
            Ok(())
        }

        /// Check a recipient's registration without reserving funds or creating
        /// an account. An atomic payment must fail if a reorg removed its setup.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::check_registered())]
        pub fn check_registered(origin: OriginFor<T>, descriptor: Descriptor) -> DispatchResult {
            ensure_signed(origin)?;
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
    }

    impl<T: Config> Pallet<T> {
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
        pub fn check_proof<const P: usize, const S: usize>(
            account: &AccountId32,
            proof: &subtensor_hashed::AuthorizationProof<P, S>,
            implication: &[u8],
        ) -> Result<ValidatedProof, DispatchError> {
            ensure!(T::Enabled::get(), Error::<T>::Disabled);
            let record = Accounts::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
            ensure!(
                record.descriptor.is_supported(),
                Error::<T>::UnsupportedDescriptor
            );
            ensure!(
                record.generation != u64::MAX,
                Error::<T>::GenerationExhausted
            );
            ensure!(
                record.generation == proof.generation,
                Error::<T>::WrongGeneration
            );
            ensure!(
                record.commitment
                    == subtensor_hashed::key_commitment(
                        record.descriptor.scheme,
                        &proof.public_key
                    ),
                Error::<T>::WrongCommitment
            );
            ensure!(
                proof.next_commitment != [0; 32]
                    && if record.descriptor.rotates() {
                        proof.next_commitment != record.commitment
                    } else {
                        proof.next_commitment == record.commitment
                    },
                Error::<T>::InvalidNextCommitment
            );
            ensure!(
                subtensor_hashed::verify(
                    account.as_ref(),
                    record.descriptor.scheme,
                    proof,
                    implication
                ),
                Error::<T>::InvalidSignature
            );
            Ok(ValidatedProof {
                account: account.clone(),
                generation: proof.generation,
                commitment: record.commitment,
                next_commitment: proof.next_commitment,
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
            let event = if validated.next_commitment == validated.commitment {
                Event::Authorized {
                    account: validated.account.clone(),
                    generation,
                }
            } else {
                Event::Rotated {
                    account: validated.account.clone(),
                    generation,
                }
            };
            Self::deposit_event(event);
            Ok(())
        }
    }
}
