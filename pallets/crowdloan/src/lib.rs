//! # Crowdloan Pallet
//!
//! Generic crowdloan raise-and-finalize flow used by Bittensor (e.g. subnet leasing).
//!
//! Lifecycle:
//! 1. [`Pallet::create`] — creator posts a deposit and configures **exactly one**
//!    finalization route (`call` **xor** `target_address`).
//! 2. [`Pallet::contribute`] / [`Pallet::withdraw`] — raise funds until `cap` or `end`.
//! 3. Success path: [`Pallet::finalize`] (requires `raised == cap`).
//! 4. Failure path: [`Pallet::refund`] (batched) then [`Pallet::dissolve`].
//!
//! During call-based finalization, [`CurrentCrowdloanId`] is briefly set so the
//! dispatched call can read which crowdloan is being finalized.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{boxed::Box, vec};
use codec::{Decode, Encode};
use frame_support::{
    PalletId,
    dispatch::GetDispatchInfo,
    pallet_prelude::*,
    sp_runtime::{
        RuntimeDebug, Saturating,
        traits::{AccountIdConversion, Dispatchable, Zero},
    },
    traits::{
        Bounded, Defensive, Get, IsSubType, QueryPreimage, StorePreimage, fungible, fungible::*,
        tokens::Preservation,
    },
};
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_runtime::traits::CheckedSub;
use sp_std::vec::Vec;
use subtensor_runtime_common::TaoBalance;
use weights::WeightInfo;

pub use pallet::*;
use subtensor_macros::freeze_struct;

/// Incrementing identifier for a crowdloan; keys [`Crowdloans`] and related maps.
pub type CrowdloanId = u32;

mod benchmarking;
mod migrations;
mod mock;
mod tests;
pub mod weights;

/// Alias for the pallet's configured currency type.
pub type CurrencyOf<T> = <T as Config>::Currency;

/// Balance type of [`CurrencyOf`], in rao (TAO smallest unit) for this runtime.
pub type BalanceOf<T> =
    <CurrencyOf<T> as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

/// Max length of a `HasMigrationRun` key (`BoundedVec<u8, …>`).
type MigrationKeyMaxLen = ConstU32<128>;

/// Preimage-bounded runtime call stored on a crowdloan for call-based finalization.
pub type BoundedCallOf<T> =
    Bounded<<T as Config>::RuntimeCall, <T as frame_system::Config>::Hashing>;

/// On-chain record for one crowdloan (cap, timing, finalization route, raised total).
///
/// Invariant: exactly one of `call` or `target_address` is `Some` for a valid
/// creatable/finalizable crowdloan; both or neither yields [`Error::InvalidFinalizationConfig`].
#[freeze_struct("8a6ddd055c5a5c0b")]
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct CrowdloanInfo<AccountId, Balance, BlockNumber, Call> {
    /// Coldkey / account that created the crowdloan and may finalize, refund, or dissolve it.
    pub creator: AccountId,
    /// Creator's locked deposit (rao); counted in `raised` and not withdrawable until dissolve.
    pub deposit: Balance,
    /// Per-contribution floor (rao); also bounded by [`Config::AbsoluteMinimumContribution`].
    pub min_contribution: Balance,
    /// First block at which contributions are rejected (`now < end` required to contribute).
    pub end: BlockNumber,
    /// Maximum `raised` (rao); finalization requires `raised == cap`.
    pub cap: Balance,
    /// Pallet-derived account holding contributed TAO for this crowdloan id.
    pub funds_account: AccountId,
    /// Total TAO held toward the cap (includes creator deposit), in rao.
    pub raised: Balance,
    /// If set (and `call` is `None`), finalize transfers `raised` here.
    pub target_address: Option<AccountId>,
    /// If set (and `target_address` is `None`), finalize dispatches this preimage-bounded call.
    pub call: Option<Call>,
    /// Set true when [`Pallet::finalize`] succeeds; blocks further withdraw/refund/dissolve.
    pub finalized: bool,
    /// Distinct contributors with a nonzero [`Contributions`] entry (includes creator).
    pub contributors_count: u32,
}

pub type CrowdloanInfoOf<T> = CrowdloanInfo<
    <T as frame_system::Config>::AccountId,
    BalanceOf<T>,
    BlockNumberFor<T>,
    BoundedCallOf<T>,
>;

#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Runtime configuration for the crowdloan pallet.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Runtime call type; must be dispatchable and subtype-checkable for nested crowdloan calls.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>
            + IsSubType<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>;

        /// Fungible used for deposits and contributions (TAO / rao in production).
        type Currency: fungible::Balanced<Self::AccountId, Balance = TaoBalance>
            + fungible::Mutate<Self::AccountId>;

        /// Extrinsic weight benchmarks for this pallet.
        type WeightInfo: WeightInfo;

        /// Stores / peeks the optional finalize `call` preimage.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// Seed for deriving per-crowdloan [`CrowdloanInfo::funds_account`] sub-accounts.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// Floor on creator deposit at [`Pallet::create`] (rao).
        #[pallet::constant]
        type MinimumDeposit: Get<BalanceOf<Self>>;

        /// Global floor on `min_contribution` at create and update (rao).
        #[pallet::constant]
        type AbsoluteMinimumContribution: Get<BalanceOf<Self>>;

        /// Minimum `end - now` block span allowed for a crowdloan window.
        #[pallet::constant]
        type MinimumBlockDuration: Get<BlockNumberFor<Self>>;

        /// Maximum `end - now` block span allowed for a crowdloan window.
        #[pallet::constant]
        type MaximumBlockDuration: Get<BlockNumberFor<Self>>;

        /// Max non-creator contributors refunded per [`Pallet::refund`] extrinsic.
        #[pallet::constant]
        type RefundContributorsLimit: Get<u32>;

        /// Hard cap on [`CrowdloanInfo::contributors_count`] (includes creator).
        #[pallet::constant]
        type MaxContributors: Get<u32>;
    }

    /// Crowdloan id → [`CrowdloanInfo`] for every live (not yet dissolved) crowdloan.
    #[pallet::storage]
    pub type Crowdloans<T: Config> =
        StorageMap<_, Twox64Concat, CrowdloanId, CrowdloanInfoOf<T>, OptionQuery>;

    /// Next unused [`CrowdloanId`]; starts at 0 and increments on each successful create.
    #[pallet::storage]
    pub type NextCrowdloanId<T> = StorageValue<_, CrowdloanId, ValueQuery, ConstU32<0>>;

    /// Per-(crowdloan id, contributor) cumulative contribution balance in rao.
    #[pallet::storage]
    pub type Contributions<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        CrowdloanId,
        Identity,
        T::AccountId,
        BalanceOf<T>,
        OptionQuery,
    >;

    /// Optional per-contributor cumulative contribution ceiling (rao) for a crowdloan.
    ///
    /// Absent means no per-account max beyond the crowdloan `cap`.
    #[pallet::storage]
    pub type MaxContributions<T: Config> =
        StorageMap<_, Twox64Concat, CrowdloanId, BalanceOf<T>, OptionQuery>;

    /// Crowdloan id being finalized while a call-route finalize dispatches; otherwise `None`.
    ///
    /// Nested crowdloan extrinsics see [`Error::AlreadyFinalizing`] while this is set.
    #[pallet::storage]
    pub type CurrentCrowdloanId<T: Config> = StorageValue<_, CrowdloanId, OptionQuery>;

    /// Idempotency flags for named storage migrations (`true` once that migration has run).
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Emitted after [`Pallet::create`] stores a new crowdloan and takes the deposit.
        Created {
            crowdloan_id: CrowdloanId,
            creator: T::AccountId,
            end: BlockNumberFor<T>,
            cap: BalanceOf<T>,
        },
        /// Emitted after a successful [`Pallet::contribute`] (accepted amount may be clipped to room).
        Contributed {
            crowdloan_id: CrowdloanId,
            contributor: T::AccountId,
            amount: BalanceOf<T>,
        },
        /// Emitted after [`Pallet::withdraw`] returns contribution TAO to the contributor.
        Withdrew {
            crowdloan_id: CrowdloanId,
            contributor: T::AccountId,
            amount: BalanceOf<T>,
        },
        /// [`Pallet::refund`] hit [`Config::RefundContributorsLimit`] before clearing all non-creators.
        PartiallyRefunded { crowdloan_id: CrowdloanId },
        /// [`Pallet::refund`] returned every non-creator contribution in this call.
        AllRefunded { crowdloan_id: CrowdloanId },
        /// Cap reached and finalization route (transfer or call dispatch) completed.
        Finalized { crowdloan_id: CrowdloanId },
        /// Crowdloan storage, contributions, and funds account provider ref cleared after dissolve.
        Dissolved { crowdloan_id: CrowdloanId },
        /// Creator changed `CrowdloanInfo::min_contribution` via [`Pallet::update_min_contribution`].
        MinContributionUpdated {
            crowdloan_id: CrowdloanId,
            new_min_contribution: BalanceOf<T>,
        },
        /// Creator changed `CrowdloanInfo::end` via [`Pallet::update_end`].
        EndUpdated {
            crowdloan_id: CrowdloanId,
            new_end: BlockNumberFor<T>,
        },
        /// Creator changed `CrowdloanInfo::cap` via [`Pallet::update_cap`].
        CapUpdated {
            crowdloan_id: CrowdloanId,
            new_cap: BalanceOf<T>,
        },
        /// Creator set or cleared [`MaxContributions`] via [`Pallet::set_max_contribution`].
        MaxContributionUpdated {
            crowdloan_id: CrowdloanId,
            new_max_contribution: Option<BalanceOf<T>>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Creator deposit below [`Config::MinimumDeposit`].
        DepositTooLow,
        /// Cap not strictly above deposit (create) or below current `raised` (update).
        CapTooLow,
        /// `min_contribution` below [`Config::AbsoluteMinimumContribution`].
        MinimumContributionTooLow,
        /// Proposed `end` is not strictly after the current block.
        CannotEndInPast,
        /// `end - now` shorter than [`Config::MinimumBlockDuration`].
        BlockDurationTooShort,
        /// `end - now` longer than [`Config::MaximumBlockDuration`].
        BlockDurationTooLong,
        /// Signer lacks free balance for the deposit or contribution transfer.
        InsufficientBalance,
        /// Checked arithmetic overflow (ids, raised totals, or contributor counts).
        Overflow,
        /// No [`Crowdloans`] entry for the given id.
        InvalidCrowdloanId,
        /// Contributions rejected because `raised` already equals `cap`.
        CapRaised,
        /// Contributions rejected because `now >= end`.
        ContributionPeriodEnded,
        /// Requested contribution below the crowdloan's `min_contribution`.
        ContributionTooLow,
        /// Signed origin is not the crowdloan creator where creator-only is required.
        InvalidOrigin,
        /// Operation blocked because `CrowdloanInfo::finalized` is already true.
        AlreadyFinalized,
        /// Nested finalize attempted while [`CurrentCrowdloanId`] is set.
        AlreadyFinalizing,
        /// Reserved for contribution-period gating (not currently returned by extrinsics).
        ContributionPeriodNotEnded,
        /// Contributor has no [`Contributions`] row (or creator missing deposit row on dissolve).
        NoContribution,
        /// Finalize requires `raised == cap`.
        CapNotRaised,
        /// Checked arithmetic underflow.
        Underflow,
        /// Finalize call preimage missing from [`Config::Preimages`].
        CallUnavailable,
        /// Dissolve requires `raised` equal only to the creator's remaining contribution.
        NotReadyToDissolve,
        /// Creator tried to withdraw when only the locked deposit remains.
        DepositCannotBeWithdrawn,
        /// New contributor would exceed [`Config::MaxContributors`].
        MaxContributorsReached,
        /// Create/finalize config is not exactly one of `call` or `target_address`.
        InvalidFinalizationConfig,
        /// Contributor already at [`MaxContributions`] for this crowdloan.
        MaxContributionReached,
        /// New max contribution below `min_contribution` or creator's current contribution.
        MaximumContributionTooLow,
        /// New min contribution above the configured [`MaxContributions`] ceiling.
        MinimumContributionTooHigh,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_runtime_upgrade() -> frame_support::weights::Weight {
            let mut weight = frame_support::weights::Weight::from_parts(0, 0);

            weight = weight
                // Add the contributors count for each crowdloan
                .saturating_add(migrations::migrate_add_contributors_count::<T>());

            weight
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Create a crowdloan that will raise funds up to a maximum cap and if successful,
        /// will either transfer funds to the target address or dispatch the call
        /// (using creator origin). Exactly one of call or target address must be provided.
        /// Providing both, or providing neither, is rejected.
        ///
        /// The initial deposit will be transferred to the crowdloan account and will be refunded
        /// in case the crowdloan fails to raise the cap. Additionally, the creator will pay for
        /// the execution of the call.
        ///
        /// The dispatch origin for this call must be _Signed_.
        ///
        /// Parameters:
        /// - `deposit`: The initial deposit from the creator.
        /// - `min_contribution`: The minimum contribution required to contribute to the crowdloan.
        /// - `cap`: The maximum amount of funds that can be raised.
        /// - `end`: The block number at which the crowdloan will end.
        /// - `call`: The call to dispatch when the crowdloan is finalized.
        /// - `target_address`: The address to transfer the raised funds to.
        #[pallet::call_index(0)]
        #[pallet::weight({
			let di = call.as_ref().map(|c| c.get_dispatch_info());
			let inner_call_weight = match di {
				Some(di) => di.call_weight,
				None => Weight::zero(),
			};
			let base_weight = T::WeightInfo::create();
			(base_weight.saturating_add(inner_call_weight), Pays::Yes)
		})]
        pub fn create(
            origin: OriginFor<T>,
            #[pallet::compact] deposit: BalanceOf<T>,
            #[pallet::compact] min_contribution: BalanceOf<T>,
            #[pallet::compact] cap: BalanceOf<T>,
            #[pallet::compact] end: BlockNumberFor<T>,
            call: Option<Box<<T as Config>::RuntimeCall>>,
            target_address: Option<T::AccountId>,
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;
            let now = frame_system::Pallet::<T>::block_number();

            // Ensure the deposit is at least the minimum deposit, cap is greater than deposit
            // and the minimum contribution is greater than the absolute minimum contribution.
            ensure!(
                deposit >= T::MinimumDeposit::get(),
                Error::<T>::DepositTooLow
            );
            ensure!(cap > deposit, Error::<T>::CapTooLow);
            ensure!(
                min_contribution >= T::AbsoluteMinimumContribution::get(),
                Error::<T>::MinimumContributionTooLow
            );
            ensure!(
                call.is_some() != target_address.is_some(),
                Error::<T>::InvalidFinalizationConfig
            );

            Self::ensure_crowdloan_end_in_window(now, end)?;

            // Ensure the creator has enough balance to pay the initial deposit
            ensure!(
                CurrencyOf::<T>::balance(&creator) >= deposit,
                Error::<T>::InsufficientBalance
            );

            let crowdloan_id = NextCrowdloanId::<T>::get();
            let next_crowdloan_id = crowdloan_id.checked_add(1).ok_or(Error::<T>::Overflow)?;
            NextCrowdloanId::<T>::put(next_crowdloan_id);

            // Derive the funds account and keep track of it
            let funds_account = Self::crowdloan_funds_account(crowdloan_id);
            frame_system::Pallet::<T>::inc_providers(&funds_account);

            // If the call is provided, bound it and store it in the preimage storage
            let call = if let Some(call) = call {
                Some(T::Preimages::bound(*call)?)
            } else {
                None
            };

            let crowdloan = CrowdloanInfo {
                creator: creator.clone(),
                deposit,
                min_contribution,
                end,
                cap,
                funds_account,
                raised: deposit,
                target_address,
                call,
                finalized: false,
                contributors_count: 1,
            };
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            // Transfer the deposit to the funds account
            CurrencyOf::<T>::transfer(
                &creator,
                &crowdloan.funds_account,
                deposit,
                Preservation::Expendable,
            )?;

            Contributions::<T>::insert(crowdloan_id, &creator, deposit);

            Self::deposit_event(Event::<T>::Created {
                crowdloan_id,
                creator,
                end,
                cap,
            });

            Ok(())
        }

        /// Contribute to an active crowdloan.
        ///
        /// The contribution will be transferred to the crowdloan account and will be refunded
        /// if the crowdloan fails to raise the cap. If the contribution would raise the amount above the cap,
        /// the contribution will be set to the amount that is left to be raised.
        ///
        /// The dispatch origin for this call must be _Signed_.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to contribute to.
        /// - `amount`: The amount to contribute.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::contribute())]
        pub fn contribute(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
            #[pallet::compact] amount: BalanceOf<T>,
        ) -> DispatchResult {
            let contributor = ensure_signed(origin)?;
            let now = frame_system::Pallet::<T>::block_number();

            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;

            // Ensure crowdloan has not ended and has not raised cap
            ensure!(now < crowdloan.end, Error::<T>::ContributionPeriodEnded);
            ensure!(crowdloan.raised < crowdloan.cap, Error::<T>::CapRaised);

            // Ensure contribution is at least the minimum contribution
            ensure!(
                amount >= crowdloan.min_contribution,
                Error::<T>::ContributionTooLow
            );

            // Ensure the crowdloan has not reached the maximum number of contributors
            ensure!(
                crowdloan.contributors_count < T::MaxContributors::get(),
                Error::<T>::MaxContributorsReached
            );

            // Compute how much room is left before the crowdloan reaches its cap.
            let left_to_raise = crowdloan
                .cap
                .checked_sub(&crowdloan.raised)
                .ok_or(Error::<T>::Underflow)?;

            // The requested contribution must meet the minimum contribution, but
            // the accepted amount may be lower when only a smaller remainder can
            // be accepted before reaching the crowdloan cap or the contributor's
            // maximum contribution.
            let amount = if let Some(max_contribution) = MaxContributions::<T>::get(crowdloan_id) {
                let current_contribution =
                    Contributions::<T>::get(crowdloan_id, &contributor).unwrap_or_else(Zero::zero);
                ensure!(
                    current_contribution < max_contribution,
                    Error::<T>::MaxContributionReached
                );
                let left_to_contribute = max_contribution
                    .checked_sub(&current_contribution)
                    .ok_or(Error::<T>::Underflow)?;
                amount.min(left_to_contribute).min(left_to_raise)
            } else {
                amount.min(left_to_raise)
            };

            // Ensure contribution does not overflow the actual raised amount
            crowdloan.raised = crowdloan
                .raised
                .checked_add(&amount)
                .ok_or(Error::<T>::Overflow)?;

            // Compute the new total contribution and ensure it does not overflow, we
            // also increment the contributor count if the contribution is new.
            let contribution =
                if let Some(contribution) = Contributions::<T>::get(crowdloan_id, &contributor) {
                    contribution
                        .checked_add(&amount)
                        .ok_or(Error::<T>::Overflow)?
                } else {
                    // We have a new contribution
                    crowdloan.contributors_count = crowdloan
                        .contributors_count
                        .checked_add(1)
                        .ok_or(Error::<T>::Overflow)?;
                    amount
                };

            // Ensure contributor has enough balance to pay
            ensure!(
                CurrencyOf::<T>::balance(&contributor) >= amount,
                Error::<T>::InsufficientBalance
            );

            CurrencyOf::<T>::transfer(
                &contributor,
                &crowdloan.funds_account,
                amount,
                Preservation::Expendable,
            )?;

            Contributions::<T>::insert(crowdloan_id, &contributor, contribution);
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            Self::deposit_event(Event::<T>::Contributed {
                crowdloan_id,
                contributor,
                amount,
            });

            Ok(())
        }

        /// Withdraw a contribution from an active (not yet finalized or dissolved) crowdloan.
        ///
        /// Only contributions over the deposit can be withdrawn by the creator.
        ///
        /// The dispatch origin for this call must be _Signed_.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to withdraw from.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::withdraw())]
        pub fn withdraw(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Ensure contributor has balance left in the crowdloan account
            let mut amount = Contributions::<T>::get(crowdloan_id, &who).unwrap_or_else(Zero::zero);
            ensure!(amount > Zero::zero(), Error::<T>::NoContribution);

            if who == crowdloan.creator {
                // Ensure the deposit is kept
                amount = amount.saturating_sub(crowdloan.deposit);
                ensure!(amount > Zero::zero(), Error::<T>::DepositCannotBeWithdrawn);
                Contributions::<T>::insert(crowdloan_id, &who, crowdloan.deposit);
            } else {
                Contributions::<T>::remove(crowdloan_id, &who);
                crowdloan.contributors_count = crowdloan
                    .contributors_count
                    .checked_sub(1)
                    .ok_or(Error::<T>::Underflow)?;
            }

            CurrencyOf::<T>::transfer(
                &crowdloan.funds_account,
                &who,
                amount,
                Preservation::Expendable,
            )?;

            // Update the crowdloan raised amount to reflect the withdrawal.
            crowdloan.raised = crowdloan.raised.saturating_sub(amount);
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            Self::deposit_event(Event::<T>::Withdrew {
                contributor: who,
                crowdloan_id,
                amount,
            });

            Ok(())
        }

        /// Finalize crowdloan that has reached the cap.
        ///
        /// The call will either transfer the raised amount to the configured target address
        /// or dispatch the configured call using the creator origin. The stored crowdloan
        /// must contain exactly one of target address or call; if both or neither are set,
        /// finalization fails before transfer or dispatch.
        ///
        /// When dispatching a call, the CurrentCrowdloanId will be set to the crowdloan id
        /// being finalized so the dispatched call can access it temporarily by accessing
        /// the `CurrentCrowdloanId` storage item.
        ///
        /// The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to finalize.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::finalize())]
        pub fn finalize(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;

            // Ensure the origin is the creator of the crowdloan and the crowdloan has raised the cap
            // and is not finalized.
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);
            ensure!(crowdloan.raised == crowdloan.cap, Error::<T>::CapNotRaised);
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);
            ensure!(
                CurrentCrowdloanId::<T>::get().is_none(),
                Error::<T>::AlreadyFinalizing
            );

            crowdloan.finalized = true;
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            match (&crowdloan.call, &crowdloan.target_address) {
                (Some(call), None) => {
                    // Set the current crowdloan id so the dispatched call
                    // can access it temporarily
                    CurrentCrowdloanId::<T>::put(crowdloan_id);

                    // Retrieve the call from the preimage storage
                    let stored_call = match T::Preimages::peek(call) {
                        Ok((call, _)) => call,
                        Err(_) => {
                            // If the call is not found, we drop it from the preimage storage
                            // because it's not needed anymore
                            T::Preimages::drop(call);
                            return Err(Error::<T>::CallUnavailable)?;
                        }
                    };

                    // Dispatch the call with creator origin
                    stored_call
                        .dispatch(frame_system::RawOrigin::Signed(who).into())
                        .map(|_| ())
                        .map_err(|e| e.error)?;

                    // Clear the current crowdloan id
                    CurrentCrowdloanId::<T>::kill();
                }
                (None, Some(target_address)) => {
                    CurrencyOf::<T>::transfer(
                        &crowdloan.funds_account,
                        target_address,
                        crowdloan.raised,
                        Preservation::Expendable,
                    )?;
                }
                (_, _) => {
                    return Err(Error::<T>::InvalidFinalizationConfig)?;
                }
            }

            Self::deposit_event(Event::<T>::Finalized { crowdloan_id });

            Ok(())
        }

        /// Refund contributors of a non-finalized crowdloan.
        ///
        /// The call will try to refund all contributors (excluding the creator) up to the limit defined by the `RefundContributorsLimit`.
        /// If the limit is reached, the call will stop and the crowdloan will be marked as partially refunded.
        /// It may be needed to dispatch this call multiple times to refund all contributors.
        ///
        /// The dispatch origin for this call must be _Signed_ and doesn't need to be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to refund.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::refund(T::RefundContributorsLimit::get()))]
        pub fn refund(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin)?;

            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;

            // Ensure the crowdloan is not finalized
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can refund the crowdloan
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            let mut refunded_contributors: Vec<T::AccountId> = vec![];
            let mut refund_count = 0;

            // Assume everyone can be refunded
            let mut all_refunded = true;

            // We try to refund all contributors (excluding the creator)
            let contributions = Contributions::<T>::iter_prefix(crowdloan_id)
                .filter(|(contributor, _)| *contributor != crowdloan.creator);
            for (contributor, amount) in contributions {
                if refund_count >= T::RefundContributorsLimit::get() {
                    // Not everyone can be refunded
                    all_refunded = false;
                    break;
                }

                CurrencyOf::<T>::transfer(
                    &crowdloan.funds_account,
                    &contributor,
                    amount,
                    Preservation::Expendable,
                )?;

                refunded_contributors.push(contributor);
                crowdloan.raised = crowdloan.raised.saturating_sub(amount);
                refund_count = refund_count.checked_add(1).ok_or(Error::<T>::Overflow)?;
            }

            crowdloan.contributors_count = crowdloan
                .contributors_count
                .checked_sub(refund_count)
                .ok_or(Error::<T>::Underflow)?;
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            // Clear refunded contributors
            for contributor in refunded_contributors {
                Contributions::<T>::remove(crowdloan_id, &contributor);
            }

            if all_refunded {
                Self::deposit_event(Event::<T>::AllRefunded { crowdloan_id });
                // The loop didn't run fully, we refund the unused weights.
                Ok(Some(T::WeightInfo::refund(refund_count)).into())
            } else {
                Self::deposit_event(Event::<T>::PartiallyRefunded { crowdloan_id });
                // The loop ran fully, we don't refund anything.
                Ok(().into())
            }
        }

        /// Dissolve a crowdloan.
        ///
        /// The crowdloan will be removed from the storage.
        /// All contributions must have been refunded before the crowdloan can be dissolved (except the creator's one).
        ///
        /// The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to dissolve.
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::dissolve())]
        pub fn dissolve(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let crowdloan = Self::require_crowdloan(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can dissolve the crowdloan
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            // It can only be dissolved if the raised amount is the creator's contribution,
            // meaning there is no contributions or every contribution has been refunded
            let creator_contribution = Contributions::<T>::get(crowdloan_id, &crowdloan.creator)
                .ok_or(Error::<T>::NoContribution)?;
            ensure!(
                creator_contribution == crowdloan.raised,
                Error::<T>::NotReadyToDissolve
            );

            // Refund the creator's contribution
            CurrencyOf::<T>::transfer(
                &crowdloan.funds_account,
                &crowdloan.creator,
                creator_contribution,
                Preservation::Expendable,
            )?;
            Contributions::<T>::remove(crowdloan_id, &crowdloan.creator);

            // Clear the call from the preimage storage
            if let Some(call) = crowdloan.call {
                T::Preimages::drop(&call);
            }

            // Remove the crowdloan
            let _ = frame_system::Pallet::<T>::dec_providers(&crowdloan.funds_account).defensive();
            Crowdloans::<T>::remove(crowdloan_id);
            MaxContributions::<T>::remove(crowdloan_id);

            Self::deposit_event(Event::<T>::Dissolved { crowdloan_id });
            Ok(())
        }

        /// Update the minimum contribution of a non-finalized crowdloan.
        ///
        /// If a maximum contribution is configured, the new minimum contribution
        /// must not exceed it.
        ///
        /// The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to update the minimum contribution of.
        /// - `new_min_contribution`: The new minimum contribution.
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::update_min_contribution())]
        pub fn update_min_contribution(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
            #[pallet::compact] new_min_contribution: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can update the min contribution.
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            // The new min contribution should be greater than absolute minimum contribution.
            ensure!(
                new_min_contribution >= T::AbsoluteMinimumContribution::get(),
                Error::<T>::MinimumContributionTooLow
            );
            if let Some(max_contribution) = MaxContributions::<T>::get(crowdloan_id) {
                ensure!(
                    new_min_contribution <= max_contribution,
                    Error::<T>::MinimumContributionTooHigh
                );
            }

            crowdloan.min_contribution = new_min_contribution;
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            Self::deposit_event(Event::<T>::MinContributionUpdated {
                crowdloan_id,
                new_min_contribution,
            });
            Ok(())
        }

        /// Update the end block of a non-finalized crowdloan.
        ///
        /// The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to update the end block of.
        /// - `new_end`: The new end block.
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::update_end())]
        pub fn update_end(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
            #[pallet::compact] new_end: BlockNumberFor<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let now = frame_system::Pallet::<T>::block_number();

            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can update the min contribution.
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            Self::ensure_crowdloan_end_in_window(now, new_end)?;

            crowdloan.end = new_end;
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            Self::deposit_event(Event::<T>::EndUpdated {
                crowdloan_id,
                new_end,
            });
            Ok(())
        }

        /// Update the cap of a non-finalized crowdloan.
        ///
        /// The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to update the cap of.
        /// - `new_cap`: The new cap.
        #[pallet::call_index(8)]
        #[pallet::weight(T::WeightInfo::update_cap())]
        pub fn update_cap(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
            #[pallet::compact] new_cap: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            // The cap can only be updated if the crowdloan has not been finalized.
            let mut crowdloan = Self::require_crowdloan(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can update the cap.
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            // The new cap should be greater than the actual raised amount.
            ensure!(new_cap >= crowdloan.raised, Error::<T>::CapTooLow);

            crowdloan.cap = new_cap;
            Crowdloans::<T>::insert(crowdloan_id, &crowdloan);

            Self::deposit_event(Event::<T>::CapUpdated {
                crowdloan_id,
                new_cap,
            });
            Ok(())
        }

        /// Set or clear the maximum cumulative contribution allowed per contributor
        /// for a non-finalized crowdloan.
        ///
        /// The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
        ///
        /// Parameters:
        /// - `crowdloan_id`: The id of the crowdloan to update the maximum contribution of.
        /// - `new_max_contribution`: The new optional maximum contribution.
        #[pallet::call_index(9)]
        #[pallet::weight(T::WeightInfo::set_max_contribution())]
        pub fn set_max_contribution(
            origin: OriginFor<T>,
            #[pallet::compact] crowdloan_id: CrowdloanId,
            new_max_contribution: Option<BalanceOf<T>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let crowdloan = Self::require_crowdloan(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can update the max contribution.
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            if let Some(max_contribution) = new_max_contribution {
                let creator_contribution =
                    Contributions::<T>::get(crowdloan_id, &crowdloan.creator)
                        .unwrap_or_else(Zero::zero);
                ensure!(
                    max_contribution >= crowdloan.min_contribution
                        && max_contribution >= creator_contribution,
                    Error::<T>::MaximumContributionTooLow
                );
                MaxContributions::<T>::insert(crowdloan_id, max_contribution);
            } else {
                MaxContributions::<T>::remove(crowdloan_id);
            }

            Self::deposit_event(Event::<T>::MaxContributionUpdated {
                crowdloan_id,
                new_max_contribution,
            });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    /// Derive the custodial account that holds TAO for `crowdloan_id` from [`Config::PalletId`].
    pub(crate) fn crowdloan_funds_account(crowdloan_id: CrowdloanId) -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating(crowdloan_id)
    }

    /// Load [`Crowdloans`] entry or [`Error::InvalidCrowdloanId`].
    fn require_crowdloan(crowdloan_id: CrowdloanId) -> Result<CrowdloanInfoOf<T>, Error<T>> {
        Crowdloans::<T>::get(crowdloan_id).ok_or(Error::<T>::InvalidCrowdloanId)
    }

    /// Reject `end` in the past or outside [`Config::MinimumBlockDuration`] /
    /// [`Config::MaximumBlockDuration`] relative to `now`.
    fn ensure_crowdloan_end_in_window(
        now: BlockNumberFor<T>,
        end: BlockNumberFor<T>,
    ) -> Result<(), Error<T>> {
        ensure!(now < end, Error::<T>::CannotEndInPast);
        let block_duration = end.checked_sub(&now).ok_or(Error::<T>::Underflow)?;
        ensure!(
            block_duration >= T::MinimumBlockDuration::get(),
            Error::<T>::BlockDurationTooShort
        );
        ensure!(
            block_duration <= T::MaximumBlockDuration::get(),
            Error::<T>::BlockDurationTooLong
        );
        Ok(())
    }
}
