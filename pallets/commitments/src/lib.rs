#![cfg_attr(not(feature = "std"), no_std)]

mod benchmarking;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod mock;

pub mod types;
pub mod weights;

use ark_serialize::CanonicalDeserialize;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::IterableStorageDoubleMap;
use frame_support::ensure;
use frame_support::weights::WeightMeter;
use frame_support::{
    BoundedVec,
    traits::{Currency, Get, ReservableCurrency},
};
use frame_system::pallet_prelude::BlockNumberFor;
pub use pallet::*;
use scale_info::prelude::collections::BTreeSet;
use sp_runtime::SaturatedConversion;
use sp_runtime::{
    Saturating, Weight,
    traits::{BlakeTwo256, Hash, Zero},
};
use sp_std::{boxed::Box, ops::Bound, vec::Vec};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{
    AlphaBalance, MechId, NetUid, NetUidStorageIndex, TaoBalance, Token, clear_prefix_with_meter,
};
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

/// Price configuration for one commitment target. Absence means free commits.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommitBurnParameters {
    pub enabled: bool,
    pub burn: TaoBalance,
    pub min_burn: TaoBalance,
    pub max_burn: TaoBalance,
    pub half_life: u16,
    pub increase_mult: U64F64,
    pub last_update_block: u64,
}

/// Subtensor operations used by coldkey-paid commitments.
pub trait CommitBurnInterface<AccountId> {
    type Error: Into<sp_runtime::DispatchError>;

    fn ensure_mechanism(netuid: NetUid, mecid: MechId) -> Result<(), Self::Error>;
    fn ensure_coldkey_hotkey(
        netuid: NetUid,
        coldkey: &AccountId,
        hotkey: &AccountId,
    ) -> Result<(), Self::Error>;
    fn burn_enabled(netuid: NetUid, mecid: MechId, channel: u8) -> bool;
    fn current_price(netuid: NetUid, mecid: MechId, channel: u8) -> Option<CommitBurnParameters>;
    fn pay(
        netuid: NetUid,
        coldkey: &AccountId,
        hotkey: &AccountId,
        amount: TaoBalance,
    ) -> Result<AlphaBalance, Self::Error>;
    fn bump(netuid: NetUid, mecid: MechId, channel: u8);
    fn payment_weight() -> Weight;
}

impl<A> CommitBurnInterface<A> for () {
    type Error = sp_runtime::DispatchError;

    fn ensure_mechanism(_: NetUid, _: MechId) -> Result<(), Self::Error> {
        Err(sp_runtime::DispatchError::Other("commit burn unavailable"))
    }
    fn ensure_coldkey_hotkey(_: NetUid, _: &A, _: &A) -> Result<(), Self::Error> {
        Err(sp_runtime::DispatchError::Other("commit burn unavailable"))
    }
    fn burn_enabled(_: NetUid, _: MechId, _: u8) -> bool {
        false
    }
    fn current_price(_: NetUid, _: MechId, _: u8) -> Option<CommitBurnParameters> {
        None
    }
    fn pay(_: NetUid, _: &A, _: &A, _: TaoBalance) -> Result<AlphaBalance, Self::Error> {
        Err(sp_runtime::DispatchError::Other("commit burn unavailable"))
    }
    fn bump(_: NetUid, _: MechId, _: u8) {}
    fn payment_weight() -> Weight {
        Weight::zero()
    }
}

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

        /// Optional paid channel commitment integration with Subtensor.
        type CommitBurn: CommitBurnInterface<Self::AccountId>;
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
        /// A channel timelock-encrypted commitment was auto-revealed.
        ChannelCommitmentRevealed {
            /// Subnet containing the commitment target.
            netuid: NetUid,
            /// Emission mechanism containing the commitment target.
            mecid: MechId,
            /// Channel containing the revealed commitment.
            channel: u8,
            /// Hotkey whose channel commitment was revealed.
            hotkey: T::AccountId,
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
        /// A coldkey paid to publish a channel commitment.
        CommitmentBurned {
            /// Subnet containing the commitment target.
            netuid: NetUid,
            /// Emission mechanism containing the commitment target.
            mecid: MechId,
            /// Application-defined channel within the mechanism.
            channel: u8,
            /// Registered hotkey whose commitment was written.
            hotkey: T::AccountId,
            /// Coldkey that signed and paid the burn.
            coldkey: T::AccountId,
            /// TAO transferred to the subnet for this commitment.
            tao: TaoBalance,
            /// Alpha removed from subnet issuance by the registration payment path.
            alpha_recycled: AlphaBalance,
            /// Price after the paid-commit demand bump.
            new_price: TaoBalance,
            /// Monotonic receipt number within this channel.
            seq: u64,
            /// Blake2-256 hash of the SCALE-encoded commitment information.
            content_hash: [u8; 32],
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
        /// The target mechanism does not exist.
        MechanismDoesNotExist,
        /// The signer is not authorized to fund this hotkey commitment.
        AccountNotAuthorized,
        /// The current price is above the caller's `max_burn`.
        BurnPriceLimitExceeded,
        /// A free hotkey-signed commit cannot pay this channel's enabled burn.
        FreeCommitmentDisabled,
        /// This hotkey's channels are awaiting a bounded hotkey-swap migration.
        ChannelMigrationPending,
    }

    /// Tracks all CommitmentOf that have at least one timelocked field.
    #[pallet::storage]
    #[pallet::getter(fn timelocked_index)]
    pub type TimelockedIndex<T: Config> =
        StorageValue<_, BTreeSet<(NetUid, T::AccountId)>, ValueQuery>;

    /// Mechanism-first targets make retirement a bounded prefix operation. The
    /// hotkey-first index continues to drive deregistration and deferred renames.
    #[pallet::storage]
    pub type ChannelMechanismTargetsOf<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUid>,
            NMapKey<Identity, MechId>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, u8>,
        ),
        (),
        OptionQuery,
    >;

    /// Additional timelocked channel targets. Keyed separately from `TimelockedIndex`
    /// so its encoding and cursor remain backward compatible, while reveal and lifecycle
    /// work can use bounded storage iterators instead of copying a global set.
    #[pallet::storage]
    pub type ChannelTimelockedIndex<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUidStorageIndex>,
            NMapKey<Identity, u8>,
            NMapKey<Twox64Concat, T::AccountId>,
        ),
        (),
        OptionQuery,
    >;

    /// Channel targets indexed by `(netuid, hotkey, mechanism, channel)`.
    /// One row per target keeps enqueue, move and cleanup work independent of set size.
    #[pallet::storage]
    pub type ChannelTargetsOf<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUid>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, MechId>,
            NMapKey<Identity, u8>,
        ),
        (),
        OptionQuery,
    >;

    /// Independent channel commitment state; never aliases legacy mech-0 commitments.
    #[pallet::storage]
    pub type ChannelCommitmentOf<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUidStorageIndex>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, u8>,
        ),
        Registration<BalanceOf<T>, T::MaxFields, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Revealed fields retained for channel commitments.
    #[pallet::storage]
    pub type ChannelRevealedCommitments<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUidStorageIndex>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, u8>,
        ),
        Vec<(Vec<u8>, u64)>,
        OptionQuery,
    >;

    /// Per-target and hotkey tempo usage.
    #[pallet::storage]
    pub type ChannelUsedSpaceOf<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUidStorageIndex>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, u8>,
        ),
        UsageTracker,
        OptionQuery,
    >;

    /// Last write block for a channel target and hotkey.
    #[pallet::storage]
    pub type ChannelLastCommitment<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUidStorageIndex>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, u8>,
        ),
        BlockNumberFor<T>,
        OptionQuery,
    >;

    /// Reserve payer for each channel commitment. Ownership swaps do not move
    /// reserves, so cleanup must release the original payer rather than the current owner.
    #[pallet::storage]
    pub type ChannelReservePayerOf<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUidStorageIndex>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Identity, u8>,
        ),
        T::AccountId,
        OptionQuery,
    >;

    /// Forward rename edges `(netuid, old hotkey) -> new hotkey`.
    #[pallet::storage]
    pub type ChannelSwapPendingHotkeys<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Twox64Concat,
        T::AccountId,
        T::AccountId,
        OptionQuery,
    >;

    /// Reverse rename edges `(netuid, new hotkey, old hotkey)`. One row per edge
    /// supports overlapping renames and bounded cancellation without global scans.
    #[pallet::storage]
    pub type ChannelSwapOrigins<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUid>,
            NMapKey<Twox64Concat, T::AccountId>,
            NMapKey<Twox64Concat, T::AccountId>,
        ),
        (),
        OptionQuery,
    >;

    /// Raw-key cursor for the bounded, round-robin channel reveal pass. An inclusive
    /// cursor survives removal of the previously visited row without starving other entries.
    #[pallet::storage]
    pub type ChannelTimelockRevealCursor<T: Config> = StorageValue<_, Vec<u8>, OptionQuery>;

    /// Optional floating burn price for each commitment channel.
    #[pallet::storage]
    pub type CommitBurnConfig<T: Config> =
        StorageMap<_, Identity, (NetUidStorageIndex, u8), CommitBurnParameters, OptionQuery>;

    /// Per-channel monotonic burn receipt sequence.
    #[pallet::storage]
    pub type CommitBurnSequence<T: Config> =
        StorageMap<_, Identity, (NetUidStorageIndex, u8), u64, ValueQuery>;

    /// Channel cleanup has begun and must finish before dissolution can complete.
    #[pallet::storage]
    pub type ChannelPurgePending<T: Config> = StorageMap<_, Identity, NetUid, bool, ValueQuery>;

    /// Retired mechanisms whose channel state must be drained before another count change.
    /// Individual rows avoid reading or rewriting a network-wide set during administration.
    #[pallet::storage]
    pub type ChannelMechanismPurgeQueue<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Identity, MechId, (), OptionQuery>;

    #[pallet::storage]
    pub type ChannelMechanismRowsDrained<T: Config> =
        StorageMap<_, Identity, (NetUid, MechId), bool, ValueQuery>;

    /// Next channel whose burn configuration must be purged for a retired mechanism.
    #[pallet::storage]
    pub type ChannelMechanismConfigPurgeCursor<T: Config> =
        StorageMap<_, Identity, (NetUid, MechId), u16, OptionQuery>;

    /// Hotkeys whose channel state must be removed by bounded idle work. The target
    /// index is retained until that work finishes so cancellation also finds migrated rows.
    #[pallet::storage]
    pub type ChannelNeuronPurgeQueue<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Twox64Concat, T::AccountId, (), OptionQuery>;

    /// Per-block cap on mechanism-retirement rows. Ordinary channel usage is limited
    /// to `MaxSpace / 100` writes per neuron per tempo; a network that reaches the
    /// cap across every neuron must still have a finite administrative drain cost.
    pub const MAX_CHANNEL_PURGE_ROWS_PER_BLOCK: u32 = 256;

    /// Last `TimelockedIndex` entry visited by a reveal pass that hit the per-block
    /// decryption budget. The next pass resumes after it so a burst of matured
    /// commitments at the front of the index cannot starve the rest. Absent when the
    /// previous pass covered the whole index.
    #[pallet::storage]
    pub type TimelockRevealCursor<T: Config> = StorageValue<_, (NetUid, T::AccountId), OptionQuery>;

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
            ensure!(
                !T::CommitBurn::burn_enabled(netuid, MechId::MAIN, 0),
                Error::<T>::FreeCommitmentDisabled
            );
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

        /// Publish a channel commitment, with the coldkey paying the optional burn.
        #[pallet::call_index(3)]
        #[pallet::weight((
            <T as pallet::Config>::WeightInfo::set_commitment_to_free()
                .max(<T as pallet::Config>::WeightInfo::set_commitment_to_paid())
                .saturating_add(T::CanCommit::validation_weight())
                .saturating_add(T::CommitBurn::payment_weight()),
            DispatchClass::Normal,
            Pays::Yes
        ))]
        #[frame_support::transactional]
        pub fn set_commitment_to(
            origin: OriginFor<T>,
            netuid: NetUid,
            mecid: MechId,
            channel: u8,
            hotkey: T::AccountId,
            info: Box<CommitmentInfo<T::MaxFields>>,
            max_burn: TaoBalance,
        ) -> DispatchResult {
            let coldkey = ensure_signed(origin)?;
            T::CommitBurn::ensure_mechanism(netuid, mecid)
                .map_err(|_| Error::<T>::MechanismDoesNotExist)?;
            T::CanCommit::validate(netuid, &hotkey)
                .map_err(|_| Error::<T>::AccountNotAllowedCommit)?;
            T::CommitBurn::ensure_coldkey_hotkey(netuid, &coldkey, &hotkey)
                .map_err(|_| Error::<T>::AccountNotAuthorized)?;
            ensure!(
                !Self::channel_migration_pending(netuid, &hotkey),
                Error::<T>::ChannelMigrationPending
            );

            let (tao, alpha_recycled, new_price, seq) =
                if let Some(config) = T::CommitBurn::current_price(netuid, mecid, channel) {
                    ensure!(config.burn <= max_burn, Error::<T>::BurnPriceLimitExceeded);
                    let alpha_recycled = T::CommitBurn::pay(netuid, &coldkey, &hotkey, config.burn)
                        .map_err(Into::into)?;
                    T::CommitBurn::bump(netuid, mecid, channel);
                    let new_price = T::CommitBurn::current_price(netuid, mecid, channel)
                        .map_or(TaoBalance::ZERO, |updated| updated.burn);
                    let seq = CommitBurnSequence::<T>::mutate(
                        (Self::target_index(netuid, mecid), channel),
                        |sequence| {
                            *sequence = sequence.saturating_add(1);
                            *sequence
                        },
                    );
                    (config.burn, alpha_recycled, new_price, seq)
                } else {
                    (TaoBalance::ZERO, AlphaBalance::ZERO, TaoBalance::ZERO, 0)
                };

            Self::put_channel_commitment(netuid, mecid, channel, &hotkey, &coldkey, *info.clone())?;
            let content_hash = BlakeTwo256::hash(&info.encode()).to_fixed_bytes();
            Self::deposit_event(Event::CommitmentBurned {
                netuid,
                mecid,
                channel,
                hotkey,
                coldkey,
                tao,
                alpha_recycled,
                new_price,
                seq,
                content_hash,
            });
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

        fn on_idle(_n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
            let base = T::DbWeight::get().reads(1);
            if remaining_weight.any_lt(base) {
                return Weight::zero();
            }
            let queue = ChannelMechanismPurgeQueue::<T>::iter_keys().next();
            let swap_item_weight = T::DbWeight::get().reads_writes(9, 13);
            let mut total_weight = if queue.is_some()
                || remaining_weight.any_lt(base.saturating_add(swap_item_weight))
            {
                base
            } else {
                let mut swap_meter = WeightMeter::with_limit(remaining_weight.saturating_sub(base));
                Self::drain_channel_swaps(&mut swap_meter);
                base.saturating_add(swap_meter.consumed())
            };
            if let Some((netuid, mecid)) = queue {
                let mut meter = WeightMeter::with_limit(remaining_weight);
                meter.consume(base);
                let drained = ChannelMechanismRowsDrained::<T>::get((netuid, mecid));
                if !drained {
                    if Self::purge_channel_mechanism_config(netuid, mecid, &mut meter) {
                        ChannelMechanismRowsDrained::<T>::insert((netuid, mecid), true);
                    }
                } else if Self::purge_channel_mechanism_rows(
                    netuid,
                    mecid,
                    &mut meter,
                    MAX_CHANNEL_PURGE_ROWS_PER_BLOCK,
                ) {
                    ChannelMechanismPurgeQueue::<T>::remove(netuid, mecid);
                    ChannelMechanismRowsDrained::<T>::remove((netuid, mecid));
                }
                total_weight = total_weight.saturating_add(meter.consumed());
            }
            if total_weight.all_lt(remaining_weight) {
                let mut meter =
                    WeightMeter::with_limit(remaining_weight.saturating_sub(total_weight));
                Self::drain_channel_purges(&mut meter);
                total_weight = total_weight.saturating_add(meter.consumed());
            }
            if ChannelMechanismPurgeQueue::<T>::iter().next().is_none()
                && total_weight.all_lt(remaining_weight)
            {
                let mut meter =
                    WeightMeter::with_limit(remaining_weight.saturating_sub(total_weight));
                Self::drain_channel_swaps(&mut meter);
                total_weight = total_weight.saturating_add(meter.consumed());
            }
            if ChannelTimelockedIndex::<T>::iter_keys().next().is_none() {
                return total_weight;
            }
            let base = T::DbWeight::get().reads(3);
            let remaining_weight = remaining_weight.saturating_sub(total_weight);
            if remaining_weight.any_lt(base) {
                return total_weight;
            }
            let mut meter = WeightMeter::with_limit(remaining_weight);
            meter.consume(base);
            total_weight.saturating_add(
                base.saturating_add(
                    Self::reveal_channel_timelocked_commitments_metered(&mut meter)
                        .unwrap_or(Weight::zero()),
                ),
            )
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
    reveal_round: u64,
}

fn tle_ciphertext_from_bytes(
    encrypted: &[u8],
    reveal_round: u64,
) -> Result<TLECiphertext<TinyBLS381>, RevealFailure> {
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
        reveal_round: envelope_round,
    } = envelope;
    if envelope_round != reveal_round {
        return Err(RevealFailure::RoundMismatch);
    }
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
    reveal_round: u64,
    pulse_signature: &[u8],
) -> Result<Vec<u8>, RevealFailure> {
    let signature_bytes = pulse_signature
        .strip_prefix(b"0x")
        .unwrap_or(pulse_signature);
    let sig_reader = &mut &signature_bytes[..];
    let sig = <TinyBLS381 as EngineBLS>::SignatureGroup::deserialize_compressed(sig_reader)
        .map_err(|_| RevealFailure::SignatureDeserialize)?;

    let commit = tle_ciphertext_from_bytes(encrypted, reveal_round)?;
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

/// Upper bound on timelock decryptions one block's `on_initialize` performs. Each
/// decryption is a BLS pairing (the benchmarked `reveal_timelocked_commitments` unit);
/// fields left over are revealed by later blocks in index order.
pub const MAX_TIMELOCK_REVEALS_PER_BLOCK: u32 = 32;

/// Maximum `TimelockedIndex` entries one reveal pass visits, whether or not they decrypt
/// anything. An entry whose round has no pulse yet still costs a commitment read and one
/// pulse read per field, so without this bound the per-block cost of the hook would grow
/// with the number of pending commitments instead of with the work actually done. The
/// cursor makes the pass round-robin, so every entry is still reached within
/// `ceil(N / MAX_TIMELOCK_VISITS_PER_BLOCK)` blocks.
pub const MAX_TIMELOCK_VISITS_PER_BLOCK: u32 = 256;

impl<T: Config> Pallet<T> {
    fn target_index(netuid: NetUid, mecid: MechId) -> NetUidStorageIndex {
        u16::from(mecid)
            .saturating_mul(4096)
            .saturating_add(u16::from(netuid))
            .into()
    }

    /// Same field validation, deposit and last-write-wins logic as `set_commitment`.
    fn put_channel_commitment(
        netuid: NetUid,
        mecid: MechId,
        channel: u8,
        hotkey: &T::AccountId,
        payer: &T::AccountId,
        info: CommitmentInfo<T::MaxFields>,
    ) -> DispatchResult {
        ensure!(
            info.fields.len() <= T::MaxFields::get() as usize,
            Error::<T>::TooManyFieldsInCommitmentInfo
        );
        ensure!(
            !info
                .fields
                .iter()
                .any(|field| matches!(field, Data::TimelockRevealFailed { .. })),
            Error::<T>::TimelockRevealFailedNotAllowed
        );
        let index = Self::target_index(netuid, mecid);
        let key = (index, hotkey.clone(), channel);
        let current_block = <frame_system::Pallet<T>>::block_number();
        let current_epoch =
            T::TempoInterface::get_epoch_index(netuid, current_block.saturated_into::<u64>());
        let required_space = info
            .fields
            .iter()
            .map(Data::len_for_rate_limit)
            .sum::<u64>()
            .max(100);
        let mut usage = ChannelUsedSpaceOf::<T>::get(&key).unwrap_or_default();
        if usage.last_epoch != current_epoch {
            usage.last_epoch = current_epoch;
            usage.used_space = 0;
        }
        ensure!(
            usage.used_space.saturating_add(required_space) <= u64::from(MaxSpace::<T>::get()),
            Error::<T>::SpaceLimitExceeded
        );
        usage.used_space = usage.used_space.saturating_add(required_space);
        ChannelUsedSpaceOf::<T>::insert(&key, usage);

        for field in &info.fields {
            if matches!(field, Data::ResetBondsFlag) {
                T::OnMetadataCommitment::on_metadata_commitment(netuid, hotkey);
                break;
            }
        }
        let mut registration =
            ChannelCommitmentOf::<T>::get(&key).unwrap_or_else(|| Registration {
                info: info.clone(),
                block: current_block,
                deposit: Zero::zero(),
            });
        registration.info = info.clone();
        registration.block = current_block;
        let old_deposit = registration.deposit;
        let old_payer = ChannelReservePayerOf::<T>::get(&key);
        let deposit = T::InitialDeposit::get().saturating_add(
            <BalanceOf<T>>::from(info.fields.len() as u32).saturating_mul(T::FieldDeposit::get()),
        );
        registration.deposit = deposit;
        if old_payer.as_ref() == Some(payer) {
            if deposit > old_deposit {
                T::Currency::reserve(payer, deposit.saturating_sub(old_deposit))?;
            } else if old_deposit > deposit {
                let leftover = T::Currency::unreserve(payer, old_deposit.saturating_sub(deposit));
                ensure!(leftover.is_zero(), Error::<T>::UnexpectedUnreserveLeftover);
            }
        } else {
            // A coldkey swap does not move an existing reserve. The new owner
            // must fund the whole deposit before the original payer is released;
            // otherwise cleanup could strand the old reserve or release funds
            // that the new owner never locked. The call is transactional, so a
            // shortfall leaves the old payer and deposit untouched.
            if !deposit.is_zero() {
                T::Currency::reserve(payer, deposit)?;
            }
            if let Some(old_payer) = old_payer {
                let leftover = T::Currency::unreserve(&old_payer, old_deposit);
                ensure!(leftover.is_zero(), Error::<T>::UnexpectedUnreserveLeftover);
            }
        }
        ChannelCommitmentOf::<T>::insert(&key, registration);
        ChannelReservePayerOf::<T>::insert(&key, payer);
        ChannelLastCommitment::<T>::insert(&key, current_block);
        ChannelTargetsOf::<T>::insert((netuid, hotkey, mecid, channel), ());
        ChannelMechanismTargetsOf::<T>::insert((netuid, mecid, hotkey, channel), ());
        if let Some(Data::TimelockEncrypted { reveal_round, .. }) = info
            .fields
            .iter()
            .find(|field| matches!(field, Data::TimelockEncrypted { .. }))
        {
            ChannelTimelockedIndex::<T>::insert((index, channel, hotkey), ());
            Self::deposit_event(Event::TimelockCommitment {
                netuid,
                who: hotkey.clone(),
                reveal_round: *reveal_round,
            });
        } else {
            ChannelTimelockedIndex::<T>::remove((index, channel, hotkey));
            Self::deposit_event(Event::Commitment {
                netuid,
                who: hotkey.clone(),
            });
        }
        Ok(())
    }

    pub fn get_channel_commitment(
        netuid: NetUid,
        mecid: MechId,
        channel: u8,
        hotkey: &T::AccountId,
    ) -> Option<Registration<BalanceOf<T>, T::MaxFields, BlockNumberFor<T>>> {
        ChannelCommitmentOf::<T>::get((Self::target_index(netuid, mecid), hotkey, channel))
    }

    /// Index entries in visiting order: those after the cursor first, then the rest.
    fn timelock_reveal_order(
        index: &BTreeSet<(NetUid, T::AccountId)>,
        cursor: Option<&(NetUid, T::AccountId)>,
    ) -> Vec<(NetUid, T::AccountId)> {
        match cursor {
            Some(cursor) => index
                .range((Bound::Excluded(cursor), Bound::Unbounded))
                .chain(index.range((Bound::Unbounded, Bound::Included(cursor))))
                .cloned()
                .collect(),
            None => index.iter().cloned().collect(),
        }
    }

    pub fn reveal_timelocked_commitments() -> Result<Weight, sp_runtime::DispatchError> {
        let mut total_weight = Weight::from_parts(0, 0);

        let index = TimelockedIndex::<T>::get();
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
        let oldest_kept = pallet_drand::OldestStoredRound::<T>::get();
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
        let cursor = TimelockRevealCursor::<T>::get();
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));

        let mut decryptions: u32 = 0;
        let mut visits: u32 = 0;
        let mut last_visited: Option<(NetUid, T::AccountId)> = None;
        let mut budget_exhausted = false;

        for (netuid, who) in Self::timelock_reveal_order(&index, cursor.as_ref()) {
            if decryptions >= MAX_TIMELOCK_REVEALS_PER_BLOCK
                || visits >= MAX_TIMELOCK_VISITS_PER_BLOCK
            {
                budget_exhausted = true;
                break;
            }
            visits = visits.saturating_add(1);
            last_visited = Some((netuid, who.clone()));
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

                        if decryptions >= MAX_TIMELOCK_REVEALS_PER_BLOCK {
                            // Out of decryption budget: leave the field for a later block.
                            remain_fields.push(Data::TimelockEncrypted {
                                encrypted,
                                reveal_round,
                            });
                            still_pending = true;
                            budget_exhausted = true;
                            continue;
                        }
                        decryptions = decryptions.saturating_add(1);
                        total_weight = total_weight.saturating_add(
                            <T as pallet::Config>::WeightInfo::reveal_timelocked_commitments(),
                        );
                        mutated = true;

                        match decrypt_timelock_ciphertext(
                            &encrypted,
                            reveal_round,
                            &pulse.signature,
                        ) {
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

        if budget_exhausted {
            if last_visited != cursor {
                TimelockRevealCursor::<T>::set(last_visited);
                total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
            }
        } else if cursor.is_some() {
            TimelockRevealCursor::<T>::kill();
            total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
        }

        Ok(total_weight)
    }
    /// Reveal channel entries with the same bounded per-block decryption work as legacy.
    pub fn reveal_channel_timelocked_commitments() -> Result<Weight, sp_runtime::DispatchError> {
        let mut meter = WeightMeter::new();
        Self::reveal_channel_timelocked_commitments_metered(&mut meter)
    }

    fn reveal_channel_timelocked_commitments_metered(
        meter: &mut WeightMeter,
    ) -> Result<Weight, sp_runtime::DispatchError> {
        let cursor = ChannelTimelockRevealCursor::<T>::get();
        meter.consume(T::DbWeight::get().reads(1));
        let mut keys = match cursor.as_ref() {
            Some(raw) => ChannelTimelockedIndex::<T>::iter_keys_from(raw.clone()),
            None => ChannelTimelockedIndex::<T>::iter_keys(),
        };
        let mut wrapped = false;
        let mut total_weight = T::DbWeight::get().reads(1);
        if keys.next().is_none() && !wrapped {
            ChannelTimelockRevealCursor::<T>::kill();
            return Ok(total_weight);
        }
        // The probe above is this pass's first visit. Rebuild the iterator rather than
        // special-casing cursor ordering; `iter_from` is inclusive so deletion is safe.
        keys = match cursor.as_ref() {
            Some(raw) => ChannelTimelockedIndex::<T>::iter_keys_from(raw.clone()),
            None => ChannelTimelockedIndex::<T>::iter_keys(),
        };
        let oldest_kept = pallet_drand::OldestStoredRound::<T>::get();
        meter.consume(T::DbWeight::get().reads(1));
        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
        let mut decryptions = 0u32;
        let mut visits = 0u32;
        let mut last_visited = cursor.clone();
        let mut budget_exhausted = false;
        loop {
            if budget_exhausted
                || decryptions >= MAX_TIMELOCK_REVEALS_PER_BLOCK
                || visits >= MAX_TIMELOCK_VISITS_PER_BLOCK
            {
                if wrapped {
                    break;
                }
                budget_exhausted = true;
                drop(keys);
                keys = ChannelTimelockedIndex::<T>::iter_keys();
                wrapped = true;
                continue;
            }
            let Some((index, channel, who)) = keys.next() else {
                if wrapped || cursor.is_none() {
                    break;
                }
                // A stale cursor can be past the last remaining row after
                // deregistration or a hotkey swap. Wrap rather than stranding
                // every earlier pending commitment.
                drop(keys);
                keys = ChannelTimelockedIndex::<T>::iter_keys();
                wrapped = true;
                continue;
            };
            let scan_weight = T::DbWeight::get()
                .reads_writes(u64::from(T::MaxFields::get()).saturating_add(4), 4);
            if !meter.can_consume(scan_weight) {
                budget_exhausted = true;
                break;
            }
            meter.consume(scan_weight);
            visits = visits.saturating_add(1);
            last_visited = Some(ChannelTimelockedIndex::<T>::hashed_key_for((
                index, channel, &who,
            )));
            let netuid = NetUid::from(u16::from(index) % 4096);
            let mecid = MechId::from((u16::from(index) / 4096) as u8);
            let key = (index, who.clone(), channel);
            let Some(mut registration) = ChannelCommitmentOf::<T>::get(&key) else {
                total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
                ChannelTimelockedIndex::<T>::remove((index, channel, &who));
                total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
                continue;
            };
            total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
            let mut remaining = Vec::new();
            let mut revealed = Vec::new();
            let mut pending = false;
            let mut mutated = false;
            for (field_index, field) in registration.info.fields.iter().cloned().enumerate() {
                if decryptions >= MAX_TIMELOCK_REVEALS_PER_BLOCK {
                    if matches!(field, Data::TimelockEncrypted { .. }) {
                        pending = true;
                    }
                    remaining.push(field);
                    budget_exhausted = true;
                    continue;
                }
                match field {
                    Data::TimelockEncrypted {
                        encrypted,
                        reveal_round,
                    } => {
                        total_weight = total_weight.saturating_add(T::DbWeight::get().reads(1));
                        let Some(pulse) = pallet_drand::Pulses::<T>::get(reveal_round) else {
                            if oldest_kept > 0 && reveal_round < oldest_kept {
                                mutated = true;
                                Self::deposit_event(Event::CommitmentRevealFailed {
                                    netuid,
                                    who: who.clone(),
                                    reveal_round,
                                    error: RevealFailure::PulseExpired,
                                });
                                remaining.push(Data::TimelockRevealFailed {
                                    encrypted,
                                    reveal_round,
                                });
                            } else {
                                remaining.push(Data::TimelockEncrypted {
                                    encrypted,
                                    reveal_round,
                                });
                                pending = true;
                            }
                            continue;
                        };
                        let decryption_weight =
                            <T as pallet::Config>::WeightInfo::reveal_timelocked_commitments();
                        let entry_weight =
                            decryption_weight.saturating_add(T::DbWeight::get().reads_writes(3, 3));
                        if !meter.can_consume(entry_weight) {
                            // Keep this field and every unvisited field queued
                            // until idle weight can cover their decryption.
                            remaining.push(Data::TimelockEncrypted {
                                encrypted,
                                reveal_round,
                            });
                            remaining.extend(
                                registration
                                    .info
                                    .fields
                                    .into_iter()
                                    .skip(field_index.saturating_add(1)),
                            );
                            pending = true;
                            budget_exhausted = true;
                            break;
                        }
                        meter.consume(entry_weight);
                        decryptions = decryptions.saturating_add(1);
                        total_weight = total_weight.saturating_add(decryption_weight);
                        mutated = true;
                        match decrypt_timelock_ciphertext(
                            &encrypted,
                            reveal_round,
                            &pulse.signature,
                        ) {
                            Ok(bytes) => revealed.push(bytes),
                            Err(error) => {
                                Self::deposit_event(Event::CommitmentRevealFailed {
                                    netuid,
                                    who: who.clone(),
                                    reveal_round,
                                    error,
                                });
                                remaining.push(Data::TimelockRevealFailed {
                                    encrypted,
                                    reveal_round,
                                });
                            }
                        }
                    }
                    other => remaining.push(other),
                }
            }
            if !mutated {
                continue;
            }
            let Ok(fields) = BoundedVec::try_from(remaining) else {
                debug_assert!(false, "revealed fields cannot exceed the original bound");
                continue;
            };
            if !revealed.is_empty() {
                let mut existing = ChannelRevealedCommitments::<T>::get(&key).unwrap_or_default();
                let block = <frame_system::Pallet<T>>::block_number().saturated_into::<u64>();
                for bytes in revealed {
                    existing.push((bytes, block));
                    Self::deposit_event(Event::ChannelCommitmentRevealed {
                        netuid,
                        mecid,
                        channel,
                        hotkey: who.clone(),
                    });
                }
                if existing.len() > 10 {
                    let excess = existing.len().saturating_sub(10);
                    existing.drain(..excess);
                }
                ChannelRevealedCommitments::<T>::insert(&key, existing);
                total_weight = total_weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
            }
            registration.info.fields = fields;
            if registration.info.fields.is_empty() {
                ChannelCommitmentOf::<T>::remove(&key);
                // Revealing the last field removes the active commitment and
                // releases its reserve. Revealed history and its target remain
                // until lifecycle cleanup; the payer is not migrated on rewrite.
                if let Some(payer) = ChannelReservePayerOf::<T>::take(&key) {
                    T::Currency::unreserve(&payer, registration.deposit);
                }
            } else {
                ChannelCommitmentOf::<T>::insert(&key, registration);
            }
            total_weight = total_weight.saturating_add(T::DbWeight::get().writes(2));
            if !pending {
                ChannelTimelockedIndex::<T>::remove((index, channel, &who));
                total_weight = total_weight.saturating_add(T::DbWeight::get().writes(1));
            }
        }
        if budget_exhausted {
            if last_visited != cursor {
                ChannelTimelockRevealCursor::<T>::set(last_visited);
            }
        } else if cursor.is_some() {
            ChannelTimelockRevealCursor::<T>::kill();
        }
        Ok(meter.consumed())
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

    /// Queue one deferred rename. Empty intermediate edges remain until their
    /// incoming source has drained, so storage iteration order cannot strand rows.
    pub fn swap_neuron(netuid: NetUid, old_hotkey: &T::AccountId, new_hotkey: &T::AccountId) {
        if ChannelSwapPendingHotkeys::<T>::get(netuid, new_hotkey).as_ref() == Some(old_hotkey)
            && ChannelSwapOrigins::<T>::contains_key((netuid, old_hotkey, new_hotkey))
        {
            // Cancel the older direction. If no row reached this intermediate,
            // the reversal is a no-op; otherwise those rows must return as well.
            ChannelSwapPendingHotkeys::<T>::remove(netuid, new_hotkey);
            if ChannelTargetsOf::<T>::iter_key_prefix((netuid, old_hotkey))
                .next()
                .is_none()
            {
                ChannelSwapOrigins::<T>::remove((netuid, old_hotkey, new_hotkey));
                return;
            }
        } else if let Some(previous) = ChannelSwapPendingHotkeys::<T>::take(netuid, old_hotkey) {
            ChannelSwapOrigins::<T>::remove((netuid, &previous, old_hotkey));
        }
        ChannelSwapPendingHotkeys::<T>::insert(netuid, old_hotkey, new_hotkey.clone());
        ChannelSwapOrigins::<T>::insert((netuid, new_hotkey, old_hotkey), ());
    }

    /// Writes resume after both incoming and outgoing channel work has drained.
    pub fn channel_migration_pending(netuid: NetUid, hotkey: &T::AccountId) -> bool {
        ChannelNeuronPurgeQueue::<T>::contains_key(netuid, hotkey)
            || ChannelSwapPendingHotkeys::<T>::contains_key(netuid, hotkey)
            || ChannelSwapOrigins::<T>::iter_key_prefix((netuid, hotkey))
                .next()
                .is_some()
    }

    fn purge_channel_target(netuid: NetUid, hotkey: &T::AccountId, mecid: MechId, channel: u8) {
        let index = Self::target_index(netuid, mecid);
        let key = (index, hotkey.clone(), channel);
        if let Some(registration) = ChannelCommitmentOf::<T>::take(&key)
            && let Some(payer) = ChannelReservePayerOf::<T>::take(&key)
        {
            T::Currency::unreserve(&payer, registration.deposit);
        } else {
            ChannelReservePayerOf::<T>::remove(&key);
        }
        ChannelRevealedCommitments::<T>::remove(&key);
        ChannelUsedSpaceOf::<T>::remove(&key);
        ChannelLastCommitment::<T>::remove(&key);
        ChannelTimelockedIndex::<T>::remove((index, channel, hotkey));
        ChannelTargetsOf::<T>::remove((netuid, hotkey, mecid, channel));
        ChannelMechanismTargetsOf::<T>::remove((netuid, mecid, hotkey, channel));
    }

    /// Queue cleanup without traversing channel state or rename edges inline.
    pub fn purge_channel_neuron(netuid: NetUid, account: &T::AccountId) {
        ChannelNeuronPurgeQueue::<T>::insert(netuid, account, ());
    }

    /// Cancel an incoming edge before deleting its destination. The source purge
    /// releases any not-yet-migrated reserve, so cleanup cannot finish and then
    /// receive a late row from that source.
    fn cancel_channel_purge_edges(netuid: NetUid, hotkey: &T::AccountId) {
        if let Some(source) = ChannelSwapOrigins::<T>::iter_key_prefix((netuid, hotkey)).next() {
            ChannelSwapPendingHotkeys::<T>::remove(netuid, &source);
            ChannelSwapOrigins::<T>::remove((netuid, hotkey, &source));
            ChannelNeuronPurgeQueue::<T>::insert(netuid, source, ());
            return;
        }
        if let Some(destination) = ChannelSwapPendingHotkeys::<T>::take(netuid, hotkey) {
            ChannelSwapOrigins::<T>::remove((netuid, destination, hotkey));
        }
    }

    pub fn drain_channel_purges(weight_meter: &mut WeightMeter) {
        let item_weight = T::DbWeight::get().reads_writes(5, 11);
        for _ in 0..MAX_CHANNEL_PURGE_ROWS_PER_BLOCK {
            if !weight_meter.can_consume(item_weight) {
                return;
            }
            let Some((netuid, hotkey)) = ChannelNeuronPurgeQueue::<T>::iter_keys().next() else {
                return;
            };
            if let Some((mecid, channel)) =
                ChannelTargetsOf::<T>::iter_key_prefix((netuid, &hotkey)).next()
            {
                Self::purge_channel_target(netuid, &hotkey, mecid, channel);
            } else if ChannelSwapOrigins::<T>::iter_key_prefix((netuid, &hotkey))
                .next()
                .is_some()
                || ChannelSwapPendingHotkeys::<T>::contains_key(netuid, &hotkey)
            {
                Self::cancel_channel_purge_edges(netuid, &hotkey);
            } else {
                ChannelNeuronPurgeQueue::<T>::remove(netuid, &hotkey);
            }
            weight_meter.consume(item_weight);
        }
    }

    pub fn drain_channel_swaps(weight_meter: &mut WeightMeter) {
        for _ in 0..MAX_CHANNEL_PURGE_ROWS_PER_BLOCK {
            Self::drain_channel_swap_step(weight_meter, true);
        }
    }

    fn drain_channel_swap_step(weight_meter: &mut WeightMeter, honor_retirement: bool) {
        let item_weight = T::DbWeight::get().reads_writes(9, 13);
        if !weight_meter.can_consume(item_weight) {
            return;
        }
        let Some((netuid, mut old_hotkey, mut new_hotkey)) =
            ChannelSwapPendingHotkeys::<T>::iter().next()
        else {
            return;
        };
        if ChannelPurgePending::<T>::get(netuid) {
            ChannelSwapPendingHotkeys::<T>::remove(netuid, &old_hotkey);
            ChannelSwapOrigins::<T>::remove((netuid, &new_hotkey, &old_hotkey));
            weight_meter.consume(item_weight);
            return;
        }
        if ChannelTargetsOf::<T>::iter_key_prefix((netuid, &old_hotkey))
            .next()
            .is_none()
            && let Some(source) =
                ChannelSwapOrigins::<T>::iter_key_prefix((netuid, &old_hotkey)).next()
            && ChannelTargetsOf::<T>::iter_key_prefix((netuid, &source))
                .next()
                .is_some()
        {
            // An empty B -> C edge may sort before A -> B. Service the source
            // that can actually supply B's next row instead of spinning on B.
            new_hotkey = old_hotkey;
            old_hotkey = source;
        }
        let mut targets = ChannelTargetsOf::<T>::iter_key_prefix((netuid, &old_hotkey));
        let first_target = targets.next();
        let first_is_live = first_target.is_some_and(|(mecid, _)| {
            !honor_retirement || !ChannelMechanismPurgeQueue::<T>::contains_key(netuid, mecid)
        });
        let target = if first_is_live {
            first_target
        } else {
            targets.find(|(mecid, _)| {
                !honor_retirement || !ChannelMechanismPurgeQueue::<T>::contains_key(netuid, *mecid)
            })
        };
        let Some((mecid, channel)) = target else {
            if ChannelNeuronPurgeQueue::<T>::contains_key(netuid, &new_hotkey) {
                ChannelNeuronPurgeQueue::<T>::insert(netuid, &old_hotkey, ());
            } else if first_target.is_none()
                && ChannelSwapOrigins::<T>::iter_key_prefix((netuid, &old_hotkey))
                    .next()
                    .is_some()
            {
                // B -> C may be visited before A -> B. Keep the empty edge
                // until A supplies B's rows; the next pass will move them on.
                weight_meter.consume(item_weight);
                return;
            }
            ChannelSwapPendingHotkeys::<T>::remove(netuid, &old_hotkey);
            ChannelSwapOrigins::<T>::remove((netuid, &new_hotkey, &old_hotkey));
            weight_meter.consume(item_weight);
            return;
        };
        if ChannelNeuronPurgeQueue::<T>::contains_key(netuid, &new_hotkey) {
            ChannelNeuronPurgeQueue::<T>::insert(netuid, &old_hotkey, ());
            ChannelSwapPendingHotkeys::<T>::remove(netuid, &old_hotkey);
            ChannelSwapOrigins::<T>::remove((netuid, &new_hotkey, &old_hotkey));
            weight_meter.consume(item_weight);
            return;
        }
        let index = Self::target_index(netuid, mecid);
        let old_key = (index, &old_hotkey, channel);
        let new_key = (index, &new_hotkey, channel);
        // A swap destination is not a subnet member but may retain a channel from
        // an earlier registration. Last-write-wins must release that row's payer
        // before replacing it, rather than orphaning the old reserve.
        if ChannelTargetsOf::<T>::contains_key((netuid, &new_hotkey, mecid, channel)) {
            Self::purge_channel_target(netuid, &new_hotkey, mecid, channel);
        }
        if let Some(registration) = ChannelCommitmentOf::<T>::take(&old_key) {
            ChannelCommitmentOf::<T>::insert(&new_key, registration);
        }
        if let Some(payer) = ChannelReservePayerOf::<T>::take(&old_key) {
            ChannelReservePayerOf::<T>::insert(&new_key, payer);
        }
        if let Some(reveals) = ChannelRevealedCommitments::<T>::take(&old_key) {
            ChannelRevealedCommitments::<T>::insert(&new_key, reveals);
        }
        if let Some(usage) = ChannelUsedSpaceOf::<T>::take(&old_key) {
            ChannelUsedSpaceOf::<T>::insert(&new_key, usage);
        }
        if let Some(block) = ChannelLastCommitment::<T>::take(&old_key) {
            ChannelLastCommitment::<T>::insert(&new_key, block);
        }
        if ChannelTimelockedIndex::<T>::take((index, channel, &old_hotkey)).is_some() {
            ChannelTimelockedIndex::<T>::insert((index, channel, &new_hotkey), ());
        }
        ChannelTargetsOf::<T>::remove((netuid, &old_hotkey, mecid, channel));
        ChannelTargetsOf::<T>::insert((netuid, &new_hotkey, mecid, channel), ());
        ChannelMechanismTargetsOf::<T>::remove((netuid, mecid, &old_hotkey, channel));
        ChannelMechanismTargetsOf::<T>::insert((netuid, mecid, &new_hotkey, channel), ());
        weight_meter.consume(item_weight);
    }

    fn purge_channel_mechanism_rows(
        netuid: NetUid,
        mecid: MechId,
        weight_meter: &mut WeightMeter,
        row_limit: u32,
    ) -> bool {
        let write_weight = T::DbWeight::get().writes(1);
        let index = Self::target_index(netuid, mecid);
        // Target rows include revealed-only channels. Retirement deletes those
        // rows directly; live mechanisms on the same pending rename still move.
        let mut remaining_rows = row_limit;
        loop {
            if remaining_rows == 0 {
                return false;
            }
            let item_weight = T::DbWeight::get().reads_writes(8, 9);
            if !weight_meter.can_consume(item_weight) {
                return false;
            }
            let Some((hotkey, channel)) =
                ChannelMechanismTargetsOf::<T>::iter_key_prefix((netuid, mecid)).next()
            else {
                break;
            };
            Self::purge_channel_target(netuid, &hotkey, mecid, channel);
            remaining_rows = remaining_rows.saturating_sub(1);
            weight_meter.consume(item_weight);
        }
        clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ChannelRevealedCommitments::<T>::clear_prefix((index,), limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ChannelReservePayerOf::<T>::clear_prefix((index,), limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ChannelUsedSpaceOf::<T>::clear_prefix((index,), limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ChannelLastCommitment::<T>::clear_prefix((index,), limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ChannelTimelockedIndex::<T>::clear_prefix((index,), limit, None)
        })
    }

    fn purge_channel_mechanism_config(
        netuid: NetUid,
        mecid: MechId,
        weight_meter: &mut WeightMeter,
    ) -> bool {
        let write_weight = T::DbWeight::get().writes(1);
        let index = Self::target_index(netuid, mecid);
        let start = ChannelMechanismConfigPurgeCursor::<T>::get((netuid, mecid)).unwrap_or(0);
        for channel in start..=u16::from(u8::MAX) {
            if !weight_meter.can_consume(write_weight.saturating_mul(2)) {
                ChannelMechanismConfigPurgeCursor::<T>::insert((netuid, mecid), channel);
                return false;
            }
            CommitBurnConfig::<T>::remove((index, channel as u8));
            CommitBurnSequence::<T>::remove((index, channel as u8));
            weight_meter.consume(write_weight.saturating_mul(2));
        }
        ChannelMechanismConfigPurgeCursor::<T>::remove((netuid, mecid));
        true
    }

    fn clear_channel_netuid_swaps(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        loop {
            let item_weight = T::DbWeight::get().reads_writes(9, 13);
            if !weight_meter.can_consume(item_weight) {
                return false;
            }
            if ChannelSwapPendingHotkeys::<T>::iter_prefix(netuid)
                .next()
                .is_none()
            {
                break;
            }
            Self::drain_channel_swap_step(weight_meter, false);
        }
        Self::clear_channel_netuid_swaps_without_migration(netuid, weight_meter)
    }

    fn clear_channel_netuid_swaps_without_migration(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
    ) -> bool {
        let item_weight = T::DbWeight::get().reads_writes(1, 3);
        loop {
            if !weight_meter.can_consume(item_weight) {
                return false;
            }
            if let Some((old_hotkey, new_hotkey)) =
                ChannelSwapPendingHotkeys::<T>::iter_prefix(netuid).next()
            {
                ChannelSwapPendingHotkeys::<T>::remove(netuid, &old_hotkey);
                ChannelSwapOrigins::<T>::remove((netuid, &new_hotkey, &old_hotkey));
                weight_meter.consume(item_weight);
                continue;
            }
            return clear_prefix_with_meter(weight_meter, item_weight, |limit| {
                ChannelNeuronPurgeQueue::<T>::clear_prefix(netuid, limit, None)
            }) && clear_prefix_with_meter(weight_meter, item_weight, |limit| {
                ChannelSwapOrigins::<T>::clear_prefix((netuid,), limit, None)
            });
        }
    }

    pub fn purge_channel_netuid(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        let write_weight = T::DbWeight::get().writes(1);
        if !ChannelPurgePending::<T>::get(netuid) {
            if !weight_meter.can_consume(write_weight) {
                return false;
            }
            ChannelPurgePending::<T>::insert(netuid, true);
            weight_meter.consume(write_weight);
        }
        // Finish or cancel every rename before releasing any channel row. A
        // later drain must never move a deleted payer into a fresh row.
        if !Self::clear_channel_netuid_swaps(netuid, weight_meter) {
            return false;
        }
        for mechanism in 0..16u8 {
            let mecid = MechId::from(mechanism);
            if !Self::purge_channel_mechanism_rows(netuid, mecid, weight_meter, u32::MAX)
                || !Self::clear_channel_netuid_swaps_without_migration(netuid, weight_meter)
            {
                return false;
            }
            if !Self::purge_channel_mechanism_config(netuid, mecid, weight_meter) {
                return false;
            }
            ChannelMechanismPurgeQueue::<T>::remove(netuid, mecid);
            ChannelMechanismRowsDrained::<T>::remove((netuid, mecid));
        }
        ChannelTimelockRevealCursor::<T>::kill();
        if weight_meter.can_consume(write_weight) {
            ChannelPurgePending::<T>::remove(netuid);
            weight_meter.consume(write_weight);
            true
        } else {
            false
        }
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
