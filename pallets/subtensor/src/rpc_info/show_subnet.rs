use super::*;
extern crate alloc;
use crate::epoch::math::*;
use codec::Compact;
use frame_support::pallet_prelude::{Decode, Encode};
use substrate_fixed::types::I64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance};

#[freeze_struct("5214275026dc3f36")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SubnetState<AccountId: TypeInfo + Encode + Decode> {
    netuid: Compact<NetUid>,
    hotkeys: Vec<AccountId>,
    coldkeys: Vec<AccountId>,
    active: Vec<bool>,
    validator_permit: Vec<bool>,
    pruning_score: Vec<Compact<u16>>,
    last_update: Vec<Compact<u64>>,
    emission: Vec<Compact<AlphaBalance>>,
    dividends: Vec<Compact<u16>>,
    incentives: Vec<Compact<u16>>,
    consensus: Vec<Compact<u16>>,
    trust: Vec<Compact<u16>>,
    rank: Vec<Compact<u16>>,
    block_at_registration: Vec<Compact<u64>>,
    alpha_stake: Vec<Compact<AlphaBalance>>,
    tao_stake: Vec<Compact<TaoBalance>>,
    total_stake: Vec<Compact<TaoBalance>>,
    emission_history: Vec<Vec<Compact<AlphaBalance>>>,
    // identities: Vec<ChainIdentityOf>,
    // tao_stake: Compact<u64>,
    // incentive: Compact<u16>,
    // consensus: Compact<u16>,
    // trust: Compact<u16>,
    // validator_trust: Compact<u16>,
    // dividends: Compact<u16>,
    // // has no weights or bonds
}

impl<T: Config> Pallet<T> {
    /// Retrieves the emission history for a list of hotkeys across all subnets.
    ///
    /// This function iterates over all subnets and collects the last emission value
    /// for each hotkey in the provided list. The result is a vector of vectors, where
    /// each inner vector contains the emission values for a specific subnet.
    ///
    /// # Arguments
    ///
    /// * `hotkeys` - A vector of hotkeys (account IDs) for which the emission history is to be retrieved.
    ///
    /// # Returns
    ///
    /// * `Vec<Vec<Compact<u64>>>` - A vector of vectors containing the emission history for each hotkey across all subnets.
    pub fn get_emissions_history(hotkeys: Vec<T::AccountId>) -> Vec<Vec<Compact<AlphaBalance>>> {
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

    /// Retrieves the state of a specific subnet.
    ///
    /// This function gathers various metrics and data points for a given subnet, identified by its `netuid`.
    /// It collects information such as hotkeys, coldkeys, block at registration, active status, validator permits,
    /// pruning scores, last updates, emissions, dividends, incentives, consensus, trust, rank, local stake, global stake,
    /// stake weight, and emission history.
    ///
    /// # Arguments
    ///
    /// * `netuid` - The unique identifier of the subnet for which the state is to be retrieved.
    ///
    /// # Returns
    ///
    /// * `Option<SubnetState<T::AccountId>>` - An optional `SubnetState` struct containing the collected data for the subnet.
    ///   Returns `None` if the subnet does not exist.
    pub fn get_subnet_state(netuid: NetUid) -> Option<SubnetState<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }
        let n: u16 = Self::get_subnetwork_n(netuid);
        let mut hotkeys: Vec<T::AccountId> = vec![];
        let mut coldkeys: Vec<T::AccountId> = vec![];
        let mut block_at_registration: Vec<Compact<u64>> = vec![];
        // let mut identities: Vec<ChainIdentityOf> = vec![];
        for uid in 0..n {
            let hotkey = Keys::<T>::get(netuid, uid);
            let coldkey = Owner::<T>::get(hotkey.clone());
            hotkeys.push(hotkey);
            coldkeys.push(coldkey);
            block_at_registration.push(BlockAtRegistration::<T>::get(netuid, uid).into());
            // identities.push( Identities::<T>::get( coldkey.clone() ) );
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
        let dividends: Vec<Compact<u16>> = Dividends::<T>::get(netuid)
            .into_iter()
            .map(Compact::from)
            .collect();
        let incentives: Vec<Compact<u16>> = Incentive::<T>::get(NetUidStorageIndex::from(netuid))
            .into_iter()
            .map(Compact::from)
            .collect();
        let consensus: Vec<Compact<u16>> = Consensus::<T>::get(netuid)
            .into_iter()
            .map(Compact::from)
            .collect();
        let trust: Vec<Compact<u16>> = Vec::new(); // Deprecated: no longer computed
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
        let emission_history = Self::get_emissions_history(hotkeys.clone());
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
