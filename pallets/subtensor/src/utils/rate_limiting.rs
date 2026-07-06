use subtensor_runtime_common::NetUid;

use super::*;

/// Enum representing different types of transactions
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
    /// Get the rate limit for a specific transaction type
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

    pub fn passes_rate_limit<T: Config>(&self, key: &T::AccountId) -> bool {
        let block = Pallet::<T>::get_current_block_as_u64();
        let limit = self.rate_limit::<T>();
        let last_block = self.last_block::<T>(key);

        Self::check_passes_rate_limit(limit, block, last_block)
    }

    pub fn check_passes_rate_limit(limit: u64, block: u64, last_block: u64) -> bool {
        // Allow the first transaction (when last_block is 0) or if the rate limit has passed
        last_block == 0 || block.saturating_sub(last_block) >= limit
    }

    /// Check if a transaction should be rate limited on a specific subnet
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

    /// Get the block number of the last transaction for a specific key, and transaction type
    pub fn last_block<T: Config>(&self, key: &T::AccountId) -> u64 {
        match self {
            Self::RegisterNetwork => Pallet::<T>::get_network_last_lock_block(),
            _ => self.last_block_on_subnet::<T>(key, NetUid::ROOT),
        }
    }

    /// Get the block number of the last transaction for a specific hotkey, network, and transaction
    /// type
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

    /// Set the block number of the last transaction for a specific hotkey, network, and transaction
    /// type
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

/// Implement conversion from TransactionType to u16
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

/// Implement conversion from u16 to TransactionType
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
}

impl<T: Config> Pallet<T> {
    // ========================
    // ==== Rate Limiting =====
    // ========================

    pub fn remove_last_tx_block(key: &T::AccountId) {
        Self::remove_rate_limited_last_block(&RateLimitKey::LastTxBlock(key.clone()))
    }
    pub fn set_last_tx_block(key: &T::AccountId, block: u64) {
        Self::set_rate_limited_last_block(&RateLimitKey::LastTxBlock(key.clone()), block);
    }
    pub fn get_last_tx_block(key: &T::AccountId) -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::LastTxBlock(key.clone()))
    }

    pub fn remove_last_tx_block_delegate_take(key: &T::AccountId) {
        Self::remove_rate_limited_last_block(&RateLimitKey::LastTxBlockDelegateTake(key.clone()))
    }
    pub fn set_last_tx_block_delegate_take(key: &T::AccountId, block: u64) {
        Self::set_rate_limited_last_block(
            &RateLimitKey::LastTxBlockDelegateTake(key.clone()),
            block,
        );
    }
    pub fn get_last_tx_block_delegate_take(key: &T::AccountId) -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::LastTxBlockDelegateTake(key.clone()))
    }
    pub fn get_last_tx_block_childkey_take(key: &T::AccountId) -> u64 {
        Self::get_rate_limited_last_block(&RateLimitKey::LastTxBlockChildKeyTake(key.clone()))
    }
    pub fn remove_last_tx_block_childkey(key: &T::AccountId) {
        Self::remove_rate_limited_last_block(&RateLimitKey::LastTxBlockChildKeyTake(key.clone()))
    }
    pub fn set_last_tx_block_childkey(key: &T::AccountId, block: u64) {
        Self::set_rate_limited_last_block(
            &RateLimitKey::LastTxBlockChildKeyTake(key.clone()),
            block,
        );
    }
    pub fn exceeds_tx_rate_limit(prev_tx_block: u64, current_block: u64) -> bool {
        let rate_limit: u64 = Self::get_tx_rate_limit();
        if rate_limit == 0 || prev_tx_block == 0 {
            return false;
        }

        current_block.saturating_sub(prev_tx_block) <= rate_limit
    }
    pub fn exceeds_tx_delegate_take_rate_limit(prev_tx_block: u64, current_block: u64) -> bool {
        let rate_limit: u64 = Self::get_tx_delegate_take_rate_limit();
        if rate_limit == 0 || prev_tx_block == 0 {
            return false;
        }

        current_block.saturating_sub(prev_tx_block) <= rate_limit
    }
}
