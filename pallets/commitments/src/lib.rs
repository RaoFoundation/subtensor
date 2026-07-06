#![cfg_attr(not(feature = "std"), no_std)]

mod benchmarking;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod mock;

pub mod types;
pub mod weights;

use ark_serialize::CanonicalDeserialize;
use codec::Encode;
use frame_support::IterableStorageDoubleMap;
use frame_support::{
    BoundedVec,
    traits::{Currency, Get},
};
use frame_system::pallet_prelude::BlockNumberFor;
pub use pallet::*;
use scale_info::prelude::collections::BTreeSet;
use sp_runtime::SaturatedConversion;
use sp_runtime::{Saturating, Weight, traits::Zero};
use sp_std::{boxed::Box, vec::Vec};
use subtensor_runtime_common::NetUid;
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
            <T as pallet::Config>::WeightInfo::set_commitment(),
            DispatchClass::Normal,
            Pays::No
        ))]
        pub fn set_commitment(
            origin: OriginFor<T>,
            netuid: NetUid,
            info: Box<CommitmentInfo<T::MaxFields>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin.clone())?;
            ensure!(
                T::CanCommit::can_commit(netuid, &who),
                Error::<T>::AccountNotAllowedCommit
            );

            let extra_fields = info.fields.len() as u32;
            ensure!(
                extra_fields <= T::MaxFields::get(),
                Error::<T>::TooManyFieldsInCommitmentInfo
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
                Ok(w) => w,
                Err(e) => {
                    log::debug!("Failed to unveil matured commitments on block {n:?}: {e:?}");
                    Weight::from_parts(0, 0)
                }
            }
        }
    }
}

// Interfaces to interact with other pallets
pub trait CanCommit<AccountId> {
    fn can_commit(netuid: NetUid, who: &AccountId) -> bool;
}

impl<A> CanCommit<A> for () {
    fn can_commit(_: NetUid, _: &A) -> bool {
        false
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

impl<T: Config> Pallet<T> {
    pub fn reveal_timelocked_commitments() -> Result<Weight, sp_runtime::DispatchError> {
        let mut total_weight = Weight::from_parts(0, 0);

        let index = TimelockedIndex::<T>::get();
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
            let mut saw_timelock = false;
            let mut processed_timelock = false;

            for data in original_fields {
                match data {
                    Data::TimelockEncrypted {
                        encrypted,
                        reveal_round,
                    } => {
                        saw_timelock = true;
                        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
                        let pulse = match pallet_drand::Pulses::<T>::get(reveal_round) {
                            Some(p) => p,
                            None => {
                                remain_fields.push(Data::TimelockEncrypted {
                                    encrypted,
                                    reveal_round,
                                });
                                continue;
                            }
                        };

                        processed_timelock = true;

                        let signature_bytes = pulse
                            .signature
                            .strip_prefix(b"0x")
                            .unwrap_or(&pulse.signature);
                        let sig_reader = &mut &signature_bytes[..];
                        let sig =
                            <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(
                                sig_reader,
                            )
                            .map_err(|e| {
                                log::warn!(
                                    "Failed to deserialize drand signature for {who:?}: {e:?}"
                                )
                            })
                            .ok();

                        let Some(sig) = sig else {
                            log::warn!("No sig after deserialization");
                            continue;
                        };

                        let reader = &mut &encrypted[..];
                        let commit = TLECiphertext::<TinyBLS381>::deserialize_compressed(reader)
                            .map_err(|e| {
                                log::warn!("Failed to deserialize TLECiphertext for {who:?}: {e:?}")
                            })
                            .ok();

                        let Some(commit) = commit else {
                            log::warn!("No commit after deserialization");
                            continue;
                        };

                        let decrypted_bytes: Vec<u8> =
                            tld::<TinyBLS381, AESGCMStreamCipherProvider>(commit, sig)
                                .map_err(|e| {
                                    log::warn!("Failed to decrypt timelock for {who:?}: {e:?}")
                                })
                                .ok()
                                .unwrap_or_default();

                        if decrypted_bytes.is_empty() {
                            log::warn!("Bytes were decrypted for {who:?} but they are empty");
                            continue;
                        }

                        revealed_fields.push(decrypted_bytes);
                    }

                    other => remain_fields.push(other),
                }
            }

            if !saw_timelock {
                TimelockedIndex::<T>::mutate(|idx| {
                    idx.remove(&(netuid, who.clone()));
                });
                total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
                continue;
            }

            // Do not rewrite CommitmentOf every block for entries whose reveal round is
            // not yet available in the drand pulse storage. The hook has only performed
            // the index, commitment, and pulse reads accounted above.
            if !processed_timelock {
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

                // Push newly revealed items onto the tail of existing_reveals and emit the event
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

                    let has_timelock = registration
                        .info
                        .fields
                        .iter()
                        .any(|f| matches!(f, Data::TimelockEncrypted { .. }));
                    if !has_timelock {
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

    pub fn purge_netuid(netuid: NetUid) {
        let _ = CommitmentOf::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = LastCommitment::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = LastBondsReset::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = RevealedCommitments::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = UsedSpaceOf::<T>::clear_prefix(netuid, u32::MAX, None);

        TimelockedIndex::<T>::mutate(|index| {
            index.retain(|(n, _)| *n != netuid);
        });
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
