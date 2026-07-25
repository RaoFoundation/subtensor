//! Transaction and hyperparameter rate-limit keys for Subtensor extrinsics.
//!
//! [`TransactionType`] maps to a `u16` stored in [`TransactionKeyLastBlock`] (or special-cased
//! storage for network register / SN owner hotkey / owner hyperparams). Those `u16` codes are
//! frozen wire/storage discriminants — append new variants at the end; do not renumber.
//!
//! [`Hyperparameter`] discriminants are likewise frozen (used inside
//! [`RateLimitKey::OwnerHyperparamUpdate`]).

use subtensor_runtime_common::NetUid;

use super::*;

/// Extrinsic / admin action categories that share the rate-limit storage path.
///
/// `Into<u16>` values are persisted; see module docs before reordering variants.
#[derive(Copy, Clone)]
#[non_exhaustive]
pub enum TransactionType {
    SetChildren,
    SetChildkeyTake,
    Unknown,
    RegisterNetwork,
    SetWeightsVersionKey,
    SetSNOwnerHotkey,
    OwnerHyperparamUpdate(Hyperparameter),
    MechanismCountUpdate,
    MechanismEmission,
    MaxUidsTrimming,
    AddStakeBurn,
    TempoUpdate,
}

impl TransactionType {
    /// Global (non-subnet) rate limit in blocks for this transaction type.
    pub fn rate_limit<T: Config>(&self) -> u64 {
        match self {
            Self::SetChildren => 150, // 30 minutes
            Self::SetChildkeyTake => TxChildkeyTakeRateLimit::<T>::get(),
            Self::RegisterNetwork => NetworkRateLimit::<T>::get(),
            Self::MechanismCountUpdate => MechanismCountSetRateLimit::<T>::get(),
            Self::MechanismEmission => MechanismEmissionRateLimit::<T>::get(),
            Self::MaxUidsTrimming => MaxUidsTrimmingRateLimit::<T>::get(),
            Self::Unknown => 0, // Default to no limit for unknown types (no limit)
            _ => 0,
        }
    }

    /// Subnet-scoped rate limit in blocks (tempo-multiplied for owner hyperparams / weights key).
    pub fn rate_limit_on_subnet<T: Config>(&self, netuid: NetUid) -> u64 {
        #[allow(clippy::match_single_binding)]
        match self {
            Self::SetWeightsVersionKey => (Tempo::<T>::get(netuid) as u64)
                .saturating_mul(WeightsVersionKeyRateLimit::<T>::get()),
            // Owner hyperparameter updates are rate-limited by N tempos on the subnet (sudo configurable)
            Self::OwnerHyperparamUpdate(_) => {
                let epochs = OwnerHyperparamRateLimit::<T>::get() as u64;
                (Tempo::<T>::get(netuid) as u64).saturating_mul(epochs)
            }
            Self::SetSNOwnerHotkey => DefaultSetSNOwnerHotkeyRateLimit::<T>::get(),
            Self::AddStakeBurn => Tempo::<T>::get(netuid) as u64,
            Self::TempoUpdate => MIN_TEMPO as u64,

            _ => self.rate_limit::<T>(),
        }
    }

    /// Whether `key` may submit this global transaction type at the current block.
    pub fn passes_rate_limit<T: Config>(&self, key: &T::AccountId) -> bool {
        let block = Pallet::<T>::get_current_block_as_u64();
        let limit = self.rate_limit::<T>();
        let last_block = self.last_block::<T>(key);

        Self::check_passes_rate_limit(limit, block, last_block)
    }

    /// `true` when `last_block == 0` (never used) or `block - last_block >= limit`.
    pub fn check_passes_rate_limit(limit: u64, block: u64, last_block: u64) -> bool {
        // Allow the first transaction (when last_block is 0) or if the rate limit has passed
        last_block == 0 || block.saturating_sub(last_block) >= limit
    }

    /// Whether `hotkey` may submit this transaction type on `netuid` at the current block.
    pub fn passes_rate_limit_on_subnet<T: Config>(
        &self,
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> bool {
        let block = Pallet::<T>::get_current_block_as_u64();
        let limit = self.rate_limit_on_subnet::<T>(netuid);
        let last_block = self.last_block_on_subnet::<T>(hotkey, netuid);

        Self::check_passes_rate_limit(limit, block, last_block)
    }

    /// Block of the last global transaction for `key` and this type.
    pub fn last_block<T: Config>(&self, key: &T::AccountId) -> u64 {
        match self {
            Self::RegisterNetwork => Pallet::<T>::get_network_last_lock_block(),
            _ => self.last_block_on_subnet::<T>(key, NetUid::ROOT),
        }
    }

    /// Block of the last subnet-scoped transaction for `hotkey` / `netuid` / this type.
    pub fn last_block_on_subnet<T: Config>(&self, hotkey: &T::AccountId, netuid: NetUid) -> u64 {
        match self {
            Self::RegisterNetwork => Pallet::<T>::get_network_last_lock_block(),
            Self::SetSNOwnerHotkey => {
                Pallet::<T>::get_rate_limited_last_block(&RateLimitKey::SetSNOwnerHotkey(netuid))
            }
            Self::OwnerHyperparamUpdate(hparam) => Pallet::<T>::get_rate_limited_last_block(
                &RateLimitKey::OwnerHyperparamUpdate(netuid, *hparam),
            ),
            _ => {
                let tx_type: u16 = (*self).into();
                TransactionKeyLastBlock::<T>::get((hotkey, netuid, tx_type))
            }
        }
    }

    /// Record `block` as the last submission time for this type on `netuid`.
    pub fn set_last_block_on_subnet<T: Config>(
        &self,
        key: &T::AccountId,
        netuid: NetUid,
        block: u64,
    ) {
        match self {
            Self::RegisterNetwork => Pallet::<T>::set_network_last_lock_block(block),
            Self::SetSNOwnerHotkey => Pallet::<T>::set_rate_limited_last_block(
                &RateLimitKey::SetSNOwnerHotkey(netuid),
                block,
            ),
            Self::OwnerHyperparamUpdate(hparam) => Pallet::<T>::set_rate_limited_last_block(
                &RateLimitKey::OwnerHyperparamUpdate(netuid, *hparam),
                block,
            ),
            _ => {
                let tx_type: u16 = (*self).into();
                TransactionKeyLastBlock::<T>::insert((key, netuid, tx_type), block);
            }
        }
    }
}

/// Frozen `u16` codes persisted in [`TransactionKeyLastBlock`] — do not renumber.
impl From<TransactionType> for u16 {
    fn from(tx_type: TransactionType) -> Self {
        match tx_type {
            TransactionType::SetChildren => 0,
            TransactionType::SetChildkeyTake => 1,
            TransactionType::Unknown => 2,
            TransactionType::RegisterNetwork => 3,
            TransactionType::SetWeightsVersionKey => 4,
            TransactionType::SetSNOwnerHotkey => 5,
            TransactionType::OwnerHyperparamUpdate(_) => 6,
            TransactionType::MechanismCountUpdate => 7,
            TransactionType::MechanismEmission => 8,
            TransactionType::MaxUidsTrimming => 9,
            TransactionType::AddStakeBurn => 10,
            TransactionType::TempoUpdate => 11,
        }
    }
}

/// Inverse of [`From<TransactionType> for u16`]; unknown codes map to [`TransactionType::Unknown`].
impl From<u16> for TransactionType {
    fn from(value: u16) -> Self {
        match value {
            0 => TransactionType::SetChildren,
            1 => TransactionType::SetChildkeyTake,
            3 => TransactionType::RegisterNetwork,
            4 => TransactionType::SetWeightsVersionKey,
            5 => TransactionType::SetSNOwnerHotkey,
            6 => TransactionType::OwnerHyperparamUpdate(Hyperparameter::Unknown),
            7 => TransactionType::MechanismCountUpdate,
            8 => TransactionType::MechanismEmission,
            9 => TransactionType::MaxUidsTrimming,
            10 => TransactionType::AddStakeBurn,
            11 => TransactionType::TempoUpdate,
            _ => TransactionType::Unknown,
        }
    }
}

impl From<Hyperparameter> for TransactionType {
    fn from(param: Hyperparameter) -> Self {
        Self::OwnerHyperparamUpdate(param)
    }
}

/// Owner-settable subnet hyperparameters used as rate-limit sub-keys.
///
/// Explicit discriminants are stored on-chain via [`RateLimitKey::OwnerHyperparamUpdate`] —
/// append only; do not renumber existing variants.
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo)]
#[non_exhaustive]
pub enum Hyperparameter {
    Unknown = 0,
    ServingRateLimit = 1,
    MaxDifficulty = 2,
    AdjustmentAlpha = 3,
    MaxWeightLimit = 4,
    ImmunityPeriod = 5,
    MinAllowedWeights = 6,
    Kappa = 7,
    Rho = 8,
    ActivityCutoff = 9,
    PowRegistrationAllowed = 10,
    MinBurn = 11,
    MaxBurn = 12,
    BondsMovingAverage = 13,
    BondsPenalty = 14,
    CommitRevealEnabled = 15,
    LiquidAlphaEnabled = 16,
    AlphaValues = 17,
    WeightCommitInterval = 18,
    TransferEnabled = 19,
    AlphaSigmoidSteepness = 20,
    Yuma3Enabled = 21,
    BondsResetEnabled = 22,
    ImmuneNeuronLimit = 23,
    RecycleOrBurn = 24,
    MaxAllowedUids = 25,
    BurnHalfLife = 26,
    BurnIncreaseMult = 27,
    SubnetEmissionEnabled = 28,
    MinChildkeyTake = 29,
    ActivityCutoffFactorMilli = 30,
    TriggerEpoch = 31,
    CollateralLockShare = 32,
    CollateralDrainRatio = 33,
}

impl<T: Config> Pallet<T> {
    // ========================
    // ==== Rate Limiting =====
    // ========================

    /// Clear the last generic tx-block marker for `key`.
    pub fn remove_last_tx_block(key: &T::AccountId) {
        Self::remove_rate_limited_last_block(&RateLimitKey::LastTxBlock(key.clone()))
    }
    /// Record the last generic tx-block for `key`.
    pub fn set_last_tx_block(key: &T::AccountId, block: u64) {
        Self::set_rate_limited_last_block(&RateLimitKey::LastTxBlock(key.clone()), block);
    }
    /// Last block at which `key` submitted a generic rate-limited tx.
    pub fn get_last_tx_block(key: &T::AccountId) -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::LastTxBlock(key.clone()))
    }

    /// Clear the last delegate-take tx-block marker for `key`.
    pub fn remove_last_tx_block_delegate_take(key: &T::AccountId) {
        Self::remove_rate_limited_last_block(&RateLimitKey::LastTxBlockDelegateTake(key.clone()))
    }
    /// Record the last delegate-take tx-block for `key`.
    pub fn set_last_tx_block_delegate_take(key: &T::AccountId, block: u64) {
        Self::set_rate_limited_last_block(
            &RateLimitKey::LastTxBlockDelegateTake(key.clone()),
            block,
        );
    }
    /// Last block at which `key` updated delegate take.
    pub fn get_last_tx_block_delegate_take(key: &T::AccountId) -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::LastTxBlockDelegateTake(key.clone()))
    }
    /// Last block at which `key` updated childkey take.
    pub fn get_last_tx_block_childkey_take(key: &T::AccountId) -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::LastTxBlockChildKeyTake(key.clone()))
    }
    /// Clear the last childkey-take tx-block marker for `key`.
    pub fn remove_last_tx_block_childkey(key: &T::AccountId) {
        Self::remove_rate_limited_last_block(&RateLimitKey::LastTxBlockChildKeyTake(key.clone()))
    }
    /// Record the last childkey-take tx-block for `key`.
    pub fn set_last_tx_block_childkey(key: &T::AccountId, block: u64) {
        Self::set_rate_limited_last_block(
            &RateLimitKey::LastTxBlockChildKeyTake(key.clone()),
            block,
        );
    }
    /// `true` if `current_block - prev_tx_block` is still within the global tx rate limit.
    pub fn exceeds_tx_rate_limit(prev_tx_block: u64, current_block: u64) -> bool {
        let rate_limit: u64 = Self::get_tx_rate_limit();
        if rate_limit == 0 || prev_tx_block == 0 {
            return false;
        }

        current_block.saturating_sub(prev_tx_block) <= rate_limit
    }
    /// `true` if `current_block - prev_tx_block` is still within the delegate-take rate limit.
    pub fn exceeds_tx_delegate_take_rate_limit(prev_tx_block: u64, current_block: u64) -> bool {
        let rate_limit: u64 = Self::get_tx_delegate_take_rate_limit();
        if rate_limit == 0 || prev_tx_block == 0 {
            return false;
        }

        current_block.saturating_sub(prev_tx_block) <= rate_limit
    }
}
