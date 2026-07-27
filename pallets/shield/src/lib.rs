//! # MevShield pallet
//!
//! Encrypts user extrinsics to the block author's ML-KEM-768 key so mempool
//! observers cannot frontrun plaintext calls. Clients encrypt to [`NextKey`]
//! (N+2 author); the inherent [`Call::announce_next_key`] rotates
//! `CurrentKey` ← `PendingKey` ← `NextKey` each block. Separately,
//! [`Call::store_encrypted`] queues ciphertext for deferred
//! [`Pallet::process_pending_extrinsics`] dispatch in `on_initialize`.
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

/// Fixed dispatch weight for [`Call::store_encrypted`] (not the benchmark figure).
pub fn store_encrypted_weight() -> Weight {
    Weight::from_parts(STORE_ENCRYPTED_WEIGHT, 0)
}

/// Runtime hook that turns queued `store_encrypted` bytes into a `RuntimeCall`.
///
/// Production may decrypt ciphertext; tests often SCALE-decode plaintext call bytes only.
pub trait ExtrinsicDecryptor<RuntimeCall> {
    /// Decrypt (or decode) stored bytes into a dispatchable `RuntimeCall`.
    fn decrypt(data: &[u8]) -> Result<RuntimeCall, DispatchError>;
}

/// Placeholder decryptor: always fails so misconfigured runtimes cannot silently dispatch.
impl<RuntimeCall> ExtrinsicDecryptor<RuntimeCall> for () {
    fn decrypt(_data: &[u8]) -> Result<RuntimeCall, DispatchError> {
        Err(DispatchError::Other("ExtrinsicDecryptor not implemented"))
    }
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use crate::weights::WeightInfo;

    /// MevShield configuration: Aura authority ids, author lookup, and decryptor for the queue.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Aura (or equivalent) authority id used as [`AuthorKeys`] map key.
        type AuthorityId: Member + Parameter + MaybeSerializeDeserialize + MaxEncodedLen;

        /// Resolves current and N+2 authors for key rotation in [`Call::announce_next_key`].
        type FindAuthors: FindAuthors<Self>;

        /// Call type decoded/dispatched from [`PendingExtrinsics`] in `on_initialize`.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo;

        /// Turns queued ciphertext/encoded bytes into [`Self::RuntimeCall`].
        type ExtrinsicDecryptor: ExtrinsicDecryptor<<Self as pallet::Config>::RuntimeCall>;

        /// Extrinsic weights for this pallet (see generated `weights` module).
        type WeightInfo: WeightInfo;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Current author's ML-KEM-768 encapsulation key after rotation (proposer-internal; not client encrypt target).
    #[pallet::storage]
    pub type CurrentKey<T> = StorageValue<_, ShieldEncKey, OptionQuery>;

    /// N+1 author's key; becomes [`CurrentKey`] next block. Hash of this key validates in-flight shielded txs.
    #[pallet::storage]
    pub type PendingKey<T> = StorageValue<_, ShieldEncKey, OptionQuery>;

    /// N+2 author's ML-KEM-768 encapsulation key — the client encrypt target for new shielded txs.
    #[pallet::storage]
    pub type NextKey<T> = StorageValue<_, ShieldEncKey, OptionQuery>;

    /// Last announced ML-KEM-768 encapsulation key per authority id (source for staging [`NextKey`]).
    #[pallet::storage]
    pub type AuthorKeys<T: Config> =
        StorageMap<_, Twox64Concat, T::AuthorityId, ShieldEncKey, OptionQuery>;

    /// Exclusive upper block bound for trusting [`PendingKey`] (set to `now + 2` when present).
    #[pallet::storage]
    pub type PendingKeyExpiresAt<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;

    /// Exclusive upper block bound for trusting [`NextKey`] (set to `now + 3` when present).
    #[pallet::storage]
    pub type NextKeyExpiresAt<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;

    /// Idempotency flags for runtime migrations keyed by migration name bytes.
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    /// Max SCALE/ciphertext bytes accepted by [`Call::store_encrypted`] (8192).
    pub type MaxEncryptedCallSize = ConstU32<8192>;

    /// Default for [`MaxPendingExtrinsicsLimit`] when unset (100).
    pub type DefaultMaxPendingExtrinsics = ConstU32<100>;

    /// Cap on [`PendingExtrinsics`] count; `store_encrypted` fails with [`Error::TooManyPendingExtrinsics`] when full.
    #[pallet::storage]
    pub type MaxPendingExtrinsicsLimit<T: Config> =
        StorageValue<_, u32, ValueQuery, DefaultMaxPendingExtrinsics>;

    /// Default for [`ExtrinsicLifetime`] when unset (10 blocks).
    pub const DEFAULT_EXTRINSIC_LIFETIME: u32 = 10;

    /// Max age (`current - submitted_at`) before a queued extrinsic is dropped as expired.
    #[pallet::storage]
    pub type ExtrinsicLifetime<T: Config> =
        StorageValue<_, u32, ValueQuery, ConstU32<DEFAULT_EXTRINSIC_LIFETIME>>;

    /// Default ref_time budget for processing the pending queue in `on_initialize`.
    pub const DEFAULT_ON_INITIALIZE_WEIGHT: u64 = 500_000_000_000;

    /// Hard ceiling for [`OnInitializeWeight`] / [`MaxExtrinsicWeight`] admin sets (half of 4s block).
    pub const MAX_ON_INITIALIZE_WEIGHT: u64 = 2_000_000_000_000;

    /// Aggregate ref_time budget for [`Pallet::process_pending_extrinsics`]; excess items emit [`Event::ExtrinsicPostponed`].
    #[pallet::storage]
    pub type OnInitializeWeight<T: Config> =
        StorageValue<_, u64, ValueQuery, ConstU64<DEFAULT_ON_INITIALIZE_WEIGHT>>;

    /// Default per-call ref_time cap during queue processing.
    pub const DEFAULT_MAX_EXTRINSIC_WEIGHT: u64 = 50_000_000_000;

    /// Per-call ref_time cap; overweight queued calls are removed with [`Event::ExtrinsicWeightExceeded`].
    #[pallet::storage]
    pub type MaxExtrinsicWeight<T: Config> =
        StorageValue<_, u64, ValueQuery, ConstU64<DEFAULT_MAX_EXTRINSIC_WEIGHT>>;

    /// One queued item: submitter, opaque call bytes, and submission block for lifetime checks.
    #[freeze_struct("f13d2a9d7bd4767d")]
    #[derive(Clone, Encode, Decode, TypeInfo, MaxEncodedLen, PartialEq, Debug)]
    #[scale_info(skip_type_params(T))]
    pub struct PendingExtrinsic<T: Config> {
        /// Signed origin that will be used when the call is later dispatched.
        pub who: T::AccountId,
        /// Opaque bytes passed to [`ExtrinsicDecryptor::decrypt`] (often SCALE-encoded call in tests).
        pub encrypted_call: BoundedVec<u8, MaxEncryptedCallSize>,
        /// Block number at insert time; compared against [`ExtrinsicLifetime`] during processing.
        pub submitted_at: BlockNumberFor<T>,
    }

    /// Counted queue of deferred calls, keyed by monotonic u32 index (gaps allowed; count is authoritative).
    #[pallet::storage]
    pub type PendingExtrinsics<T: Config> =
        CountedStorageMap<_, Identity, u32, PendingExtrinsic<T>, OptionQuery>;

    /// Next free index for [`PendingExtrinsics`] inserts; never decremented (unique auto-increment).
    #[pallet::storage]
    pub type NextPendingExtrinsicIndex<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// MevShield events: shielded submit, queue lifecycle, and admin limit updates.
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// `submit_encrypted` accepted; `id` is `hash(who, ciphertext)`.
        EncryptedSubmitted { id: T::Hash, who: T::AccountId },
        /// Call bytes enqueued under `index` for later `on_initialize` dispatch.
        ExtrinsicStored { index: u32, who: T::AccountId },
        /// [`ExtrinsicDecryptor`] failed; item removed from the queue.
        ExtrinsicDecodeFailed { index: u32 },
        /// Decrypted call dispatched but returned an error; item already removed.
        ExtrinsicDispatchFailed { index: u32, error: DispatchError },
        /// Queued call dispatched successfully under the original signed origin.
        ExtrinsicDispatched { index: u32 },
        /// Item age exceeded [`ExtrinsicLifetime`]; removed without dispatch.
        ExtrinsicExpired { index: u32 },
        /// Remaining [`OnInitializeWeight`] budget insufficient; left in queue for a later block.
        ExtrinsicPostponed { index: u32 },
        /// Root updated [`MaxPendingExtrinsicsLimit`].
        MaxPendingExtrinsicsNumberSet { value: u32 },
        /// Root updated [`OnInitializeWeight`].
        OnInitializeWeightSet { value: u64 },
        /// Root updated [`ExtrinsicLifetime`].
        ExtrinsicLifetimeSet { value: u32 },
        /// Root updated [`MaxExtrinsicWeight`].
        MaxExtrinsicWeightSet { value: u64 },
        /// Call weight exceeded [`MaxExtrinsicWeight`]; removed without dispatch.
        ExtrinsicWeightExceeded { index: u32 },
    }

    /// MevShield dispatch errors for key announce and queue admin/store paths.
    #[pallet::error]
    pub enum Error<T> {
        /// Announced key length ≠ [`MLKEM768_ENC_KEY_LEN`].
        BadEncKeyLen,
        /// Inherent ran without a resolvable current author (`FindAuthors` returned `None`).
        Unreachable,
        /// [`PendingExtrinsics`] count already at [`MaxPendingExtrinsicsLimit`].
        TooManyPendingExtrinsics,
        /// Admin weight argument exceeded [`MAX_ON_INITIALIZE_WEIGHT`].
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

        /// Enqueue opaque call bytes for deferred dispatch in `on_initialize`.
        ///
        /// Fails with [`Error::TooManyPendingExtrinsics`] when the counted queue is full.
        /// Weight is the fixed [`store_encrypted_weight`] (above the benchmark) to deter spam.
        #[allow(unknown_lints, benchmarked_weight_not_plugged)]
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
    /// Drain [`PendingExtrinsics`] from oldest index until empty, expired, overweight, or budget exhausted.
    ///
    /// Returns total weight consumed (DB + successful/failed dispatch weights). Postponed items stay queued.
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

    /// If `uxt` is a checked `submit_encrypted` call, parse its wire ciphertext into [`ShieldedTransaction`].
    ///
    /// Returns `None` for non-shield calls, bad signatures, malformed ciphertext, or extrinsic depth > [`MAX_EXTRINSIC_DEPTH`].
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

    /// True when `key_hash` equals `twox_128(PendingKey)` — the key clients encrypted toward one block ago.
    pub fn is_shielded_using_current_key(key_hash: &[u8; 16]) -> bool {
        let pending = PendingKey::<T>::get();
        let pending_hash = pending.as_ref().map(|k| twox_128(&k[..]));
        pending_hash.as_ref() == Some(key_hash)
    }

    /// Decrypt `shielded_tx` with raw ML-KEM-768 decapsulation key bytes and SCALE-decode the inner extrinsic.
    pub fn try_unshield_tx<Block: BlockT>(
        dec_key_bytes: alloc::vec::Vec<u8>,
        shielded_tx: ShieldedTransaction,
    ) -> Option<<Block as BlockT>::Extrinsic> {
        let plaintext =
            decrypt_shielded_ciphertext(&dec_key_bytes, &shielded_tx).or_else(|| {
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

/// Looks up the current block author and the author two slots ahead for key rotation.
pub trait FindAuthors<T: Config> {
    /// Authority producing the block in which `announce_next_key` runs.
    fn find_current_author() -> Option<T::AuthorityId>;
    /// Authority two slots ahead whose [`AuthorKeys`] entry stages into [`NextKey`].
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

/// ML-KEM-768 decapsulate + XChaCha20-Poly1305 decrypt; WASM-only (no host crypto).
fn decrypt_shielded_ciphertext(
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
