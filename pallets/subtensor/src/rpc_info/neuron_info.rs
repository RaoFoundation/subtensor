//! Per-uid neuron RPC views (`NeuronInfo` / `NeuronInfoLite`).

use super::*;
use frame_support::pallet_prelude::{Decode, Encode};
extern crate alloc;
use codec::Compact;
use sp_runtime::PerU16;
use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex};

/// Full per-uid neuron view including sparse weights and bonds matrices.
///
/// `rank`, `trust`, and `pruning_score` are legacy fields (zeros / max); they are no longer
/// computed on-chain. `stake` currently carries a single owner-coldkey total, not a full
/// coldkey→stake map.
#[freeze_struct("23b656b0f34441f5")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct NeuronInfo<AccountId: TypeInfo + Encode + Decode> {
    hotkey: AccountId,
    coldkey: AccountId,
    uid: Compact<u16>,
    netuid: Compact<NetUid>,
    active: bool,
    axon_info: AxonInfo,
    prometheus_info: PrometheusInfo,
    /// Owner coldkey paired with total hotkey alpha on this subnet (not a full nominator map).
    stake: Vec<(AccountId, Compact<AlphaBalance>)>,
    /// Deprecated: always 0.
    rank: Compact<u16>,
    emission: Compact<AlphaBalance>,
    incentive: Compact<PerU16>,
    consensus: Compact<PerU16>,
    /// Deprecated: always 0.
    trust: Compact<PerU16>,
    validator_trust: Compact<PerU16>,
    dividends: Compact<PerU16>,
    last_update: Compact<u64>,
    validator_permit: bool,
    /// Sparse `(target_uid, weight)` pairs with weight > 0.
    weights: Vec<(Compact<u16>, Compact<u16>)>,
    /// Sparse `(target_uid, bond)` pairs with bond > 0.
    bonds: Vec<(Compact<u16>, Compact<u16>)>,
    /// Deprecated: always `u16::MAX`.
    pruning_score: Compact<u16>,
}

/// [`NeuronInfo`] without weights/bonds — cheaper for list endpoints.
#[freeze_struct("8bd1725e22406377")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct NeuronInfoLite<AccountId: TypeInfo + Encode + Decode> {
    hotkey: AccountId,
    coldkey: AccountId,
    uid: Compact<u16>,
    netuid: Compact<NetUid>,
    active: bool,
    axon_info: AxonInfo,
    prometheus_info: PrometheusInfo,
    /// Owner coldkey paired with total hotkey alpha on this subnet (not a full nominator map).
    stake: Vec<(AccountId, Compact<AlphaBalance>)>,
    /// Deprecated: always 0.
    rank: Compact<u16>,
    emission: Compact<AlphaBalance>,
    incentive: Compact<PerU16>,
    consensus: Compact<PerU16>,
    /// Deprecated: always 0.
    trust: Compact<PerU16>,
    validator_trust: Compact<PerU16>,
    dividends: Compact<PerU16>,
    last_update: Compact<u64>,
    validator_permit: bool,
    /// Deprecated: always `u16::MAX`.
    pruning_score: Compact<u16>,
}

impl<T: Config> Pallet<T> {
    /// All neurons on `netuid`, or empty if the subnet does not exist.
    pub fn get_neurons(netuid: NetUid) -> Vec<NeuronInfo<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return Vec::new();
        }

        let mut neurons = Vec::new();
        let n = Self::get_subnetwork_n(netuid);
        for uid in 0..n {
            let neuron = match Self::neuron_info_for_existing_subnet_uid(netuid, uid) {
                Some(n) => n,
                None => break, // No more neurons
            };

            neurons.push(neuron);
        }
        neurons
    }

    /// Full [`NeuronInfo`] for `uid` on an existing subnet; `None` if the uid has no hotkey.
    fn neuron_info_for_existing_subnet_uid(
        netuid: NetUid,
        uid: u16,
    ) -> Option<NeuronInfo<T::AccountId>> {
        let hotkey = match Self::get_hotkey_for_net_and_uid(netuid, uid) {
            Ok(h) => h,
            Err(_) => return None,
        };

        let axon_info = Self::get_axon_info(netuid, &hotkey.clone());

        let prometheus_info = Self::get_prometheus_info(netuid, &hotkey.clone());

        let coldkey = Owner::<T>::get(hotkey.clone()).clone();

        let active = Self::get_active_for_uid(netuid, uid);
        let emission = Self::get_emission_for_uid(netuid, uid);
        let incentive = Self::get_incentive_for_uid(netuid.into(), uid);
        let consensus = Self::get_consensus_for_uid(netuid, uid);
        let validator_trust = Self::get_validator_trust_for_uid(netuid, uid);
        let dividends = Self::get_dividends_for_uid(netuid, uid);
        let last_update = Self::get_last_update_for_uid(NetUidStorageIndex::from(netuid), uid);
        let validator_permit = Self::get_validator_permit_for_uid(netuid, uid);

        let weights = Weights::<T>::get(NetUidStorageIndex::from(netuid), uid)
            .into_iter()
            .filter_map(|(i, w)| {
                if w > 0 {
                    Some((i.into(), w.into()))
                } else {
                    None
                }
            })
            .collect::<Vec<(Compact<u16>, Compact<u16>)>>();

        let bonds = Bonds::<T>::get(NetUidStorageIndex::from(netuid), uid)
            .iter()
            .filter_map(|(i, b)| {
                if *b > 0 {
                    Some((i.into(), b.into()))
                } else {
                    None
                }
            })
            .collect::<Vec<(Compact<u16>, Compact<u16>)>>();
        let stake: Vec<(T::AccountId, Compact<AlphaBalance>)> = vec![(
            coldkey.clone(),
            Self::get_stake_for_hotkey_on_subnet(&hotkey, netuid).into(),
        )];
        let neuron = NeuronInfo {
            hotkey: hotkey.clone(),
            coldkey: coldkey.clone(),
            uid: uid.into(),
            netuid: netuid.into(),
            active,
            axon_info,
            prometheus_info,
            stake,
            rank: 0.into(), // Deprecated: no longer computed
            emission: emission.into(),
            incentive: PerU16::from_parts(incentive).into(),
            consensus: PerU16::from_parts(consensus).into(),
            trust: PerU16::zero().into(), // Deprecated: no longer computed
            validator_trust: PerU16::from_parts(validator_trust).into(),
            dividends: PerU16::from_parts(dividends).into(),
            last_update: last_update.into(),
            validator_permit,
            weights,
            bonds,
            pruning_score: u16::MAX.into(), // Deprecated: no longer computed
        };

        Some(neuron)
    }

    /// One full neuron, or `None` if the subnet or uid is missing.
    pub fn get_neuron(netuid: NetUid, uid: u16) -> Option<NeuronInfo<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        Self::neuron_info_for_existing_subnet_uid(netuid, uid)
    }

    /// Lite neuron for `uid` on an existing subnet; `None` if the uid has no hotkey.
    fn neuron_info_lite_for_existing_subnet_uid(
        netuid: NetUid,
        uid: u16,
    ) -> Option<NeuronInfoLite<T::AccountId>> {
        let hotkey = match Self::get_hotkey_for_net_and_uid(netuid, uid) {
            Ok(h) => h,
            Err(_) => return None,
        };

        let axon_info = Self::get_axon_info(netuid, &hotkey.clone());

        let prometheus_info = Self::get_prometheus_info(netuid, &hotkey.clone());

        let coldkey = Owner::<T>::get(hotkey.clone()).clone();

        let active = Self::get_active_for_uid(netuid, uid);
        let emission = Self::get_emission_for_uid(netuid, uid);
        let incentive = Self::get_incentive_for_uid(netuid.into(), uid);
        let consensus = Self::get_consensus_for_uid(netuid, uid);
        let validator_trust = Self::get_validator_trust_for_uid(netuid, uid);
        let dividends = Self::get_dividends_for_uid(netuid, uid);
        let last_update = Self::get_last_update_for_uid(NetUidStorageIndex::from(netuid), uid);
        let validator_permit = Self::get_validator_permit_for_uid(netuid, uid);

        let stake: Vec<(T::AccountId, Compact<AlphaBalance>)> = vec![(
            coldkey.clone(),
            Self::get_stake_for_hotkey_on_subnet(&hotkey, netuid).into(),
        )];

        let neuron = NeuronInfoLite {
            hotkey: hotkey.clone(),
            coldkey: coldkey.clone(),
            uid: uid.into(),
            netuid: netuid.into(),
            active,
            axon_info,
            prometheus_info,
            stake,
            rank: 0.into(), // Deprecated: no longer computed
            emission: emission.into(),
            incentive: PerU16::from_parts(incentive).into(),
            consensus: PerU16::from_parts(consensus).into(),
            trust: PerU16::zero().into(), // Deprecated: no longer computed
            validator_trust: PerU16::from_parts(validator_trust).into(),
            dividends: PerU16::from_parts(dividends).into(),
            last_update: last_update.into(),
            validator_permit,
            pruning_score: u16::MAX.into(), // Deprecated: no longer computed
        };

        Some(neuron)
    }

    /// Lite neurons for every uid on `netuid`, or empty if the subnet does not exist.
    pub fn get_neurons_lite(netuid: NetUid) -> Vec<NeuronInfoLite<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return Vec::new();
        }

        let mut neurons: Vec<NeuronInfoLite<T::AccountId>> = Vec::new();
        let n = Self::get_subnetwork_n(netuid);
        for uid in 0..n {
            let neuron = match Self::neuron_info_lite_for_existing_subnet_uid(netuid, uid) {
                Some(n) => n,
                None => break, // No more neurons
            };

            neurons.push(neuron);
        }
        neurons
    }

    /// One lite neuron, or `None` if the subnet or uid is missing.
    pub fn get_neuron_lite(netuid: NetUid, uid: u16) -> Option<NeuronInfoLite<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        Self::neuron_info_lite_for_existing_subnet_uid(netuid, uid)
    }
}
