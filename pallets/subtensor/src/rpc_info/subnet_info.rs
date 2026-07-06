use super::*;
use frame_support::pallet_prelude::{Decode, Encode};
use frame_support::storage::IterableStorageMap;
extern crate alloc;
use codec::Compact;
use substrate_fixed::types::{I32F32, U64F64};
use subtensor_runtime_common::{NetUid, TaoBalance};

#[freeze_struct("f691073111c39620")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SubnetInfo<AccountId: TypeInfo + Encode + Decode> {
    netuid: Compact<NetUid>,
    rho: Compact<u16>,
    kappa: Compact<u16>,
    difficulty: Compact<u64>,
    immunity_period: Compact<u16>,
    max_allowed_validators: Compact<u16>,
    min_allowed_weights: Compact<u16>,
    max_weights_limit: Compact<u16>,
    scaling_law_power: Compact<u16>,
    subnetwork_n: Compact<u16>,
    max_allowed_uids: Compact<u16>,
    blocks_since_last_step: Compact<u64>,
    tempo: Compact<u16>,
    network_modality: Compact<u16>,
    network_connect: Vec<[u16; 2]>,
    emission_values: Compact<u64>,
    burn: Compact<TaoBalance>,
    owner: AccountId,
}

#[freeze_struct("e8e028bf4fbc6741")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SubnetInfov2<AccountId: TypeInfo + Encode + Decode> {
    netuid: Compact<NetUid>,
    rho: Compact<u16>,
    kappa: Compact<u16>,
    difficulty: Compact<u64>,
    immunity_period: Compact<u16>,
    max_allowed_validators: Compact<u16>,
    min_allowed_weights: Compact<u16>,
    max_weights_limit: Compact<u16>,
    scaling_law_power: Compact<u16>,
    subnetwork_n: Compact<u16>,
    max_allowed_uids: Compact<u16>,
    blocks_since_last_step: Compact<u64>,
    tempo: Compact<u16>,
    network_modality: Compact<u16>,
    network_connect: Vec<[u16; 2]>,
    emission_value: Compact<u64>,
    burn: Compact<TaoBalance>,
    owner: AccountId,
    identity: Option<SubnetIdentityV3>,
}

#[freeze_struct("5a0830a4518a7325")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SubnetHyperparams {
    rho: Compact<u16>,
    kappa: Compact<u16>,
    immunity_period: Compact<u16>,
    min_allowed_weights: Compact<u16>,
    max_weights_limit: Compact<u16>,
    tempo: Compact<u16>,
    min_difficulty: Compact<u64>,
    max_difficulty: Compact<u64>,
    weights_version: Compact<u64>,
    weights_rate_limit: Compact<u64>,
    adjustment_interval: Compact<u16>,
    activity_cutoff: Compact<u64>,
    pub registration_allowed: bool,
    target_regs_per_interval: Compact<u16>,
    min_burn: Compact<TaoBalance>,
    max_burn: Compact<TaoBalance>,
    bonds_moving_avg: Compact<u64>,
    max_regs_per_block: Compact<u16>,
    serving_rate_limit: Compact<u64>,
    max_validators: Compact<u16>,
    adjustment_alpha: Compact<u64>,
    difficulty: Compact<u64>,
    commit_reveal_period: Compact<u64>,
    commit_reveal_weights_enabled: bool,
    alpha_high: Compact<u16>,
    alpha_low: Compact<u16>,
    liquid_alpha_enabled: bool,
}

#[freeze_struct("336a6658e70b5554")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SubnetHyperparamsV2 {
    rho: Compact<u16>,
    kappa: Compact<u16>,
    immunity_period: Compact<u16>,
    min_allowed_weights: Compact<u16>,
    max_weights_limit: Compact<u16>,
    tempo: Compact<u16>,
    min_difficulty: Compact<u64>,
    max_difficulty: Compact<u64>,
    weights_version: Compact<u64>,
    weights_rate_limit: Compact<u64>,
    adjustment_interval: Compact<u16>,
    activity_cutoff: Compact<u64>,
    pub registration_allowed: bool,
    target_regs_per_interval: Compact<u16>,
    min_burn: Compact<TaoBalance>,
    max_burn: Compact<TaoBalance>,
    bonds_moving_avg: Compact<u64>,
    max_regs_per_block: Compact<u16>,
    serving_rate_limit: Compact<u64>,
    max_validators: Compact<u16>,
    adjustment_alpha: Compact<u64>,
    difficulty: Compact<u64>,
    commit_reveal_period: Compact<u64>,
    commit_reveal_weights_enabled: bool,
    alpha_high: Compact<u16>,
    alpha_low: Compact<u16>,
    liquid_alpha_enabled: bool,
    alpha_sigmoid_steepness: I32F32,
    yuma_version: Compact<u16>,
    subnet_is_active: bool,
    transfers_enabled: bool,
    bonds_reset_enabled: bool,
    user_liquidity_enabled: bool,
}

/// Tagged value for a single hyperparameter in [`SubnetHyperparamsV3`].
///
/// Clients decode by matching on the SCALE variant tag (resolved through
/// runtime metadata). Adding a new hyperparam whose value already fits an
/// existing variant is purely additive. Introducing a brand-new value type
/// requires a new variant *and* a coordinated client update, since that
/// changes the variant tag space.
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub enum HyperparamValue {
    Bool(bool),
    U16(Compact<u16>),
    U32(Compact<u32>),
    U64(Compact<u64>),
    U128(Compact<u128>),
    TaoBalance(Compact<TaoBalance>),
    I32F32(I32F32),
    U64F64(U64F64),
}

/// One named hyperparameter and its typed value.
///
/// `name` is the ASCII identifier (e.g. `b"rho"`), matching the field names
/// used in [`SubnetHyperparamsV2`]. Clients should look up params by name
/// and treat unknown names as forward-compatible additions.
#[freeze_struct("3fee3576c33e216c")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct HyperparamEntry {
    pub name: Vec<u8>,
    pub value: HyperparamValue,
}

impl From<(&str, HyperparamValue)> for HyperparamEntry {
    fn from((name, value): (&str, HyperparamValue)) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            value,
        }
    }
}

/// Dynamic, forward-compatible view of subnet hyperparameters.
///
/// Unlike [`SubnetHyperparamsV2`], the wire shape never changes when a new
/// hyperparam is added — callers receive a flat list of
/// [`HyperparamEntry`] and look up values by name. The order in which
/// entries appear is stable insertion order from the runtime getter, but
/// clients should not rely on it for semantics.
pub type SubnetHyperparamsV3 = Vec<HyperparamEntry>;

impl<T: Config> Pallet<T> {
    pub fn get_subnet_info(netuid: NetUid) -> Option<SubnetInfo<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        let rho = Self::get_rho(netuid);
        let kappa = Self::get_kappa(netuid);
        let difficulty: Compact<u64> = Self::get_difficulty_as_u64(netuid).into();
        let immunity_period = Self::get_immunity_period(netuid);
        let max_allowed_validators = Self::get_max_allowed_validators(netuid);
        let min_allowed_weights = Self::get_min_allowed_weights(netuid);
        let max_weights_limit = Self::get_max_weight_limit(netuid);
        let scaling_law_power = Self::get_scaling_law_power(netuid);
        let subnetwork_n = Self::get_subnetwork_n(netuid);
        let max_allowed_uids = Self::get_max_allowed_uids(netuid);
        let blocks_since_last_step = Self::get_blocks_since_last_step(netuid);
        let tempo = Self::get_tempo(netuid);
        let burn = Compact::from(Self::get_burn(netuid));
        // DEPRECATED
        let network_connect: Vec<[u16; 2]> = Vec::<[u16; 2]>::new();
        // DEPRECATED for ( _netuid_, con_req) in < NetworkConnect<T> as IterableStorageDoubleMap<u16, u16, u16> >::iter_prefix(netuid) {
        //     network_connect.push([_netuid_, con_req]);
        // }

        Some(SubnetInfo {
            rho: rho.into(),
            kappa: kappa.into(),
            difficulty,
            immunity_period: immunity_period.into(),
            netuid: netuid.into(),
            max_allowed_validators: max_allowed_validators.into(),
            min_allowed_weights: min_allowed_weights.into(),
            max_weights_limit: max_weights_limit.into(),
            scaling_law_power: scaling_law_power.into(),
            subnetwork_n: subnetwork_n.into(),
            max_allowed_uids: max_allowed_uids.into(),
            blocks_since_last_step: blocks_since_last_step.into(),
            tempo: tempo.into(),
            network_modality: 0_u16.into(),
            network_connect,
            emission_values: 0.into(),
            burn,
            owner: Self::get_subnet_owner(netuid),
        })
    }

    pub fn get_subnets_info() -> Vec<Option<SubnetInfo<T::AccountId>>> {
        let mut subnet_netuids = Vec::<NetUid>::new();
        let mut max_netuid: u16 = 0;
        for (netuid, added) in <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter() {
            if added {
                subnet_netuids.push(netuid);
                if u16::from(netuid) > max_netuid {
                    max_netuid = u16::from(netuid);
                }
            }
        }

        let mut subnets_info = Vec::<Option<SubnetInfo<T::AccountId>>>::new();
        for netuid_ in 0..=max_netuid {
            if subnet_netuids.contains(&netuid_.into()) {
                subnets_info.push(Self::get_subnet_info(netuid_.into()));
            }
        }

        subnets_info
    }

    pub fn get_subnet_info_v2(netuid: NetUid) -> Option<SubnetInfov2<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        let rho = Self::get_rho(netuid);
        let kappa = Self::get_kappa(netuid);
        let difficulty: Compact<u64> = Self::get_difficulty_as_u64(netuid).into();
        let immunity_period = Self::get_immunity_period(netuid);
        let max_allowed_validators = Self::get_max_allowed_validators(netuid);
        let min_allowed_weights = Self::get_min_allowed_weights(netuid);
        let max_weights_limit = Self::get_max_weight_limit(netuid);
        let scaling_law_power = Self::get_scaling_law_power(netuid);
        let subnetwork_n = Self::get_subnetwork_n(netuid);
        let max_allowed_uids = Self::get_max_allowed_uids(netuid);
        let blocks_since_last_step = Self::get_blocks_since_last_step(netuid);
        let tempo = Self::get_tempo(netuid);
        let burn = Compact::from(Self::get_burn(netuid));
        let identity: Option<SubnetIdentityV3> = SubnetIdentitiesV3::<T>::get(netuid);

        // DEPRECATED
        let network_connect: Vec<[u16; 2]> = Vec::<[u16; 2]>::new();
        // DEPRECATED for ( _netuid_, con_req) in < NetworkConnect<T> as IterableStorageDoubleMap<u16, u16, u16> >::iter_prefix(netuid) {
        //     network_connect.push([_netuid_, con_req]);
        // }

        Some(SubnetInfov2 {
            rho: rho.into(),
            kappa: kappa.into(),
            difficulty,
            immunity_period: immunity_period.into(),
            netuid: netuid.into(),
            max_allowed_validators: max_allowed_validators.into(),
            min_allowed_weights: min_allowed_weights.into(),
            max_weights_limit: max_weights_limit.into(),
            scaling_law_power: scaling_law_power.into(),
            subnetwork_n: subnetwork_n.into(),
            max_allowed_uids: max_allowed_uids.into(),
            blocks_since_last_step: blocks_since_last_step.into(),
            tempo: tempo.into(),
            network_modality: 0.into(),
            network_connect,
            emission_value: 0.into(),
            burn,
            owner: Self::get_subnet_owner(netuid),
            identity,
        })
    }

    pub fn get_subnets_info_v2() -> Vec<Option<SubnetInfov2<T::AccountId>>> {
        let mut subnet_netuids = Vec::<NetUid>::new();
        let mut max_netuid: u16 = 0;
        for (netuid, added) in <NetworksAdded<T> as IterableStorageMap<NetUid, bool>>::iter() {
            if added {
                subnet_netuids.push(netuid);
                if u16::from(netuid) > max_netuid {
                    max_netuid = u16::from(netuid);
                }
            }
        }

        let mut subnets_info = Vec::<Option<SubnetInfov2<T::AccountId>>>::new();
        for netuid_ in 0..=max_netuid {
            if subnet_netuids.contains(&netuid_.into()) {
                subnets_info.push(Self::get_subnet_info_v2(netuid_.into()));
            }
        }

        subnets_info
    }

    pub fn get_subnet_hyperparams(netuid: NetUid) -> Option<SubnetHyperparams> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        let rho = Self::get_rho(netuid);
        let kappa = Self::get_kappa(netuid);
        let immunity_period = Self::get_immunity_period(netuid);
        let min_allowed_weights = Self::get_min_allowed_weights(netuid);
        let max_weights_limit = Self::get_max_weight_limit(netuid);
        let tempo = Self::get_tempo(netuid);
        let min_difficulty = Self::get_min_difficulty(netuid);
        let max_difficulty = Self::get_max_difficulty(netuid);
        let weights_version = Self::get_weights_version_key(netuid);
        let weights_rate_limit = Self::get_weights_set_rate_limit(netuid);
        let adjustment_interval = Self::get_adjustment_interval(netuid);
        let activity_cutoff = Self::get_activity_cutoff_blocks(netuid);
        let registration_allowed = Self::get_network_registration_allowed(netuid);
        let target_regs_per_interval = Self::get_target_registrations_per_interval(netuid);
        let min_burn = Self::get_min_burn(netuid);
        let max_burn = Self::get_max_burn(netuid);
        let bonds_moving_avg = Self::get_bonds_moving_average(netuid);
        let max_regs_per_block = Self::get_max_registrations_per_block(netuid);
        let serving_rate_limit = Self::get_serving_rate_limit(netuid);
        let max_validators = Self::get_max_allowed_validators(netuid);
        let adjustment_alpha = Self::get_adjustment_alpha(netuid);
        let difficulty = Self::get_difficulty_as_u64(netuid);
        let commit_reveal_periods = Self::get_reveal_period(netuid);
        let commit_reveal_weights_enabled = Self::get_commit_reveal_weights_enabled(netuid);
        let liquid_alpha_enabled = Self::get_liquid_alpha_enabled(netuid);
        let (alpha_low, alpha_high): (u16, u16) = Self::get_alpha_values(netuid);

        Some(SubnetHyperparams {
            rho: rho.into(),
            kappa: kappa.into(),
            immunity_period: immunity_period.into(),
            min_allowed_weights: min_allowed_weights.into(),
            max_weights_limit: max_weights_limit.into(),
            tempo: tempo.into(),
            min_difficulty: min_difficulty.into(),
            max_difficulty: max_difficulty.into(),
            weights_version: weights_version.into(),
            weights_rate_limit: weights_rate_limit.into(),
            adjustment_interval: adjustment_interval.into(),
            activity_cutoff: activity_cutoff.into(),
            registration_allowed,
            target_regs_per_interval: target_regs_per_interval.into(),
            min_burn: min_burn.into(),
            max_burn: max_burn.into(),
            bonds_moving_avg: bonds_moving_avg.into(),
            max_regs_per_block: max_regs_per_block.into(),
            serving_rate_limit: serving_rate_limit.into(),
            max_validators: max_validators.into(),
            adjustment_alpha: adjustment_alpha.into(),
            difficulty: difficulty.into(),
            commit_reveal_period: commit_reveal_periods.into(),
            commit_reveal_weights_enabled,
            alpha_high: alpha_high.into(),
            alpha_low: alpha_low.into(),
            liquid_alpha_enabled,
        })
    }

    pub fn get_subnet_hyperparams_v2(netuid: NetUid) -> Option<SubnetHyperparamsV2> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        let rho = Self::get_rho(netuid);
        let kappa = Self::get_kappa(netuid);
        let immunity_period = Self::get_immunity_period(netuid);
        let min_allowed_weights = Self::get_min_allowed_weights(netuid);
        let max_weights_limit = Self::get_max_weight_limit(netuid);
        let tempo = Self::get_tempo(netuid);
        let min_difficulty = Self::get_min_difficulty(netuid);
        let max_difficulty = Self::get_max_difficulty(netuid);
        let weights_version = Self::get_weights_version_key(netuid);
        let weights_rate_limit = Self::get_weights_set_rate_limit(netuid);
        let adjustment_interval = Self::get_adjustment_interval(netuid);
        let activity_cutoff = Self::get_activity_cutoff_blocks(netuid);
        let registration_allowed = Self::get_network_registration_allowed(netuid);
        let target_regs_per_interval = Self::get_target_registrations_per_interval(netuid);
        let min_burn = Self::get_min_burn(netuid);
        let max_burn = Self::get_max_burn(netuid);
        let bonds_moving_avg = Self::get_bonds_moving_average(netuid);
        let max_regs_per_block = Self::get_max_registrations_per_block(netuid);
        let serving_rate_limit = Self::get_serving_rate_limit(netuid);
        let max_validators = Self::get_max_allowed_validators(netuid);
        let adjustment_alpha = Self::get_adjustment_alpha(netuid);
        let difficulty = Self::get_difficulty_as_u64(netuid);
        let commit_reveal_period = Self::get_reveal_period(netuid);
        let commit_reveal_weights_enabled = Self::get_commit_reveal_weights_enabled(netuid);
        let liquid_alpha_enabled = Self::get_liquid_alpha_enabled(netuid);
        let (alpha_low, alpha_high): (u16, u16) = Self::get_alpha_values(netuid);
        let alpha_sigmoid_steepness = Self::get_alpha_sigmoid_steepness(netuid);
        let yuma_version: u16 = match Self::get_yuma3_enabled(netuid) {
            true => 3u16,
            false => 2u16,
        };
        let subnet_token_enabled = Self::get_subtoken_enabled(netuid);
        let transfers_enabled = Self::get_transfer_toggle(netuid);
        let bonds_reset = Self::get_bonds_reset(netuid);

        Some(SubnetHyperparamsV2 {
            rho: rho.into(),
            kappa: kappa.into(),
            immunity_period: immunity_period.into(),
            min_allowed_weights: min_allowed_weights.into(),
            max_weights_limit: max_weights_limit.into(),
            tempo: tempo.into(),
            min_difficulty: min_difficulty.into(),
            max_difficulty: max_difficulty.into(),
            weights_version: weights_version.into(),
            weights_rate_limit: weights_rate_limit.into(),
            adjustment_interval: adjustment_interval.into(),
            activity_cutoff: activity_cutoff.into(),
            registration_allowed,
            target_regs_per_interval: target_regs_per_interval.into(),
            min_burn: min_burn.into(),
            max_burn: max_burn.into(),
            bonds_moving_avg: bonds_moving_avg.into(),
            max_regs_per_block: max_regs_per_block.into(),
            serving_rate_limit: serving_rate_limit.into(),
            max_validators: max_validators.into(),
            adjustment_alpha: adjustment_alpha.into(),
            difficulty: difficulty.into(),
            commit_reveal_period: commit_reveal_period.into(),
            commit_reveal_weights_enabled,
            alpha_high: alpha_high.into(),
            alpha_low: alpha_low.into(),
            liquid_alpha_enabled,
            alpha_sigmoid_steepness,
            yuma_version: yuma_version.into(),
            subnet_is_active: subnet_token_enabled,
            transfers_enabled,
            bonds_reset_enabled: bonds_reset,
            user_liquidity_enabled: false,
        })
    }

    pub fn get_coldkey_auto_stake_hotkey(
        coldkey: T::AccountId,
        netuid: NetUid,
    ) -> Option<T::AccountId> {
        AutoStakeDestination::<T>::get(coldkey, netuid)
    }

    /// Returns every hyperparameter for `netuid` as a flat
    /// [`SubnetHyperparamsV3`] list, or `None` if the subnet does not exist.
    ///
    /// Adding a new hyperparameter is a single `(name, value).into()` push
    /// below — no struct edit and no V4 required, provided the value's type
    /// already has a [`HyperparamValue`] variant.
    pub fn get_subnet_hyperparams_v3(netuid: NetUid) -> Option<SubnetHyperparamsV3> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        let (alpha_low, alpha_high): (u16, u16) = Self::get_alpha_values(netuid);
        let yuma_version: u16 = if Self::get_yuma3_enabled(netuid) {
            3
        } else {
            2
        };

        Some(alloc::vec![
            (
                "kappa",
                HyperparamValue::U16(Self::get_kappa(netuid).into())
            )
                .into(),
            (
                "immunity_period",
                HyperparamValue::U16(Self::get_immunity_period(netuid).into()),
            )
                .into(),
            (
                "min_allowed_weights",
                HyperparamValue::U16(Self::get_min_allowed_weights(netuid).into()),
            )
                .into(),
            (
                "max_weights_limit",
                HyperparamValue::U16(Self::get_max_weight_limit(netuid).into()),
            )
                .into(),
            (
                "tempo",
                HyperparamValue::U16(Self::get_tempo(netuid).into())
            )
                .into(),
            (
                "weights_version",
                HyperparamValue::U64(Self::get_weights_version_key(netuid).into()),
            )
                .into(),
            (
                "weights_rate_limit",
                HyperparamValue::U64(Self::get_weights_set_rate_limit(netuid).into()),
            )
                .into(),
            (
                "activity_cutoff",
                HyperparamValue::U64(Self::get_activity_cutoff_blocks(netuid).into()),
            )
                .into(),
            (
                "activity_cutoff_factor",
                HyperparamValue::U32(Self::get_activity_cutoff_factor_milli(netuid).into()),
            )
                .into(),
            (
                "registration_allowed",
                HyperparamValue::Bool(Self::get_network_registration_allowed(netuid)),
            )
                .into(),
            (
                "target_regs_per_interval",
                HyperparamValue::U16(Self::get_target_registrations_per_interval(netuid).into()),
            )
                .into(),
            (
                "min_burn",
                HyperparamValue::TaoBalance(Self::get_min_burn(netuid).into()),
            )
                .into(),
            (
                "max_burn",
                HyperparamValue::TaoBalance(Self::get_max_burn(netuid).into()),
            )
                .into(),
            (
                "burn_half_life",
                HyperparamValue::U16(BurnHalfLife::<T>::get(netuid).into()),
            )
                .into(),
            (
                "burn_increase_mult",
                HyperparamValue::U64F64(BurnIncreaseMult::<T>::get(netuid)),
            )
                .into(),
            (
                "bonds_moving_avg",
                HyperparamValue::U64(Self::get_bonds_moving_average(netuid).into()),
            )
                .into(),
            (
                "max_regs_per_block",
                HyperparamValue::U16(Self::get_max_registrations_per_block(netuid).into()),
            )
                .into(),
            (
                "serving_rate_limit",
                HyperparamValue::U64(Self::get_serving_rate_limit(netuid).into()),
            )
                .into(),
            (
                "max_validators",
                HyperparamValue::U16(Self::get_max_allowed_validators(netuid).into()),
            )
                .into(),
            (
                "commit_reveal_period",
                HyperparamValue::U64(Self::get_reveal_period(netuid).into()),
            )
                .into(),
            (
                "commit_reveal_weights_enabled",
                HyperparamValue::Bool(Self::get_commit_reveal_weights_enabled(netuid)),
            )
                .into(),
            ("alpha_high", HyperparamValue::U16(alpha_high.into())).into(),
            ("alpha_low", HyperparamValue::U16(alpha_low.into())).into(),
            (
                "liquid_alpha_enabled",
                HyperparamValue::Bool(Self::get_liquid_alpha_enabled(netuid)),
            )
                .into(),
            (
                "alpha_sigmoid_steepness",
                HyperparamValue::I32F32(Self::get_alpha_sigmoid_steepness(netuid)),
            )
                .into(),
            ("yuma_version", HyperparamValue::U16(Compact(yuma_version)),).into(),
            (
                "subnet_is_active",
                HyperparamValue::Bool(Self::get_subtoken_enabled(netuid)),
            )
                .into(),
            (
                "transfers_enabled",
                HyperparamValue::Bool(Self::get_transfer_toggle(netuid)),
            )
                .into(),
            (
                "bonds_reset_enabled",
                HyperparamValue::Bool(Self::get_bonds_reset(netuid)),
            )
                .into(),
            ("user_liquidity_enabled", HyperparamValue::Bool(false),).into(),
            (
                "owner_cut_enabled",
                HyperparamValue::Bool(Self::get_owner_cut_enabled(netuid)),
            )
                .into(),
            (
                "owner_cut_auto_lock_enabled",
                HyperparamValue::Bool(Self::get_owner_cut_auto_lock_enabled(netuid)),
            )
                .into(),
            (
                "min_childkey_take",
                HyperparamValue::U16(Self::get_effective_min_childkey_take(netuid).into()),
            )
                .into(),
        ])
    }
}
