#![cfg_attr(not(feature = "std"), no_std)]

mod benchmarking;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod mock;

pub mod types;
pub mod weights;

use ark_serialize::CanonicalDeserialize;
use codec::{Decode, Encode};
use frame_support::IterableStorageDoubleMap;
use frame_support::weights::WeightMeter;
use frame_support::{
    BoundedVec,
    traits::{Currency, Get, ReservableCurrency},
};
use frame_system::pallet_prelude::BlockNumberFor;
pub use pallet::*;
use scale_info::prelude::collections::BTreeSet;
use sp_runtime::SaturatedConversion;
use sp_runtime::{Saturating, Weight, traits::Zero};
use sp_std::{boxed::Box, vec::Vec};
use subtensor_runtime_common::{NetUid, clear_prefix_with_meter};
use tle::{
    curves::drand::TinyBLS381,
    stream_ciphers::AESGCMStreamCipherProvider,
    tlock::{TLECiphertext, tld},
};
pub use types::*;
use w3f_bls::EngineBLS;
pub use weights::WeightInfo;

type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
#[deny(missing_docs)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*, traits::ReservableCurrency};
    use frame_system::pallet_prelude::{BlockNumberFor, *};

    #[pallet::pallet]
    #[pallet::without_storage_info]
    pub struct Pallet<T>(_);

    // Configure the pallet by specifying the parameters and types on which it depends.
    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_drand::Config {
        ///Currency type that will be used to reserve deposits for commitments
        type Currency: ReservableCurrency<Self::AccountId> + Send + Sync;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;

        /// Interface to access-limit metadata commitments
        type CanCommit: CanCommit<Self::AccountId>;

        /// Interface to trigger other pallets when metadata is committed
        type OnMetadataCommitment: OnMetadataCommitment<Self::AccountId>;

        /// The maximum number of additional fields that can be added to a commitment
        #[pallet::constant]
        type MaxFields: Get<u32> + TypeInfo + 'static;

        /// The amount held on deposit for a registered identity
        #[pallet::constant]
        type InitialDeposit: Get<BalanceOf<Self>>;

        /// The amount held on deposit per additional field for a registered identity.
        #[pallet::constant]
        type FieldDeposit: Get<BalanceOf<Self>>;

        /// Used to retrieve the given subnet's tempo
        type TempoInterface: GetTempoInterface;
    }

    /// Used to retrieve the given subnet's tempo
    pub trait GetTempoInterface {
        /// Used to retreive the epoch index for the given subnet.
        fn get_epoch_index(netuid: NetUid, cur_block: u64) -> u64;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A commitment was set
        Commitment {
            /// The netuid of the commitment
            netuid: NetUid,
            /// The account
            who: T::AccountId,
        },
        /// A timelock-encrypted commitment was set
        TimelockCommitment {
            /// The netuid of the commitment
            netuid: NetUid,
            /// The account
            who: T::AccountId,
            /// The drand round to reveal
            reveal_round: u64,
        },
        /// A timelock-encrypted commitment was auto-revealed
        CommitmentRevealed {
            /// The netuid of the commitment
            netuid: NetUid,
            /// The account
            who: T::AccountId,
        },
        /// A timelock-encrypted commitment could not be revealed and was left in place
        CommitmentRevealFailed {
            /// The netuid of the commitment
            netuid: NetUid,
            /// The account
            who: T::AccountId,
            /// The drand round that was attempted
            reveal_round: u64,
            /// Why reveal failed
            error: RevealFailure,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Account passed too many additional fields to their commitment
        TooManyFieldsInCommitmentInfo,
        /// Account is not allowed to make commitments to the chain
        AccountNotAllowedCommit,
        /// Space Limit Exceeded for the current interval
        SpaceLimitExceeded,
        /// Indicates that unreserve returned a leftover, which is unexpected.
        UnexpectedUnreserveLeftover,
        /// `TimelockRevealFailed` fields may only be created by the runtime.
        TimelockRevealFailedNotAllowed,
    }

    /// Tracks all CommitmentOf that have at least one timelocked field.
    #[pallet::storage]
    #[pallet::getter(fn timelocked_index)]
    pub type TimelockedIndex<T: Config> =
        StorageValue<_, BTreeSet<(NetUid, T::AccountId)>, ValueQuery>;

    /// Identity data by account
    #[pallet::storage]
    #[pallet::getter(fn commitment_of)]
    pub(super) type CommitmentOf<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Twox64Concat,
        T::AccountId,
        Registration<BalanceOf<T>, T::MaxFields, BlockNumberFor<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn last_commitment)]
    pub(super) type LastCommitment<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Twox64Concat,
        T::AccountId,
        BlockNumberFor<T>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn last_bonds_reset)]
    pub(super) type LastBondsReset<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Twox64Concat,
        T::AccountId,
        BlockNumberFor<T>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn revealed_commitments)]
    pub(super) type RevealedCommitments<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Twox64Concat,
        T::AccountId,
        Vec<(Vec<u8>, u64)>, // Reveals<(Data, RevealBlock)>
        OptionQuery,
    >;

    /// Maps (netuid, who) -> usage (how many “bytes” they've committed)
    /// in the RateLimit window
    #[pallet::storage]
    #[pallet::getter(fn used_space_of)]
    pub type UsedSpaceOf<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Twox64Concat,
        T::AccountId,
        UsageTracker,
        OptionQuery,
    >;

    #[pallet::type_value]
    /// The default Maximum Space
    pub fn DefaultMaxSpace() -> u32 {
        3100
    }

    #[pallet::storage]
    #[pallet::getter(fn max_space_per_user_per_rate_limit)]
    pub type MaxSpace<T> = StorageValue<_, u32, ValueQuery, DefaultMaxSpace>;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Set the commitment for a given netuid
        #[pallet::call_index(0)]
        #[pallet::weight((
            <T as pallet::Config>::WeightInfo::set_commitment()
                .saturating_add(T::CanCommit::validation_weight()),
            DispatchClass::Normal,
            Pays::No
        ))]
        pub fn set_commitment(
            origin: OriginFor<T>,
            netuid: NetUid,
            info: Box<CommitmentInfo<T::MaxFields>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin.clone())?;
            T::CanCommit::validate(netuid, &who)
                .map_err(|_| Error::<T>::AccountNotAllowedCommit)?;

            let extra_fields = info.fields.len() as u32;
            ensure!(
                extra_fields <= T::MaxFields::get(),
                Error::<T>::TooManyFieldsInCommitmentInfo
            );
            ensure!(
                !info
                    .fields
                    .iter()
                    .any(|field| matches!(field, Data::TimelockRevealFailed { .. })),
                Error::<T>::TimelockRevealFailedNotAllowed
            );

            let cur_block = <frame_system::Pallet<T>>::block_number();

            let min_used_space: u64 = 100;
            let required_space: u64 = info
                .fields
                .iter()
                .map(|field| field.len_for_rate_limit())
                .sum::<u64>()
                .max(min_used_space);

            let mut usage = UsedSpaceOf::<T>::get(netuid, &who).unwrap_or_default();
            let cur_block_u64 = cur_block.saturated_into::<u64>();
            let current_epoch = T::TempoInterface::get_epoch_index(netuid, cur_block_u64);

            if usage.last_epoch != current_epoch {
                usage.last_epoch = current_epoch;
                usage.used_space = 0;
            }

            // check if ResetBondsFlag is set in the fields
            for field in info.fields.iter() {
                if let Data::ResetBondsFlag = field {
                    // track when bonds reset was last triggered
                    <LastBondsReset<T>>::insert(netuid, &who, cur_block);
                    T::OnMetadataCommitment::on_metadata_commitment(netuid, &who);
                    break;
                }
            }

            let max_allowed = MaxSpace::<T>::get() as u64;
            ensure!(
                usage.used_space.saturating_add(required_space) <= max_allowed,
                Error::<T>::SpaceLimitExceeded
            );

            usage.used_space = usage.used_space.saturating_add(required_space);

            UsedSpaceOf::<T>::insert(netuid, &who, usage);

            let mut id = match <CommitmentOf<T>>::get(netuid, &who) {
                Some(mut id) => {
                    id.info = *info.clone();
                    id.block = cur_block;
                    id
                }
                None => Registration {
                    info: *info.clone(),
                    block: cur_block,
                    deposit: Zero::zero(),
                },
            };

            let old_deposit = id.deposit;
            let fd = <BalanceOf<T>>::from(extra_fields).saturating_mul(T::FieldDeposit::get());
            id.deposit = T::InitialDeposit::get().saturating_add(fd);
            if id.deposit > old_deposit {
                T::Currency::reserve(&who, id.deposit.saturating_sub(old_deposit))?;
            }
            if old_deposit > id.deposit {
                let err_amount =
                    T::Currency::unreserve(&who, old_deposit.saturating_sub(id.deposit));
                if !err_amount.is_zero() {
                    return Err(Error::<T>::UnexpectedUnreserveLeftover.into());
                }
            }

            <CommitmentOf<T>>::insert(netuid, &who, id);
            <LastCommitment<T>>::insert(netuid, &who, cur_block);

            if let Some(Data::TimelockEncrypted { reveal_round, .. }) = info
                .fields
                .iter()
                .find(|data| matches!(data, Data::TimelockEncrypted { .. }))
            {
                Self::deposit_event(Event::TimelockCommitment {
                    netuid,
                    who: who.clone(),
                    reveal_round: *reveal_round,
                });

                TimelockedIndex::<T>::mutate(|index| {
                    index.insert((netuid, who.clone()));
                });
            } else {
                Self::deposit_event(Event::Commitment {
                    netuid,
                    who: who.clone(),
                });

                TimelockedIndex::<T>::mutate(|index| {
                    index.remove(&(netuid, who.clone()));
                });
            }

            Ok(())
        }

        /// Sudo-set MaxSpace
        #[pallet::call_index(2)]
        #[pallet::weight(<T as pallet::Config>::WeightInfo::set_max_space())]
        pub fn set_max_space(origin: OriginFor<T>, new_limit: u32) -> DispatchResult {
            ensure_root(origin)?;
            MaxSpace::<T>::set(new_limit);
            Ok(())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            match Self::reveal_timelocked_commitments() {
                Ok(w) => <T as pallet::Config>::WeightInfo::reveal_timelocked_commitments()
                    .saturating_add(w),
                Err(e) => {
                    log::debug!("Failed to unveil matured commitments on block {n:?}: {e:?}");
                    <T as pallet::Config>::WeightInfo::reveal_timelocked_commitments()
                }
            }
        }
    }
}

// Interfaces to interact with other pallets
pub trait CanCommit<AccountId> {
    type Error;

    fn validate(netuid: NetUid, who: &AccountId) -> Result<(), Self::Error>;
    fn validation_weight() -> frame_support::weights::Weight;
}

impl<A> CanCommit<A> for () {
    type Error = ();

    fn validate(_: NetUid, _: &A) -> Result<(), Self::Error> {
        Err(())
    }

    fn validation_weight() -> frame_support::weights::Weight {
        frame_support::weights::Weight::zero()
    }
}

pub trait OnMetadataCommitment<AccountId> {
    fn on_metadata_commitment(netuid: NetUid, account: &AccountId);
}

impl<A> OnMetadataCommitment<A> for () {
    fn on_metadata_commitment(_: NetUid, _: &A) {}
}

/************************************************************
    CallType definition
************************************************************/
#[derive(Debug, PartialEq, Default)]
pub enum CallType {
    SetCommitment,
    #[default]
    Other,
}

use frame_support::{dispatch::DispatchResult, pallet_prelude::TypeInfo};

/// SCALE envelope used by `bt.timelock.encrypt` / `encrypt_at_round`.
/// The chain also accepts a raw compressed `TLECiphertext`.
#[derive(Decode)]
struct TimelockUserData {
    encrypted_data: Vec<u8>,
    _reveal_round: u64,
}

fn tle_ciphertext_from_bytes(encrypted: &[u8]) -> Result<TLECiphertext<TinyBLS381>, RevealFailure> {
    let mut raw_reader = encrypted;
    if let Ok(commit) = TLECiphertext::<TinyBLS381>::deserialize_compressed(&mut raw_reader)
        && raw_reader.is_empty()
    {
        return Ok(commit);
    }

    let mut envelope_reader = encrypted;
    let Ok(envelope) = TimelockUserData::decode(&mut envelope_reader) else {
        return Err(RevealFailure::CiphertextDeserialize);
    };
    if !envelope_reader.is_empty() {
        return Err(RevealFailure::CiphertextDeserialize);
    }

    let TimelockUserData {
        encrypted_data,
        _reveal_round,
    } = envelope;
    let mut inner_reader = encrypted_data.as_slice();
    let commit = TLECiphertext::<TinyBLS381>::deserialize_compressed(&mut inner_reader)
        .map_err(|_| RevealFailure::CiphertextDeserialize)?;
    if !inner_reader.is_empty() {
        return Err(RevealFailure::CiphertextDeserialize);
    }
    Ok(commit)
}

fn decrypt_timelock_ciphertext(
    encrypted: &[u8],
    pulse_signature: &[u8],
) -> Result<Vec<u8>, RevealFailure> {
    let signature_bytes = pulse_signature
        .strip_prefix(b"0x")
        .unwrap_or(pulse_signature);
    let sig_reader = &mut &signature_bytes[..];
    let sig = <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(sig_reader)
        .map_err(|_| RevealFailure::SignatureDeserialize)?;

    let commit = tle_ciphertext_from_bytes(encrypted)?;
    if commit.header.v.len() != 32 || commit.header.w.len() != 32 {
        return Err(RevealFailure::CiphertextDeserialize);
    }

    let decrypted_bytes = tld::<TinyBLS381, AESGCMStreamCipherProvider>(commit, sig)
        .map_err(|_| RevealFailure::Decrypt)?;
    if decrypted_bytes.is_empty() {
        return Err(RevealFailure::EmptyPlaintext);
    }
    Ok(decrypted_bytes)
}

impl<T: Config> Pallet<T> {
    pub fn reveal_timelocked_commitments() -> Result<Weight, sp_runtime::DispatchError> {
        let mut total_weight = Weight::from_parts(0, 0);

        let index = TimelockedIndex::<T>::get();
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
        let oldest_kept = pallet_drand::OldestStoredRound::<T>::get();
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

        for (netuid, who) in index.clone() {
            let maybe_registration = <CommitmentOf<T>>::get(netuid, &who);
            total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

            let Some(mut registration) = maybe_registration else {
                TimelockedIndex::<T>::mutate(|idx| {
                    idx.remove(&(netuid, who.clone()));
                });

                total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
                continue;
            };

            let original_fields = registration.info.fields.clone();
            let mut remain_fields = Vec::new();
            let mut revealed_fields = Vec::new();
            let mut still_pending = false;
            let mut mutated = false;

            for data in original_fields {
                match data {
                    Data::TimelockEncrypted {
                        encrypted,
                        reveal_round,
                    } => {
                        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
                        let pulse = match pallet_drand::Pulses::<T>::get(reveal_round) {
                            Some(p) => p,
                            None => {
                                if oldest_kept > 0 && reveal_round < oldest_kept {
                                    mutated = true;
                                    log::warn!(
                                        "Timelock round {reveal_round} expired for {who:?} (oldest kept {oldest_kept})"
                                    );
                                    Self::deposit_event(Event::CommitmentRevealFailed {
                                        netuid,
                                        who: who.clone(),
                                        reveal_round,
                                        error: RevealFailure::PulseExpired,
                                    });
                                    remain_fields.push(Data::TimelockRevealFailed {
                                        encrypted,
                                        reveal_round,
                                    });
                                } else {
                                    remain_fields.push(Data::TimelockEncrypted {
                                        encrypted,
                                        reveal_round,
                                    });
                                    still_pending = true;
                                }
                                continue;
                            }
                        };

                        mutated = true;

                        match decrypt_timelock_ciphertext(&encrypted, &pulse.signature) {
                            Ok(decrypted_bytes) => {
                                revealed_fields.push(decrypted_bytes);
                            }
                            Err(error) => {
                                log::warn!(
                                    "Failed to reveal timelock for {who:?} round {reveal_round}: {error:?}"
                                );
                                Self::deposit_event(Event::CommitmentRevealFailed {
                                    netuid,
                                    who: who.clone(),
                                    reveal_round,
                                    error,
                                });
                                remain_fields.push(Data::TimelockRevealFailed {
                                    encrypted,
                                    reveal_round,
                                });
                            }
                        }
                    }

                    other => remain_fields.push(other),
                }
            }

            // Waiting on a pulse, nothing converted or revealed: leave storage alone.
            if !mutated {
                if !still_pending {
                    TimelockedIndex::<T>::mutate(|idx| {
                        idx.remove(&(netuid, who.clone()));
                    });
                    total_weight =
                        total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
                }
                continue;
            }

            let Ok(remaining_fields) = BoundedVec::try_from(remain_fields) else {
                log::error!(
                    "Failed to build BoundedVec for remain_fields; this should be impossible \
    					because remain_fields is a subset of the original commitment fields"
                );
                continue;
            };

            if !revealed_fields.is_empty() {
                let mut existing_reveals =
                    RevealedCommitments::<T>::get(netuid, &who).unwrap_or_default();
                total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

                let current_block = <frame_system::Pallet<T>>::block_number();
                let block_u64 = current_block.saturated_into::<u64>();

                for revealed_bytes in revealed_fields {
                    existing_reveals.push((revealed_bytes, block_u64));
                    Self::deposit_event(Event::CommitmentRevealed {
                        netuid,
                        who: who.clone(),
                    });
                }

                const MAX_REVEALS: usize = 10;
                if existing_reveals.len() > MAX_REVEALS {
                    let remove_count = existing_reveals.len().saturating_sub(MAX_REVEALS);
                    existing_reveals.drain(0..remove_count);
                }

                RevealedCommitments::<T>::insert(netuid, &who, existing_reveals);
                total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
            }

            registration.info.fields = remaining_fields;

            match registration.info.fields.is_empty() {
                true => {
                    <CommitmentOf<T>>::remove(netuid, &who);
                    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

                    TimelockedIndex::<T>::mutate(|idx| {
                        idx.remove(&(netuid, who.clone()));
                    });

                    total_weight =
                        total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
                }
                false => {
                    <CommitmentOf<T>>::insert(netuid, &who, &registration);
                    total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));

                    if !still_pending {
                        TimelockedIndex::<T>::mutate(|idx| {
                            idx.remove(&(netuid, who.clone()));
                        });
                        total_weight =
                            total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
                    }
                }
            }
        }

        Ok(total_weight)
    }
    pub fn get_commitments(netuid: NetUid) -> Vec<(T::AccountId, Vec<u8>)> {
        let commitments: Vec<(T::AccountId, Vec<u8>)> =
            <CommitmentOf<T> as IterableStorageDoubleMap<
                NetUid,
                T::AccountId,
                Registration<BalanceOf<T>, T::MaxFields, BlockNumberFor<T>>,
            >>::iter_prefix(netuid)
            .map(|(account, registration)| {
                let bytes = registration.encode();
                (account, bytes)
            })
            .collect();
        commitments
    }

    /// Purges all commitment state for one neuron on a subnet.
    pub fn purge_neuron(netuid: NetUid, account: &T::AccountId) {
        if let Some(registration) = CommitmentOf::<T>::take(netuid, account) {
            T::Currency::unreserve(account, registration.deposit);
        }
        LastCommitment::<T>::remove(netuid, account);
        LastBondsReset::<T>::remove(netuid, account);
        RevealedCommitments::<T>::remove(netuid, account);
        UsedSpaceOf::<T>::remove(netuid, account);
        TimelockedIndex::<T>::mutate(|index| {
            index.remove(&(netuid, account.clone()));
        });
    }

    pub fn purge_netuid(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        let write_weight = T::DbWeight::get().writes(1);

        let result = clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            CommitmentOf::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            LastCommitment::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            LastBondsReset::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            RevealedCommitments::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            UsedSpaceOf::<T>::clear_prefix(netuid, limit, None)
        });

        if !result {
            return false;
        }

        if weight_meter.can_consume(write_weight) {
            TimelockedIndex::<T>::mutate(|index| {
                index.retain(|(n, _)| *n != netuid);
            });
            weight_meter.consume(write_weight);
            true
        } else {
            false
        }
    }
}

pub trait GetCommitments<AccountId> {
    fn get_commitments(netuid: NetUid) -> Vec<(AccountId, Vec<u8>)>;
}

impl<AccountId> GetCommitments<AccountId> for () {
    fn get_commitments(_netuid: NetUid) -> Vec<(AccountId, Vec<u8>)> {
        Vec::new()
    }
}
