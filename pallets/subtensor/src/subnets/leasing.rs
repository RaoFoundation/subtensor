//! This file defines abstraction for subnet leasing.
//!
//! It is used to register a new leased network through a crowdloan using the `register_leased_network` extrinsic
//! as a call parameter to the crowdloan pallet `create` extrinsic. A new subnet will be registered
//! paying the lock cost using the crowdloan funds and a proxy will be created for the beneficiary
//! to operate the subnet.
//!
//! The crowdloan's contributions are used to compute the share of the emissions that the contributors
//! will receive as dividends. The leftover cap is refunded to the contributors and the beneficiary.
//!
//! The lease can have a defined end block, after which the lease will be terminated and the subnet
//! will be transferred to the beneficiary. In case the lease is perpetual, the lease will never be
//! terminated and emissions will continue to be distributed to the contributors.
//!
//! The lease can be terminated by the beneficiary after the end block has passed (if any) and the subnet
//! ownership will be transferred to the beneficiary.

use super::*;
use crate::weights::WeightInfo;
use frame_support::{
    dispatch::RawOrigin,
    traits::{Defensive, fungible::*},
};
use frame_system::pallet_prelude::OriginFor;
use frame_system::pallet_prelude::*;
use sp_core::blake2_256;
use sp_runtime::{Percent, traits::TrailingZeroInput};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid};

pub type LeaseId = u32;

pub type CurrencyOf<T> = <T as Config>::Currency;

pub type BalanceOf<T> =
    <CurrencyOf<T> as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

#[freeze_struct("8cc3d0594faed7dd")]
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, RuntimeDebug, TypeInfo)]
pub struct SubnetLease<AccountId, BlockNumber, Balance> {
    /// The beneficiary of the lease, able to operate the subnet through
    /// a proxy and taking ownership of the subnet at the end of the lease (if defined).
    pub beneficiary: AccountId,
    /// The coldkey of the lease.
    pub coldkey: AccountId,
    /// The hotkey of the lease.
    pub hotkey: AccountId,
    /// The share of the emissions that the contributors will receive.
    pub emissions_share: Percent,
    /// The block at which the lease will end. If not defined, the lease is perpetual.
    pub end_block: Option<BlockNumber>,
    /// The netuid of the subnet that the lease is for.
    pub netuid: NetUid,
    /// The cost of the lease including the network registration and proxy.
    pub cost: Balance,
}

pub type SubnetLeaseOf<T> =
    SubnetLease<<T as frame_system::Config>::AccountId, BlockNumberFor<T>, BalanceOf<T>>;

impl<T: Config> Pallet<T> {
    /// Register a new leased network through a crowdloan. A new subnet will be registered
    /// paying the lock cost using the crowdloan funds and a proxy will be created for the beneficiary
    /// to operate the subnet.
    ///
    /// The crowdloan's contributions are used to compute the share of the emissions that the contributors
    /// will receive as dividends.
    ///
    /// The leftover cap is refunded to the contributors and the beneficiary.
    pub fn do_register_leased_network(
        origin: OriginFor<T>,
        emissions_share: Percent,
        end_block: Option<BlockNumberFor<T>>,
    ) -> DispatchResultWithPostInfo {
        let who = ensure_signed(origin)?;
        let now = frame_system::Pallet::<T>::block_number();

        // Ensure the origin is the creator of the crowdloan
        let (crowdloan_id, crowdloan) = Self::get_crowdloan_being_finalized()?;
        ensure!(
            who == crowdloan.creator,
            Error::<T>::InvalidLeaseBeneficiary
        );

        if let Some(end_block) = end_block {
            ensure!(end_block > now, Error::<T>::LeaseCannotEndInThePast);
        }

        // Initialize the lease id, coldkey and hotkey and keep track of them
        let lease_id = Self::get_next_lease_id()?;
        let lease_coldkey = Self::lease_coldkey(lease_id)?;
        let lease_hotkey = Self::lease_hotkey(lease_id)?;
        frame_system::Pallet::<T>::inc_providers(&lease_coldkey);
        frame_system::Pallet::<T>::inc_providers(&lease_hotkey);

        Self::transfer_tao(&crowdloan.funds_account, &lease_coldkey, crowdloan.raised)?;

        Self::do_register_network(
            RawOrigin::Signed(lease_coldkey.clone()).into(),
            &lease_hotkey,
            1,
            None,
        )?;

        let netuid =
            Self::find_lease_netuid(&lease_coldkey).ok_or(Error::<T>::LeaseNetuidNotFound)?;

        // Enable the beneficiary to operate the subnet through a proxy
        T::ProxyInterface::add_lease_beneficiary_proxy(&lease_coldkey, &who)?;

        // Get left leftover cap and compute the cost of the registration + proxy
        let leftover_cap = <T as Config>::Currency::balance(&lease_coldkey);
        let cost = crowdloan.raised.saturating_sub(leftover_cap);

        SubnetLeases::<T>::insert(
            lease_id,
            SubnetLease {
                beneficiary: who.clone(),
                coldkey: lease_coldkey.clone(),
                hotkey: lease_hotkey.clone(),
                emissions_share,
                end_block,
                netuid,
                cost,
            },
        );
        SubnetUidToLeaseId::<T>::insert(netuid, lease_id);

        // The lease take should be 0% to allow all contributors to receive dividends entirely.
        Self::delegate_hotkey(&lease_hotkey, 0);

        // Get all the contributions to the crowdloan except for the beneficiary
        // because its share will be computed as the dividends are distributed
        let contributions = pallet_crowdloan::Contributions::<T>::iter_prefix(crowdloan_id)
            .into_iter()
            .filter(|(contributor, _)| contributor != &who);

        let mut refunded_cap = 0u64;
        for (contributor, amount) in contributions {
            // Compute the share of the contributor to the lease
            let share: U64F64 = U64F64::from(u64::from(amount))
                .saturating_div(U64F64::from(u64::from(crowdloan.raised)));
            SubnetLeaseShares::<T>::insert(lease_id, &contributor, share);

            // Refund the unused part of the cap to the contributor relative to their share
            let contributor_refund = share
                .saturating_mul(U64F64::from(u64::from(leftover_cap)))
                .floor()
                .saturating_to_num::<u64>();
            Self::transfer_tao(&lease_coldkey, &contributor, contributor_refund.into())?;
            refunded_cap = refunded_cap.saturating_add(contributor_refund);
        }

        // Refund what's left after refunding the contributors to the beneficiary
        let beneficiary_refund = leftover_cap.saturating_sub(refunded_cap.into());
        Self::transfer_tao(&lease_coldkey, &who, beneficiary_refund)?;

        Self::deposit_event(Event::SubnetLeaseCreated {
            beneficiary: who,
            lease_id,
            netuid,
            end_block,
        });

        if crowdloan.contributors_count < T::MaxContributors::get() {
            // We have less contributors than the max allowed, so we need to refund the difference
            Ok(Some(<T as Config>::WeightInfo::register_leased_network(
                crowdloan.contributors_count,
            ))
            .into())
        } else {
            // We have the max number of contributors, so we don't need to refund anything
            Ok(().into())
        }
    }

    /// Terminate a lease.
    ///
    /// The beneficiary can terminate the lease after the end block has passed and get the subnet ownership.
    /// The subnet is transferred to the beneficiary and the lease is removed from storage.
    pub fn do_terminate_lease(
        origin: OriginFor<T>,
        lease_id: LeaseId,
        hotkey: T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let who = ensure_signed(origin)?;
        let now = frame_system::Pallet::<T>::block_number();

        // Ensure the lease exists and the beneficiary is the caller
        let lease = SubnetLeases::<T>::get(lease_id).ok_or(Error::<T>::LeaseDoesNotExist)?;
        ensure!(
            lease.beneficiary == who,
            Error::<T>::ExpectedBeneficiaryOrigin
        );

        // Ensure the lease has an end block and we are past it
        let end_block = lease.end_block.ok_or(Error::<T>::LeaseHasNoEndBlock)?;
        ensure!(now >= end_block, Error::<T>::LeaseHasNotEnded);

        // Transfer ownership to the beneficiary
        ensure!(
            Self::coldkey_owns_hotkey(&lease.beneficiary, &hotkey),
            Error::<T>::BeneficiaryDoesNotOwnHotkey
        );
        // A lease whose deferred dividends could not all be paid keeps its record (see
        // below) and may be terminated again to retry; the one-time hand-over steps run
        // only the first time, which is when the subnet is not yet owned by the beneficiary.
        let first_termination = SubnetOwner::<T>::get(lease.netuid) != lease.beneficiary;
        SubnetOwner::<T>::insert(lease.netuid, lease.beneficiary.clone());
        Self::set_subnet_owner_hotkey(lease.netuid, &hotkey)?;

        // Settle deferred contributor dividends before the lease state goes away. Each
        // payment runs in its own storage layer; a debt that still cannot be transferred
        // (below the minimum transfer, or blocked by a lock) keeps its row so the amount is
        // never silently dropped. The alpha stays in the lease position.
        let settled = Self::settle_unpaid_lease_dividends(lease_id, &lease);

        // Remove the contributors and accumulated dividends from storage
        let clear_result =
            SubnetLeaseShares::<T>::clear_prefix(lease_id, T::MaxContributors::get(), None);
        AccumulatedLeaseDividends::<T>::remove(lease_id);

        if first_termination {
            // Stop tracking the lease coldkey and hotkey
            let _ = frame_system::Pallet::<T>::dec_providers(&lease.coldkey).defensive();
            let _ = frame_system::Pallet::<T>::dec_providers(&lease.hotkey).defensive();

            // Remove the beneficiary proxy
            T::ProxyInterface::remove_lease_beneficiary_proxy(&lease.coldkey, &lease.beneficiary)?;
        }

        // The lease record and the subnet mapping stay while any deferred dividend is
        // still owed, so the debt remains claimable: the owner-cut hook retries it every
        // epoch and removes the record once everything is paid. An ended lease no longer
        // distributes anything, so keeping the record has no other effect.
        Self::finish_lease_cleanup_if_settled(lease_id, lease.netuid);

        Self::deposit_event(Event::SubnetLeaseTerminated {
            beneficiary: lease.beneficiary,
            netuid: lease.netuid,
        });

        // Lease shares exclude the beneficiary, while the benchmark's `k` includes them.
        let contributors_count = clear_result
            .unique
            .saturating_add(1)
            .min(T::MaxContributors::get());
        let settlement =
            <T as Config>::WeightInfo::transfer_stake().saturating_mul(u64::from(settled));
        Ok(Some(
            <T as Config>::WeightInfo::terminate_lease(contributors_count)
                .saturating_add(settlement),
        )
        .into())
    }

    /// Pre-dispatch weight of `terminate_lease`: the benchmarked clear for the most
    /// contributors plus one stake transfer per possible deferred dividend. Refunded to the
    /// contributors cleared and the debts actually settled.
    pub fn terminate_lease_declared_weight() -> Weight {
        <T as Config>::WeightInfo::terminate_lease(T::MaxContributors::get()).saturating_add(
            <T as Config>::WeightInfo::transfer_stake()
                .saturating_mul(u64::from(T::MaxContributors::get())),
        )
    }

    /// Remove the lease record and subnet mapping once no deferred dividend is owed.
    fn finish_lease_cleanup_if_settled(lease_id: LeaseId, netuid: NetUid) {
        if SubnetLeaseUnpaidDividends::<T>::iter_prefix(lease_id)
            .next()
            .is_some()
        {
            return;
        }
        SubnetLeases::<T>::remove(lease_id);
        SubnetUidToLeaseId::<T>::remove(netuid);
    }

    /// Pay every deferred contributor dividend of `lease_id` that can be transferred now.
    /// Returns the number of debts attempted; settled rows are removed, the rest stay.
    fn settle_unpaid_lease_dividends(lease_id: LeaseId, lease: &SubnetLeaseOf<T>) -> u32 {
        let mut attempted: u32 = 0;
        let unpaid: Vec<(T::AccountId, AlphaBalance)> =
            SubnetLeaseUnpaidDividends::<T>::iter_prefix(lease_id).collect();
        for (contributor, owed) in unpaid {
            attempted = attempted.saturating_add(1);
            if owed.is_zero() {
                SubnetLeaseUnpaidDividends::<T>::remove(lease_id, &contributor);
                continue;
            }
            let paid = frame_support::storage::with_storage_layer(|| {
                ensure!(
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &lease.hotkey,
                        &lease.coldkey,
                        lease.netuid,
                    ) >= owed,
                    Error::<T>::NotEnoughStakeToWithdraw
                );
                Self::transfer_stake_within_subnet(
                    &lease.coldkey,
                    &lease.hotkey,
                    &contributor,
                    &lease.hotkey,
                    lease.netuid,
                    owed,
                )
                .map(|_| ())
            });
            match paid {
                Ok(()) => {
                    SubnetLeaseUnpaidDividends::<T>::remove(lease_id, &contributor);
                    Self::deposit_event(Event::SubnetLeaseDividendsDistributed {
                        lease_id,
                        contributor,
                        alpha: owed,
                    });
                }
                Err(err) => {
                    log::debug!(
                        "Deferred lease {lease_id} dividend still unpayable at termination: {err:?}"
                    );
                    Self::deposit_event(Event::SubnetLeaseDividendSkipped {
                        lease_id,
                        contributor,
                        alpha: owed,
                    });
                }
            }
        }
        attempted
    }

    /// Hook used when the subnet owner's cut is distributed to split the amount into dividends
    /// for the contributors and the beneficiary in shares relative to their initial contributions.
    /// It accumulates dividends to be distributed later when the interval for distribution is reached.
    /// Distribution is made in alpha and stake to the contributor coldkey and lease hotkey.
    pub fn distribute_leased_network_dividends(lease_id: LeaseId, owner_cut_alpha: AlphaBalance) {
        // Ensure the lease exists
        let Some(lease) = SubnetLeases::<T>::get(lease_id) else {
            log::debug!("Lease {lease_id} doesn't exists so we can't distribute dividends");
            return;
        };

        // An ended lease distributes nothing more. If it has been terminated with
        // deferred dividends still owed, retry those payments here (the record only
        // survives termination for this purpose) and drop the record once all are paid.
        let now = frame_system::Pallet::<T>::block_number();
        if lease.end_block.is_some_and(|end_block| end_block <= now) {
            if SubnetOwner::<T>::get(lease.netuid) == lease.beneficiary {
                Self::settle_unpaid_lease_dividends(lease_id, &lease);
                Self::finish_lease_cleanup_if_settled(lease_id, lease.netuid);
            }
            return;
        }

        // Get the actual amount of alpha to distribute from the owner's cut,
        // we voluntarily round up to favor the contributors
        let current_contributors_cut_alpha =
            lease.emissions_share.mul_ceil(owner_cut_alpha.to_u64());

        // Get the total amount of alpha to distribute from the contributors
        // including the dividends accumulated so far
        let total_contributors_cut_alpha = AccumulatedLeaseDividends::<T>::get(lease_id)
            .saturating_add(current_contributors_cut_alpha.into());

        // Ensure the distribution interval is not zero
        let rem = now
            .into()
            .checked_rem(T::LeaseDividendsDistributionInterval::get().into());
        if rem.is_none() {
            // This should never happen but we check it anyway
            log::error!("LeaseDividendsDistributionInterval must be greater than 0");
            AccumulatedLeaseDividends::<T>::set(lease_id, total_contributors_cut_alpha);
            return;
        } else if rem.is_some_and(|rem| rem > 0u32.into()) {
            // This is not the time to distribute dividends, so we accumulate the dividends
            AccumulatedLeaseDividends::<T>::set(lease_id, total_contributors_cut_alpha);
            return;
        }

        // Contributor slices are floored (the beneficiary takes the remainder, so nothing
        // is lost) and each is transferred in its own storage layer. A slice that cannot be
        // paid — too small for the minimum transfer, or blocked by a lock on either side —
        // is recorded against that contributor alone and retried with their next slice; it
        // never re-enters the shared pot, so no other contributor or the beneficiary can be
        // paid from it. Only the beneficiary's remainder is fatal for the whole distribution.
        if let Err(err) = frame_support::storage::with_storage_layer(|| {
            let mut alpha_distributed = AlphaBalance::ZERO;

            for (contributor, share) in SubnetLeaseShares::<T>::iter_prefix(lease_id) {
                let slice: AlphaBalance = share
                    .saturating_mul(U64F64::from(total_contributors_cut_alpha.to_u64()))
                    .floor()
                    .saturating_to_num::<u64>()
                    .into();
                // This interval's slice is allocated to the contributor whether or not the
                // transfer succeeds: it is either paid now or recorded as unpaid.
                alpha_distributed = alpha_distributed.saturating_add(slice);
                let owed = slice
                    .saturating_add(SubnetLeaseUnpaidDividends::<T>::get(lease_id, &contributor));
                if owed.is_zero() {
                    continue;
                }

                let paid = frame_support::storage::with_storage_layer(|| {
                    // The transfer helper silently skips an unfunded debit.
                    ensure!(
                        Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                            &lease.hotkey,
                            &lease.coldkey,
                            lease.netuid,
                        ) >= owed,
                        Error::<T>::NotEnoughStakeToWithdraw
                    );
                    Self::transfer_stake_within_subnet(
                        &lease.coldkey,
                        &lease.hotkey,
                        &contributor,
                        &lease.hotkey,
                        lease.netuid,
                        owed,
                    )
                    .map(|_| ())
                });

                match paid {
                    Ok(()) => {
                        SubnetLeaseUnpaidDividends::<T>::remove(lease_id, &contributor);
                        Self::deposit_event(Event::SubnetLeaseDividendsDistributed {
                            lease_id,
                            contributor,
                            alpha: owed,
                        });
                    }
                    Err(err) => {
                        log::debug!(
                            "Deferring lease {lease_id} dividend for a contributor this interval: {err:?}"
                        );
                        SubnetLeaseUnpaidDividends::<T>::insert(lease_id, &contributor, owed);
                        Self::deposit_event(Event::SubnetLeaseDividendSkipped {
                            lease_id,
                            contributor,
                            alpha: owed,
                        });
                    }
                }
            }

            // The beneficiary takes what no contributor slice claims.
            let beneficiary_cut_alpha =
                total_contributors_cut_alpha.saturating_sub(alpha_distributed);
            if !beneficiary_cut_alpha.is_zero() {
                ensure!(
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &lease.hotkey,
                        &lease.coldkey,
                        lease.netuid,
                    ) >= beneficiary_cut_alpha,
                    Error::<T>::NotEnoughStakeToWithdraw
                );
                Self::transfer_stake_within_subnet(
                    &lease.coldkey,
                    &lease.hotkey,
                    &lease.beneficiary,
                    &lease.hotkey,
                    lease.netuid,
                    beneficiary_cut_alpha,
                )?;
                Self::deposit_event(Event::SubnetLeaseDividendsDistributed {
                    lease_id,
                    contributor: lease.beneficiary.clone(),
                    alpha: beneficiary_cut_alpha,
                });
            }

            // Every unit of the pot is now paid, owed to the beneficiary, or recorded
            // against the contributor it belongs to.
            AccumulatedLeaseDividends::<T>::insert(lease_id, AlphaBalance::ZERO);

            Ok::<(), DispatchError>(())
        }) {
            log::debug!("Couldn't distributing dividends for lease {lease_id}: {err:?}");
            AccumulatedLeaseDividends::<T>::set(lease_id, total_contributors_cut_alpha);
        };
    }

    /// The part of a leased subnet's owner cut that stays with the lease after the
    /// contributors' share is carved out. Only this part may be auto-locked: the contributors'
    /// share is paid out from the lease position and must remain transferable, so locking it
    /// would block every dividend distribution. An ended or missing lease keeps the whole cut.
    pub fn leased_owner_cut_retained(
        lease_id: LeaseId,
        owner_cut_alpha: AlphaBalance,
    ) -> AlphaBalance {
        let Some(lease) = SubnetLeases::<T>::get(lease_id) else {
            return owner_cut_alpha;
        };
        let now = frame_system::Pallet::<T>::block_number();
        if lease.end_block.is_some_and(|end_block| end_block <= now) {
            return owner_cut_alpha;
        }
        let contributors_cut: AlphaBalance = lease
            .emissions_share
            .mul_ceil(owner_cut_alpha.to_u64())
            .into();
        owner_cut_alpha.saturating_sub(contributors_cut)
    }

    fn lease_coldkey(lease_id: LeaseId) -> Result<T::AccountId, DispatchError> {
        let entropy = ("leasing/coldkey", lease_id).using_encoded(blake2_256);
        T::AccountId::decode(&mut TrailingZeroInput::new(entropy.as_ref()))
            .map_err(|_| Error::<T>::InvalidValue.into())
    }

    fn lease_hotkey(lease_id: LeaseId) -> Result<T::AccountId, DispatchError> {
        let entropy = ("leasing/hotkey", lease_id).using_encoded(blake2_256);
        T::AccountId::decode(&mut TrailingZeroInput::new(entropy.as_ref()))
            .map_err(|_| Error::<T>::InvalidValue.into())
    }

    fn get_next_lease_id() -> Result<LeaseId, Error<T>> {
        let lease_id = NextSubnetLeaseId::<T>::get();

        // Increment the lease id
        let next_lease_id = lease_id.checked_add(1).ok_or(Error::<T>::Overflow)?;
        NextSubnetLeaseId::<T>::put(next_lease_id);

        Ok(lease_id)
    }

    fn find_lease_netuid(lease_coldkey: &T::AccountId) -> Option<NetUid> {
        SubnetOwner::<T>::iter()
            .find(|(_, coldkey)| coldkey == lease_coldkey)
            .map(|(netuid, _)| netuid)
    }

    // Get the crowdloan being finalized from the crowdloan pallet when the call is executed,
    // and the current crowdloan ID is exposed to us.
    fn get_crowdloan_being_finalized() -> Result<
        (
            pallet_crowdloan::CrowdloanId,
            pallet_crowdloan::CrowdloanInfoOf<T>,
        ),
        pallet_crowdloan::Error<T>,
    > {
        let crowdloan_id = pallet_crowdloan::CurrentCrowdloanId::<T>::get()
            .ok_or(pallet_crowdloan::Error::<T>::InvalidCrowdloanId)?;
        let crowdloan = pallet_crowdloan::Crowdloans::<T>::get(crowdloan_id)
            .ok_or(pallet_crowdloan::Error::<T>::InvalidCrowdloanId)?;
        Ok((crowdloan_id, crowdloan))
    }
}
