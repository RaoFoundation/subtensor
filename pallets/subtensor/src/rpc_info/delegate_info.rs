use super::*;
use frame_support::IterableStorageMap;
use frame_support::pallet_prelude::{Decode, Encode};
use safe_math::*;
use substrate_fixed::types::U64F64;
extern crate alloc;
use alloc::collections::BTreeMap;
use codec::Compact;
use subtensor_runtime_common::{AlphaBalance, NetUid};

#[freeze_struct("1fafc4fcf28cba7a")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct DelegateInfo<AccountId: TypeInfo + Encode + Decode> {
    pub delegate_ss58: AccountId,
    pub take: Compact<u16>,
    pub nominators: Vec<(AccountId, Vec<(Compact<NetUid>, Compact<u64>)>)>, // map of nominator_ss58 to netuid and stake amount
    pub owner_ss58: AccountId,
    pub registrations: Vec<Compact<NetUid>>, // Vec of netuid this delegate is registered on
    pub validator_permits: Vec<Compact<NetUid>>, // Vec of netuid this delegate has validator permit on
    pub return_per_1000: Compact<u64>, // Delegators current daily return per 1000 TAO staked minus take fee
    pub total_daily_return: Compact<u64>, // Delegators current daily return
}

impl<T: Config> Pallet<T> {
    fn return_per_1000_tao(
        take: Compact<u16>,
        total_stake: U64F64,
        emissions_per_day: U64F64,
    ) -> U64F64 {
        // Get the take as a percentage and subtract it from 1 for remainder.
        let without_take: U64F64 = U64F64::saturating_from_num(1)
            .saturating_sub(U64F64::saturating_from_num(take.0).safe_div(u16::MAX.into()));

        if total_stake > U64F64::saturating_from_num(0) {
            emissions_per_day
                .saturating_mul(without_take)
                // Divide by 1000 TAO for return per 1k
                .safe_div(total_stake.safe_div(U64F64::saturating_from_num(1000.0 * 1e9)))
        } else {
            U64F64::saturating_from_num(0)
        }
    }

    #[cfg(test)]
    pub fn return_per_1000_tao_test(
        take: Compact<u16>,
        total_stake: U64F64,
        emissions_per_day: U64F64,
    ) -> U64F64 {
        Self::return_per_1000_tao(take, total_stake, emissions_per_day)
    }

    fn get_delegate_by_existing_account(
        delegate: AccountIdOf<T>,
        skip_nominators: bool,
    ) -> DelegateInfo<T::AccountId> {
        let mut nominators = Vec::<(T::AccountId, Vec<(Compact<NetUid>, Compact<u64>)>)>::new();
        let mut nominator_map =
            BTreeMap::<T::AccountId, Vec<(Compact<NetUid>, Compact<u64>)>>::new();

        if !skip_nominators {
            let mut alpha_share_pools = vec![];
            for netuid in Self::get_all_subnet_netuids() {
                let alpha_share_pool = Self::get_alpha_share_pool(delegate.clone(), netuid);
                alpha_share_pools.push(alpha_share_pool);
            }

            for (nominator, netuid, alpha_stake) in Self::alpha_iter_single_prefix(&delegate) {
                if alpha_stake.is_zero() {
                    continue;
                }

                if let Some(alpha_share_pool) = alpha_share_pools.get(u16::from(netuid) as usize) {
                    let coldkey_stake = alpha_share_pool.get_value_from_shares(alpha_stake);

                    nominator_map
                        .entry(nominator.clone())
                        .or_insert(Vec::new())
                        .push((netuid.into(), coldkey_stake.into()));
                }
            }

            for (nominator, stakes) in nominator_map {
                nominators.push((nominator, stakes));
            }
        }

        let registrations = Self::get_registered_networks_for_hotkey(&delegate.clone());
        let mut validator_permits = Vec::<Compact<NetUid>>::new();
        let mut emissions_per_day: U64F64 = U64F64::saturating_from_num(0);

        for netuid in registrations.iter() {
            if let Ok(uid) = Self::get_uid_for_net_and_hotkey(*netuid, &delegate.clone()) {
                let validator_permit = Self::get_validator_permit_for_uid(*netuid, uid);
                if validator_permit {
                    validator_permits.push((*netuid).into());
                }

                let emission: U64F64 = u64::from(Self::get_emission_for_uid(*netuid, uid)).into();
                let tempo: U64F64 = Self::get_tempo(*netuid).into();
                if tempo > U64F64::saturating_from_num(0) {
                    let epochs_per_day: U64F64 = U64F64::saturating_from_num(7200).safe_div(tempo);
                    emissions_per_day =
                        emissions_per_day.saturating_add(emission.saturating_mul(epochs_per_day));
                }
            }
        }

        let owner = Self::get_owning_coldkey_for_hotkey(&delegate.clone());
        let take: Compact<u16> = <Delegates<T>>::get(delegate.clone()).into();

        let total_stake: U64F64 = u64::from(Self::get_stake_for_hotkey_on_subnet(
            &delegate.clone(),
            NetUid::ROOT,
        ))
        .into();

        let return_per_1000: U64F64 =
            Self::return_per_1000_tao(take, total_stake, emissions_per_day);

        DelegateInfo {
            delegate_ss58: delegate.clone(),
            take,
            nominators,
            owner_ss58: owner.clone(),
            registrations: registrations.iter().map(|x| x.into()).collect(),
            validator_permits,
            return_per_1000: return_per_1000.saturating_to_num::<u64>().into(),
            total_daily_return: emissions_per_day.saturating_to_num::<u64>().into(),
        }
    }

    pub fn get_delegate(delegate: T::AccountId) -> Option<DelegateInfo<T::AccountId>> {
        // Check delegate exists
        if !<Delegates<T>>::contains_key(delegate.clone()) {
            return None;
        }

        let delegate_info = Self::get_delegate_by_existing_account(delegate.clone(), false);
        Some(delegate_info)
    }

    /// get all delegates info from storage
    ///
    pub fn get_delegates() -> Vec<DelegateInfo<T::AccountId>> {
        let mut delegates = Vec::<DelegateInfo<T::AccountId>>::new();
        for delegate in <Delegates<T> as IterableStorageMap<T::AccountId, u16>>::iter_keys() {
            let delegate_info = Self::get_delegate_by_existing_account(delegate.clone(), false);
            delegates.push(delegate_info);
        }

        delegates
    }

    /// get all delegate info and staked token amount for a given delegatee account
    ///
    pub fn get_delegated(
        delegatee: T::AccountId,
    ) -> Vec<(
        DelegateInfo<T::AccountId>,
        (Compact<NetUid>, Compact<AlphaBalance>),
    )> {
        let mut delegates: Vec<(
            DelegateInfo<T::AccountId>,
            (Compact<NetUid>, Compact<AlphaBalance>),
        )> = Vec::new();
        for delegate in <Delegates<T> as IterableStorageMap<T::AccountId, u16>>::iter_keys() {
            // Staked to this delegate, so add to list
            for (netuid, _) in Self::alpha_iter_prefix((&delegate, &delegatee)) {
                let delegate_info = Self::get_delegate_by_existing_account(delegate.clone(), true);
                delegates.push((
                    delegate_info,
                    (
                        netuid.into(),
                        Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                            &delegate, &delegatee, netuid,
                        )
                        .into(),
                    ),
                ));
            }
        }

        delegates
    }

    // Helper function to get the coldkey associated with a hotkey
    pub fn get_coldkey_for_hotkey(hotkey: &T::AccountId) -> T::AccountId {
        Owner::<T>::get(hotkey)
    }

    pub fn maybe_coldkey_for_hotkey(hotkey: &T::AccountId) -> Option<T::AccountId> {
        Owner::<T>::try_get(hotkey).ok()
    }
}
