//! # Crowdloan Pallet
//!
//! A pallet allowing users to create generic crowdloans and contribute to them,
//! then finalize them through exactly one configured route: transfer the raised
//! funds to a target address or dispatch an extrinsic, making it reusable for any
//! crowdloan type.
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

pub type CrowdloanId = u32;

mod benchmarking;
mod migrations;
mod mock;
mod tests;
pub mod weights;

pub type CurrencyOf<T> = <T as Config>::Currency;

pub type BalanceOf<T> =
    <CurrencyOf<T> as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

// Define a maximum length for the migration key
type MigrationKeyMaxLen = ConstU32<128>;

pub type BoundedCallOf<T> =
    Bounded<<T as Config>::RuntimeCall, <T as frame_system::Config>::Hashing>;

/// A struct containing the information about a crowdloan.
#[freeze_struct("5db9538284491545")]
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct CrowdloanInfo<AccountId, Balance, BlockNumber, Call> {
    /// The creator of the crowdloan.
    pub creator: AccountId,
    /// The initial deposit of the crowdloan from the creator.
    pub deposit: Balance,
    /// Minimum contribution to the crowdloan.
    pub min_contribution: Balance,
    /// The end block of the crowdloan.
    pub end: BlockNumber,
    /// The cap to raise.
    pub cap: Balance,
    /// The account holding the funds for this crowdloan. Derived on chain but put here for ease of use.
    pub funds_account: AccountId,
    /// The amount raised so far.
    pub raised: Balance,
    /// The optional target address to transfer the raised funds to, if not
    /// provided, it means the funds will be transferred from on chain logic
    /// inside the provided call to dispatch.
    pub target_address: Option<AccountId>,
    /// The optional call to dispatch when the crowdloan is finalized.
    pub call: Option<Call>,
    /// Whether the crowdloan has been finalized.
    pub finalized: bool,
    /// The number of contributors to the crowdloan.
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

    /// Configuration trait.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching call type.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>
            + IsSubType<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>;

        /// The currency mechanism.
        type Currency: fungible::Balanced<Self::AccountId, Balance = TaoBalance>
            + fungible::Mutate<Self::AccountId>;

        /// The weight information for the pallet.
        type WeightInfo: WeightInfo;

        /// The preimage provider which will be used to store the call to dispatch.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// The pallet id that will be used to derive crowdloan account ids.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// The minimum deposit required to create a crowdloan.
        #[pallet::constant]
        type MinimumDeposit: Get<BalanceOf<Self>>;

        /// The absolute minimum contribution required to contribute to a crowdloan.
        #[pallet::constant]
        type AbsoluteMinimumContribution: Get<BalanceOf<Self>>;

        /// The minimum block duration for a crowdloan.
        #[pallet::constant]
        type MinimumBlockDuration: Get<BlockNumberFor<Self>>;

        /// The maximum block duration for a crowdloan.
        #[pallet::constant]
        type MaximumBlockDuration: Get<BlockNumberFor<Self>>;

        /// The maximum number of contributors that can be refunded in a single refund.
        #[pallet::constant]
        type RefundContributorsLimit: Get<u32>;

        // The maximum number of contributors that can contribute to a crowdloan.
        #[pallet::constant]
        type MaxContributors: Get<u32>;
    }

    /// A map of crowdloan ids to their information.
    #[pallet::storage]
    pub type Crowdloans<T: Config> =
        StorageMap<_, Twox64Concat, CrowdloanId, CrowdloanInfoOf<T>, OptionQuery>;

    /// The next incrementing crowdloan id.
    #[pallet::storage]
    pub type NextCrowdloanId<T> = StorageValue<_, CrowdloanId, ValueQuery, ConstU32<0>>;

    /// A map of crowdloan ids to their contributors and their contributions.
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

    /// A map of crowdloan ids to their optional maximum cumulative contribution per contributor.
    #[pallet::storage]
    pub type MaxContributions<T: Config> =
        StorageMap<_, Twox64Concat, CrowdloanId, BalanceOf<T>, OptionQuery>;

    /// The current crowdloan id that will be set during the finalize call, making it
    /// temporarily accessible to the dispatched call.
    #[pallet::storage]
    pub type CurrentCrowdloanId<T: Config> = StorageValue<_, CrowdloanId, OptionQuery>;

    /// Storage for the migration run status.
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A crowdloan was created.
        Created {
            crowdloan_id: CrowdloanId,
            creator: T::AccountId,
            end: BlockNumberFor<T>,
            cap: BalanceOf<T>,
        },
        /// A contribution was made to an active crowdloan.
        Contributed {
            crowdloan_id: CrowdloanId,
            contributor: T::AccountId,
            amount: BalanceOf<T>,
        },
        /// A contribution was withdrawn from a failed crowdloan.
        Withdrew {
            crowdloan_id: CrowdloanId,
            contributor: T::AccountId,
            amount: BalanceOf<T>,
        },
        /// A refund was partially processed for a failed crowdloan.
        PartiallyRefunded { crowdloan_id: CrowdloanId },
        /// A refund was fully processed for a failed crowdloan.
        AllRefunded { crowdloan_id: CrowdloanId },
        /// A crowdloan was finalized, funds were transferred and the call was dispatched.
        Finalized { crowdloan_id: CrowdloanId },
        /// A crowdloan was dissolved.
        Dissolved { crowdloan_id: CrowdloanId },
        /// The minimum contribution was updated.
        MinContributionUpdated {
            crowdloan_id: CrowdloanId,
            new_min_contribution: BalanceOf<T>,
        },
        /// The end was updated.
        EndUpdated {
            crowdloan_id: CrowdloanId,
            new_end: BlockNumberFor<T>,
        },
        /// The cap was updated.
        CapUpdated {
            crowdloan_id: CrowdloanId,
            new_cap: BalanceOf<T>,
        },
        /// The maximum contribution was updated.
        MaxContributionUpdated {
            crowdloan_id: CrowdloanId,
            new_max_contribution: Option<BalanceOf<T>>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The crowdloan initial deposit is too low.
        DepositTooLow,
        /// The crowdloan cap is too low.
        CapTooLow,
        /// The minimum contribution is too low.
        MinimumContributionTooLow,
        /// The crowdloan cannot end in the past.
        CannotEndInPast,
        /// The crowdloan block duration is too short.
        BlockDurationTooShort,
        /// The block duration is too long.
        BlockDurationTooLong,
        /// The account does not have enough balance to pay for the initial deposit/contribution.
        InsufficientBalance,
        /// An overflow occurred.
        Overflow,
        /// The crowdloan id is invalid.
        InvalidCrowdloanId,
        /// The crowdloan cap has been fully raised.
        CapRaised,
        /// The contribution period has ended.
        ContributionPeriodEnded,
        /// The contribution is too low.
        ContributionTooLow,
        /// The origin of this call is invalid.
        InvalidOrigin,
        /// The crowdloan has already been finalized.
        AlreadyFinalized,
        /// A crowdloan finalization is already in progress.
        AlreadyFinalizing,
        /// The crowdloan contribution period has not ended yet.
        ContributionPeriodNotEnded,
        /// The contributor has no contribution for this crowdloan.
        NoContribution,
        /// The crowdloan cap has not been raised.
        CapNotRaised,
        /// An underflow occurred.
        Underflow,
        /// Call to dispatch was not found in the preimage storage.
        CallUnavailable,
        /// The crowdloan is not ready to be dissolved, it still has contributions.
        NotReadyToDissolve,
        /// The deposit cannot be withdrawn from the crowdloan.
        DepositCannotBeWithdrawn,
        /// The maximum number of contributors has been reached.
        MaxContributorsReached,
        /// Exactly one of call or target address must be provided.
        InvalidFinalizationConfig,
        /// The contributor has already reached the maximum contribution.
        MaxContributionReached,
        /// The maximum contribution is too low.
        MaximumContributionTooLow,
        /// The minimum contribution is too high.
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

            Self::ensure_valid_end(now, end)?;

            // Ensure the creator has enough balance to pay the initial deposit
            ensure!(
                CurrencyOf::<T>::balance(&creator) >= deposit,
                Error::<T>::InsufficientBalance
            );

            let crowdloan_id = NextCrowdloanId::<T>::get();
            let next_crowdloan_id = crowdloan_id.checked_add(1).ok_or(Error::<T>::Overflow)?;
            NextCrowdloanId::<T>::put(next_crowdloan_id);

            // Derive the funds account and keep track of it
            let funds_account = Self::funds_account(crowdloan_id);
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

            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;

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

            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;
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

            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;

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

            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;

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

            let crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;
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

            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;
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

            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;
            ensure!(!crowdloan.finalized, Error::<T>::AlreadyFinalized);

            // Only the creator can update the min contribution.
            ensure!(who == crowdloan.creator, Error::<T>::InvalidOrigin);

            Self::ensure_valid_end(now, new_end)?;

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
            let mut crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;
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

            let crowdloan = Self::ensure_crowdloan_exists(crowdloan_id)?;
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
    fn funds_account(id: CrowdloanId) -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating(id)
    }

    fn ensure_crowdloan_exists(crowdloan_id: CrowdloanId) -> Result<CrowdloanInfoOf<T>, Error<T>> {
        Crowdloans::<T>::get(crowdloan_id).ok_or(Error::<T>::InvalidCrowdloanId)
    }

    // Ensure the provided end block is after the current block and the duration is
    // between the minimum and maximum block duration
    fn ensure_valid_end(now: BlockNumberFor<T>, end: BlockNumberFor<T>) -> Result<(), Error<T>> {
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
