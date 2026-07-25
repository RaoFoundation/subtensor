//! Compact per-uid subnet state RPC view (`SubnetState`).

use super::*;
extern crate alloc;
use crate::epoch::math::*;
use codec::Compact;
use frame_support::pallet_prelude::{Decode, Encode};
use sp_runtime::PerU16;
use substrate_fixed::types::I64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};

/// Per-uid vectors for a subnet: keys, consensus scores, stakes, and emission history.
///
/// `pruning_score`, `trust`, and `rank` are deprecated empty vectors. `emission_history` is
/// last emission per hotkey across all subnets (outer index = subnet order from
/// [`Pallet::get_all_subnet_netuids`]).
#[freeze_struct("8a7c09d1eba9df6a")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SubnetState<AccountId: TypeInfo + Encode + Decode> {
    netuid: Compact<NetUid>,
    hotkeys: Vec<AccountId>,
    coldkeys: Vec<AccountId>,
    active: Vec<bool>,
    validator_permit: Vec<bool>,
    /// Deprecated: always empty.
    pruning_score: Vec<Compact<u16>>,
    last_update: Vec<Compact<u64>>,
    emission: Vec<Compact<AlphaBalance>>,
    dividends: Vec<Compact<PerU16>>,
    incentives: Vec<Compact<PerU16>>,
    consensus: Vec<Compact<PerU16>>,
    /// Deprecated: always empty.
    trust: Vec<Compact<PerU16>>,
    /// Deprecated: always empty.
    rank: Vec<Compact<u16>>,
    block_at_registration: Vec<Compact<u64>>,
    alpha_stake: Vec<Compact<AlphaBalance>>,
    tao_stake: Vec<Compact<TaoBalance>>,
    total_stake: Vec<Compact<TaoBalance>>,
    emission_history: Vec<Vec<Compact<AlphaBalance>>>,
}

impl<T: Config> Pallet<T> {
    /// Last hotkey emission on each subnet for the given hotkeys.
    ///
    /// Outer vector follows [`Self::get_all_subnet_netuids`]; each inner vector aligns with
    /// `hotkeys`.
    fn last_emission_history_across_subnets(
        hotkeys: Vec<T::AccountId>,
    ) -> Vec<Vec<Compact<AlphaBalance>>> {
        let mut result: Vec<Vec<Compact<AlphaBalance>>> = vec![];
        for netuid in Self::get_all_subnet_netuids() {
            let mut hotkeys_emissions: Vec<Compact<AlphaBalance>> = vec![];
            for hotkey in hotkeys.clone() {
                let last_emission: Compact<AlphaBalance> =
                    LastHotkeyEmissionOnNetuid::<T>::get(hotkey.clone(), netuid).into();
                hotkeys_emissions.push(last_emission);
            }
            result.push(hotkeys_emissions.clone());
        }
        result
    }

    /// [`SubnetState`] for `netuid`, or `None` if the subnet does not exist.
    pub fn get_subnet_state(netuid: NetUid) -> Option<SubnetState<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }
        let n: u16 = Self::get_subnetwork_n(netuid);
        let mut hotkeys: Vec<T::AccountId> = vec![];
        let mut coldkeys: Vec<T::AccountId> = vec![];
        let mut block_at_registration: Vec<Compact<u64>> = vec![];
        for uid in 0..n {
            let hotkey = Keys::<T>::get(netuid, uid);
            let coldkey = Owner::<T>::get(hotkey.clone());
            hotkeys.push(hotkey);
            coldkeys.push(coldkey);
            block_at_registration.push(BlockAtRegistration::<T>::get(netuid, uid).into());
        }
        let active: Vec<bool> = Active::<T>::get(netuid);
        let validator_permit: Vec<bool> = ValidatorPermit::<T>::get(netuid);
        let pruning_score: Vec<Compact<u16>> = Vec::new(); // Deprecated: no longer computed
        let last_update: Vec<Compact<u64>> = LastUpdate::<T>::get(NetUidStorageIndex::from(netuid))
            .into_iter()
            .map(Compact::from)
            .collect();
        let emission = Emission::<T>::get(netuid)
            .into_iter()
            .map(Compact::from)
            .collect();
        let dividends: Vec<Compact<PerU16>> = Dividends::<T>::get(netuid)
            .into_iter()
            .map(Compact::from)
            .collect();
        let incentives: Vec<Compact<PerU16>> =
            Incentive::<T>::get(NetUidStorageIndex::from(netuid))
                .into_iter()
                .map(Compact::from)
                .collect();
        let consensus: Vec<Compact<PerU16>> = Consensus::<T>::get(netuid)
            .into_iter()
            .map(Compact::from)
            .collect();
        let trust: Vec<Compact<PerU16>> = Vec::new(); // Deprecated: no longer computed
        let rank: Vec<Compact<u16>> = Vec::new(); // Deprecated: no longer computed
        let (total_stake_fl, alpha_stake_fl, tao_stake_fl): (
            Vec<I64F64>,
            Vec<I64F64>,
            Vec<I64F64>,
        ) = Self::get_stake_weights_for_network(netuid);
        let alpha_stake: Vec<Compact<AlphaBalance>> = alpha_stake_fl
            .iter()
            .map(|xi| Compact::from(AlphaBalance::from(fixed64_to_u64(*xi))))
            .collect();
        let tao_stake: Vec<Compact<TaoBalance>> = tao_stake_fl
            .iter()
            .map(|xi| Compact::from(TaoBalance::from(fixed64_to_u64(*xi))))
            .collect();
        let total_stake: Vec<Compact<TaoBalance>> = total_stake_fl
            .iter()
            .map(|xi| Compact::from(TaoBalance::from(fixed64_to_u64(*xi))))
            .collect();
        let emission_history = Self::last_emission_history_across_subnets(hotkeys.clone());
        Some(SubnetState {
            netuid: netuid.into(),
            hotkeys,
            coldkeys,
            active,
            validator_permit,
            pruning_score,
            last_update,
            emission,
            dividends,
            incentives,
            consensus,
            trust,
            rank,
            block_at_registration,
            alpha_stake,
            tao_stake,
            total_stake,
            emission_history,
        })
    }
}
