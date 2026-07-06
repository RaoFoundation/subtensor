// pallets/mev-shield/src/lib.rs
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use frame_support::{
    dispatch::{GetDispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{ConstU64, IsSubType},
};
use frame_system::{ensure_none, ensure_root, ensure_signed, pallet_prelude::*};
use ml_kem::{
    Ciphertext, EncodedSizeUser, MlKem768, MlKem768Params,
    kem::{Decapsulate, DecapsulationKey},
};
use sp_io::hashing::twox_128;
use sp_runtime::traits::{Applyable, Block as BlockT, Checkable, Hash};
use sp_runtime::traits::{Dispatchable, Saturating};
use stp_shield::{
    INHERENT_IDENTIFIER, InherentType, LOG_TARGET, MLKEM768_ENC_KEY_LEN, ShieldEncKey,
    ShieldedTransaction,
};
use subtensor_macros::freeze_struct;

pub use pallet::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod weights;
pub use weights::WeightInfo;

#[cfg(test)]
pub mod mock;

#[cfg(test)]
mod tests;

mod extension;
mod migrations;
pub use extension::CheckShieldedTxValidity;

type MigrationKeyMaxLen = ConstU32<128>;

type ExtrinsicOf<Block> = <Block as BlockT>::Extrinsic;
type CheckedOf<T, Context> = <T as Checkable<Context>>::Checked;
type ApplyableCallOf<T> = <T as Applyable>::Call;

const MAX_EXTRINSIC_DEPTH: u32 = 8;

/// Weight for `store_encrypted`, intentionally set higher than the benchmark
/// to discourage abuse of the encrypted extrinsic queue.
const STORE_ENCRYPTED_WEIGHT: u64 = 20_000_000_000;

pub fn store_encrypted_weight() -> Weight {
    Weight::from_parts(STORE_ENCRYPTED_WEIGHT, 0)
}

/// Trait for decrypting stored extrinsics before dispatch.
pub trait ExtrinsicDecryptor<RuntimeCall> {
    /// Decrypt the stored bytes and return the decoded RuntimeCall.
    fn decrypt(data: &[u8]) -> Result<RuntimeCall, DispatchError>;
}

/// Default implementation that always returns an error.
impl<RuntimeCall> ExtrinsicDecryptor<RuntimeCall> for () {
    fn decrypt(_data: &[u8]) -> Result<RuntimeCall, DispatchError> {
        Err(DispatchError::Other("ExtrinsicDecryptor not implemented"))
    }
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use crate::weights::WeightInfo;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The identifier type for an authority.
        type AuthorityId: Member + Parameter + MaybeSerializeDeserialize + MaxEncodedLen;

        /// A way to find the current and next block author.
        type FindAuthors: FindAuthors<Self>;

        /// The overarching call type for dispatching stored extrinsics.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo;

        /// Decryptor for stored extrinsics.
        type ExtrinsicDecryptor: ExtrinsicDecryptor<<Self as pallet::Config>::RuntimeCall>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Current block author's ML-KEM-768 encapsulation key (internal, not for encryption).
    #[pallet::storage]
    pub type CurrentKey<T> = StorageValue<_, ShieldEncKey, OptionQuery>;

    /// Next block author's key, staged here before promoting to `CurrentKey`.
    #[pallet::storage]
    pub type PendingKey<T> = StorageValue<_, ShieldEncKey, OptionQuery>;

    /// Key users should encrypt with (N+2 author's key).
    #[pallet::storage]
    pub type NextKey<T> = StorageValue<_, ShieldEncKey, OptionQuery>;

    /// Per-author ML-KEM-768 encapsulation key, updated each time the author produces a block.
    #[pallet::storage]
    pub type AuthorKeys<T: Config> =
        StorageMap<_, Twox64Concat, T::AuthorityId, ShieldEncKey, OptionQuery>;

    /// Block number at which `PendingKey` is no longer valid (exclusive upper bound).
    /// Updated every block during rotation.
    #[pallet::storage]
    pub type PendingKeyExpiresAt<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;

    /// Block number at which `NextKey` is no longer valid (exclusive upper bound).
    /// Updated every block during rotation.
    #[pallet::storage]
    pub type NextKeyExpiresAt<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;

    /// Stores whether some migration has been run.
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    /// Maximum size of a single encoded call.
    pub type MaxEncryptedCallSize = ConstU32<8192>;

    /// Default maximum number of pending extrinsics.
    pub type DefaultMaxPendingExtrinsics = ConstU32<100>;

    /// Configurable maximum number of pending extrinsics.
    /// Defaults to 100 if not explicitly set via `set_max_pending_extrinsics`.
    #[pallet::storage]
    pub type MaxPendingExtrinsicsLimit<T: Config> =
        StorageValue<_, u32, ValueQuery, DefaultMaxPendingExtrinsics>;

    /// Default extrinsic lifetime in blocks.
    pub const DEFAULT_EXTRINSIC_LIFETIME: u32 = 10;

    /// Configurable extrinsic lifetime (max block difference between submission and execution).
    /// Defaults to 10 blocks if not explicitly set.
    #[pallet::storage]
    pub type ExtrinsicLifetime<T: Config> =
        StorageValue<_, u32, ValueQuery, ConstU32<DEFAULT_EXTRINSIC_LIFETIME>>;

    /// Default maximum weight allowed for on_initialize processing.
    pub const DEFAULT_ON_INITIALIZE_WEIGHT: u64 = 500_000_000_000;

    /// Absolute maximum weight for on_initialize: half the total block weight (2s of 4s).
    pub const MAX_ON_INITIALIZE_WEIGHT: u64 = 2_000_000_000_000;

    /// Configurable maximum weight for on_initialize processing.
    /// Defaults to 500_000_000_000 ref_time if not explicitly set.
    #[pallet::storage]
    pub type OnInitializeWeight<T: Config> =
        StorageValue<_, u64, ValueQuery, ConstU64<DEFAULT_ON_INITIALIZE_WEIGHT>>;

    /// Default maximum weight for a single extrinsic.
    pub const DEFAULT_MAX_EXTRINSIC_WEIGHT: u64 = 50_000_000_000;

    /// Configurable maximum weight for a single extrinsic dispatched during on_initialize.
    /// Extrinsics exceeding this limit are removed from the queue.
    #[pallet::storage]
    pub type MaxExtrinsicWeight<T: Config> =
        StorageValue<_, u64, ValueQuery, ConstU64<DEFAULT_MAX_EXTRINSIC_WEIGHT>>;

    /// A pending extrinsic stored for later execution.
    #[freeze_struct("f13d2a9d7bd4767d")]
    #[derive(Clone, Encode, Decode, TypeInfo, MaxEncodedLen, PartialEq, Debug)]
    #[scale_info(skip_type_params(T))]
    pub struct PendingExtrinsic<T: Config> {
        /// The account that submitted the extrinsic.
        pub who: T::AccountId,
        /// The encoded call data.
        pub encrypted_call: BoundedVec<u8, MaxEncryptedCallSize>,
        /// The block number when the extrinsic was submitted.
        pub submitted_at: BlockNumberFor<T>,
    }

    /// Storage map for encrypted extrinsics to be executed in on_initialize.
    /// Uses u32 index for O(1) insertion and removal. Count is maintained automatically.
    #[pallet::storage]
    pub type PendingExtrinsics<T: Config> =
        CountedStorageMap<_, Identity, u32, PendingExtrinsic<T>, OptionQuery>;

    /// Next index to use when inserting a pending extrinsic (unique auto-increment).
    #[pallet::storage]
    pub type NextPendingExtrinsicIndex<T: Config> = StorageValue<_, u32, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Encrypted wrapper accepted.
        EncryptedSubmitted { id: T::Hash, who: T::AccountId },
        /// Encrypted extrinsic was stored for later execution.
        ExtrinsicStored { index: u32, who: T::AccountId },
        /// Extrinsic decode failed during on_initialize.
        ExtrinsicDecodeFailed { index: u32 },
        /// Extrinsic dispatch failed during on_initialize.
        ExtrinsicDispatchFailed { index: u32, error: DispatchError },
        /// Extrinsic was successfully dispatched during on_initialize.
        ExtrinsicDispatched { index: u32 },
        /// Extrinsic expired (exceeded max block lifetime).
        ExtrinsicExpired { index: u32 },
        /// Extrinsic postponed due to weight limit.
        ExtrinsicPostponed { index: u32 },
        /// Maximum pending extrinsics limit was updated.
        MaxPendingExtrinsicsNumberSet { value: u32 },
        /// Maximum on_initialize weight was updated.
        OnInitializeWeightSet { value: u64 },
        /// Extrinsic lifetime was updated.
        ExtrinsicLifetimeSet { value: u32 },
        /// Maximum per-extrinsic weight was updated.
        MaxExtrinsicWeightSet { value: u64 },
        /// Extrinsic exceeded the per-extrinsic weight limit and was removed.
        ExtrinsicWeightExceeded { index: u32 },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The announced ML‑KEM encapsulation key length is invalid.
        BadEncKeyLen,
        /// Unreachable.
        Unreachable,
        /// Too many pending extrinsics in storage.
        TooManyPendingExtrinsics,
        /// Weight exceeds the absolute maximum (half of total block weight).
        WeightExceedsAbsoluteMax,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
            Self::process_pending_extrinsics()
        }

        fn on_runtime_upgrade() -> frame_support::weights::Weight {
            let mut weight = frame_support::weights::Weight::from_parts(0, 0);

            weight = weight.saturating_add(
                migrations::migrate_clear_v1_storage::migrate_clear_v1_storage::<T>(),
            );

            weight
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Rotate the key chain and announce the current author's ML-KEM encapsulation key.
        ///
        /// Called as an inherent every block. `enc_key` is `None` on node failure,
        /// which removes the author from future shielded tx eligibility.
        ///
        /// Key rotation order (using pre-update AuthorKeys):
        ///   1. CurrentKey  ← PendingKey
        ///   2. PendingKey  ← NextKey
        ///   3. NextKey     ← next-next author's key  (user-facing)
        ///   4. AuthorKeys[current] ← announced key
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::announce_next_key())]
        pub fn announce_next_key(
            origin: OriginFor<T>,
            enc_key: Option<ShieldEncKey>,
        ) -> DispatchResult {
            ensure_none(origin)?;

            let author = T::FindAuthors::find_current_author().ok_or(Error::<T>::Unreachable)?;
            let now = <frame_system::Pallet<T>>::block_number();

            // 1. CurrentKey ← PendingKey
            if let Some(pending_key) = PendingKey::<T>::take() {
                CurrentKey::<T>::put(pending_key);
            } else {
                CurrentKey::<T>::kill();
            }

            // 2. PendingKey ← NextKey (what was N+2 last block is now N+1)
            if let Some(next_key) = NextKey::<T>::take() {
                PendingKey::<T>::put(next_key);
            } else {
                PendingKey::<T>::kill();
            }

            // 3. NextKey ← next-next author's key
            if let Some(next_next_author) = T::FindAuthors::find_next_next_author()
                && let Some(key) = AuthorKeys::<T>::get(&next_next_author)
            {
                NextKey::<T>::put(key);
            } else {
                NextKey::<T>::kill();
            }

            // 4. Update AuthorKeys after rotations for consistent reads above.
            if let Some(enc_key) = &enc_key {
                ensure!(
                    enc_key.len() == MLKEM768_ENC_KEY_LEN,
                    Error::<T>::BadEncKeyLen
                );
                AuthorKeys::<T>::insert(&author, enc_key.clone());
            } else {
                AuthorKeys::<T>::remove(&author);
            }

            // 5. Set expiration blocks for user-facing keys.
            if PendingKey::<T>::get().is_some() {
                PendingKeyExpiresAt::<T>::put(now + 2u32.into());
            } else {
                PendingKeyExpiresAt::<T>::kill();
            }
            if NextKey::<T>::get().is_some() {
                NextKeyExpiresAt::<T>::put(now + 3u32.into());
            } else {
                NextKeyExpiresAt::<T>::kill();
            }

            Ok(())
        }

        /// Users submit an encrypted wrapper.
        ///
        /// Client‑side:
        ///
        ///   1. Read `NextKey` (ML‑KEM encapsulation key bytes) from storage.
        ///   2. Sign your extrinsic so that it can be executed when added to the pool,
        ///        i.e. you may need to increment the nonce if you submit using the same account.
        ///   3. Encrypt:
        ///
        ///        plaintext = signed_extrinsic
        ///        key_hash = xxhash128(NextKey)
        ///        kem_len = Length of kem_ct in bytes (u16)
        ///        kem_ct = Ciphertext from ML‑KEM‑768
        ///        nonce = Random 24 bytes used for XChaCha20‑Poly1305
        ///        aead_ct = Ciphertext from XChaCha20‑Poly1305
        ///
        ///      with ML‑KEM‑768 + XChaCha20‑Poly1305, producing
        ///
        ///        ciphertext = key_hash || kem_len || kem_ct || nonce || aead_ct
        ///
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::submit_encrypted())]
        pub fn submit_encrypted(
            origin: OriginFor<T>,
            ciphertext: BoundedVec<u8, ConstU32<8192>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let id: T::Hash = T::Hashing::hash_of(&(who.clone(), &ciphertext));

            Self::deposit_event(Event::EncryptedSubmitted { id, who });
            Ok(())
        }

        /// Store an encrypted extrinsic for later execution in on_initialize.
        #[pallet::call_index(2)]
        #[pallet::weight(store_encrypted_weight())]
        pub fn store_encrypted(
            origin: OriginFor<T>,
            encrypted_call: BoundedVec<u8, MaxEncryptedCallSize>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            ensure!(
                PendingExtrinsics::<T>::count() < MaxPendingExtrinsicsLimit::<T>::get(),
                Error::<T>::TooManyPendingExtrinsics
            );

            let index = NextPendingExtrinsicIndex::<T>::get();
            let pending = PendingExtrinsic {
                who: who.clone(),
                encrypted_call,
                submitted_at: frame_system::Pallet::<T>::block_number(),
            };
            PendingExtrinsics::<T>::insert(index, pending);

            NextPendingExtrinsicIndex::<T>::put(index.saturating_add(1));

            Self::deposit_event(Event::ExtrinsicStored { index, who });
            Ok(())
        }

        /// Set the maximum number of pending extrinsics allowed in the queue.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::set_max_pending_extrinsics_number())]
        pub fn set_max_pending_extrinsics_number(
            origin: OriginFor<T>,
            value: u32,
        ) -> DispatchResult {
            ensure_root(origin)?;

            MaxPendingExtrinsicsLimit::<T>::put(value);

            Self::deposit_event(Event::MaxPendingExtrinsicsNumberSet { value });
            Ok(())
        }

        /// Set the maximum weight allowed for on_initialize processing.
        /// Rejects values exceeding the absolute limit (half of total block weight).
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::set_on_initialize_weight())]
        pub fn set_on_initialize_weight(origin: OriginFor<T>, value: u64) -> DispatchResult {
            ensure_root(origin)?;

            ensure!(
                value <= MAX_ON_INITIALIZE_WEIGHT,
                Error::<T>::WeightExceedsAbsoluteMax
            );

            OnInitializeWeight::<T>::put(value);

            Self::deposit_event(Event::OnInitializeWeightSet { value });
            Ok(())
        }

        /// Set the extrinsic lifetime (max blocks between submission and execution).
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::set_stored_extrinsic_lifetime())]
        pub fn set_stored_extrinsic_lifetime(origin: OriginFor<T>, value: u32) -> DispatchResult {
            ensure_root(origin)?;

            ExtrinsicLifetime::<T>::put(value);

            Self::deposit_event(Event::ExtrinsicLifetimeSet { value });
            Ok(())
        }

        /// Set the maximum weight allowed for a single extrinsic during on_initialize processing.
        /// Extrinsics exceeding this limit are removed from the queue.
        /// Rejects values exceeding the absolute limit.
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::set_max_extrinsic_weight())]
        pub fn set_max_extrinsic_weight(origin: OriginFor<T>, value: u64) -> DispatchResult {
            ensure_root(origin)?;

            ensure!(
                value <= MAX_ON_INITIALIZE_WEIGHT,
                Error::<T>::WeightExceedsAbsoluteMax
            );

            MaxExtrinsicWeight::<T>::put(value);

            Self::deposit_event(Event::MaxExtrinsicWeightSet { value });
            Ok(())
        }
    }

    #[pallet::inherent]
    impl<T: Config> ProvideInherent for Pallet<T> {
        type Call = Call<T>;
        type Error = sp_inherents::MakeFatalError<()>;

        const INHERENT_IDENTIFIER: InherentIdentifier = INHERENT_IDENTIFIER;

        fn create_inherent(data: &InherentData) -> Option<Self::Call> {
            let enc_key = data
                .get_data::<InherentType>(&INHERENT_IDENTIFIER)
                .inspect_err(
                    |e| log::debug!(target: LOG_TARGET, "Failed to get shielded enc key inherent data: {:?}", e),
                )
                .ok()??;
            Some(Call::announce_next_key { enc_key })
        }

        fn is_inherent(call: &Self::Call) -> bool {
            matches!(call, Call::announce_next_key { .. })
        }
    }
}

impl<T: Config> Pallet<T> {
    /// Process pending encrypted extrinsics up to the weight limit.
    /// Returns the total weight consumed.
    pub fn process_pending_extrinsics() -> Weight {
        let next_index = NextPendingExtrinsicIndex::<T>::get();
        let count = PendingExtrinsics::<T>::count();

        let mut weight = T::DbWeight::get().reads(2);

        if count == 0 {
            return weight;
        }

        let start_index = next_index.saturating_sub(count);
        let current_block = frame_system::Pallet::<T>::block_number();

        // Process extrinsics
        for index in start_index..next_index {
            let Some(pending) = PendingExtrinsics::<T>::get(index) else {
                weight = weight.saturating_add(T::DbWeight::get().reads(1));

                continue;
            };

            let remove_weight = T::DbWeight::get().reads_writes(1, 2);

            // Check if the extrinsic has expired
            let age = current_block.saturating_sub(pending.submitted_at);
            if age > ExtrinsicLifetime::<T>::get().into() {
                PendingExtrinsics::<T>::remove(index);
                weight = weight.saturating_add(remove_weight);

                Self::deposit_event(Event::ExtrinsicExpired { index });

                continue;
            }

            let Ok(call) = T::ExtrinsicDecryptor::decrypt(&pending.encrypted_call) else {
                PendingExtrinsics::<T>::remove(index);
                weight = weight.saturating_add(remove_weight);

                Self::deposit_event(Event::ExtrinsicDecodeFailed { index });

                continue;
            };

            // Check if dispatching would exceed weight limit
            let info = call.get_dispatch_info();
            let dispatch_weight = T::DbWeight::get()
                .writes(2)
                .saturating_add(info.call_weight);

            // Check per-extrinsic weight limit
            let max_extrinsic_weight = Weight::from_parts(MaxExtrinsicWeight::<T>::get(), 0);
            if info.call_weight.any_gt(max_extrinsic_weight) {
                PendingExtrinsics::<T>::remove(index);
                weight = weight.saturating_add(remove_weight);

                Self::deposit_event(Event::ExtrinsicWeightExceeded { index });

                continue;
            }

            let max_weight = Weight::from_parts(OnInitializeWeight::<T>::get(), 0);

            if weight.saturating_add(dispatch_weight).any_gt(max_weight) {
                Self::deposit_event(Event::ExtrinsicPostponed { index });
                break;
            }

            // We're going to execute it - remove the item from storage
            PendingExtrinsics::<T>::remove(index);
            weight = weight.saturating_add(remove_weight);

            // Dispatch the extrinsic
            let origin: T::RuntimeOrigin = frame_system::RawOrigin::Signed(pending.who).into();
            let result = call.dispatch(origin);

            match result {
                Ok(post_info) => {
                    let actual_weight = post_info.actual_weight.unwrap_or(info.call_weight);
                    weight = weight.saturating_add(actual_weight);

                    Self::deposit_event(Event::ExtrinsicDispatched { index });
                }
                Err(e) => {
                    weight = weight.saturating_add(info.call_weight);

                    Self::deposit_event(Event::ExtrinsicDispatchFailed {
                        index,
                        error: e.error,
                    });
                }
            }
        }

        weight
    }

    pub fn try_decode_shielded_tx<Block: BlockT, Context: Default>(
        uxt: ExtrinsicOf<Block>,
    ) -> Option<ShieldedTransaction>
    where
        Block::Extrinsic: Checkable<Context>,
        CheckedOf<Block::Extrinsic, Context>: Applyable,
        ApplyableCallOf<CheckedOf<Block::Extrinsic, Context>>: IsSubType<Call<T>>,
    {
        // Prevent stack overflows by limiting the depth of the extrinsic.
        let encoded = uxt.encode();
        let uxt = <Block::Extrinsic as codec::DecodeLimit>::decode_all_with_depth_limit(
            MAX_EXTRINSIC_DEPTH,
            &mut &encoded[..],
        )
        .inspect_err(
            |e| log::debug!(target: LOG_TARGET, "Failed to decode shielded extrinsic: {:?}", e),
        )
        .ok()?;

        // Verify that the signature is correct.
        let xt = ExtrinsicOf::<Block>::check(uxt, &Context::default())
            .inspect_err(
                |e| log::debug!(target: LOG_TARGET, "Failed to check shielded extrinsic: {:?}", e),
            )
            .ok()?;
        let call = xt.call();

        let Some(Call::submit_encrypted { ciphertext }) = IsSubType::<Call<T>>::is_sub_type(call)
        else {
            return None;
        };

        ShieldedTransaction::parse(ciphertext)
    }

    pub fn is_shielded_using_current_key(key_hash: &[u8; 16]) -> bool {
        let pending = PendingKey::<T>::get();
        let pending_hash = pending.as_ref().map(|k| twox_128(&k[..]));
        pending_hash.as_ref() == Some(key_hash)
    }

    pub fn try_unshield_tx<Block: BlockT>(
        dec_key_bytes: alloc::vec::Vec<u8>,
        shielded_tx: ShieldedTransaction,
    ) -> Option<<Block as BlockT>::Extrinsic> {
        let plaintext = unshield(&dec_key_bytes, &shielded_tx).or_else(|| {
            log::debug!(target: LOG_TARGET, "Failed to unshield transaction");
            None
        })?;

        if plaintext.is_empty() {
            return None;
        }

        ExtrinsicOf::<Block>::decode(&mut &plaintext[..]).inspect_err(
            |e| log::debug!(target: LOG_TARGET, "Failed to decode shielded transaction: {:?}", e),
        ).ok()
    }
}

pub trait FindAuthors<T: Config> {
    fn find_current_author() -> Option<T::AuthorityId>;
    fn find_next_next_author() -> Option<T::AuthorityId>;
}

impl<T: Config> FindAuthors<T> for () {
    fn find_current_author() -> Option<T::AuthorityId> {
        None
    }
    fn find_next_next_author() -> Option<T::AuthorityId> {
        None
    }
}

/// Decrypt a shielded transaction using the raw decapsulation key bytes.
///
/// Performs ML-KEM-768 decapsulation followed by XChaCha20-Poly1305 AEAD decryption.
/// Runs entirely in WASM — no host functions needed.
fn unshield(
    dec_key_bytes: &[u8],
    shielded_tx: &ShieldedTransaction,
) -> Option<alloc::vec::Vec<u8>> {
    let dec_key = DecapsulationKey::<MlKem768Params>::from_bytes(dec_key_bytes.try_into().ok()?);
    let ciphertext = Ciphertext::<MlKem768>::try_from(shielded_tx.kem_ct.as_slice()).ok()?;
    let shared_secret = dec_key.decapsulate(&ciphertext).ok()?;

    let aead = XChaCha20Poly1305::new(shared_secret.as_slice().into());
    let nonce = XNonce::from_slice(&shielded_tx.nonce);
    aead.decrypt(
        nonce,
        Payload {
            msg: &shielded_tx.aead_ct,
            aad: &[],
        },
    )
    .ok()
}
