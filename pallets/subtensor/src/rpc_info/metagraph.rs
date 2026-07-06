use super::*;
extern crate alloc;
use crate::epoch::math::*;
use codec::Compact;
use frame_support::IterableStorageDoubleMap;
use frame_support::pallet_prelude::{Decode, Encode};
use pallet_commitments::GetCommitments;
use substrate_fixed::types::I64F64;
use substrate_fixed::types::I96F32;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, NetUidStorageIndex, TaoBalance};

#[freeze_struct("54520f5534d7e59e")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct Metagraph<AccountId: TypeInfo + Encode + Decode> {
    // Subnet index
    netuid: Compact<NetUid>,

    // Name and symbol
    name: Vec<Compact<u8>>,              // name
    symbol: Vec<Compact<u8>>,            // token symbol
    identity: Option<SubnetIdentityV3>,  // identity information.
    network_registered_at: Compact<u64>, // block at registration

    // Keys for owner.
    owner_hotkey: AccountId,  // hotkey
    owner_coldkey: AccountId, // coldkey.

    // Tempo terms.
    block: Compact<u64>,                  // block at call.
    tempo: Compact<u16>,                  // epoch tempo
    last_step: Compact<u64>,              // last epoch
    blocks_since_last_step: Compact<u64>, // blocks since last epoch.

    // Subnet emission terms
    subnet_emission: Compact<u64>,    // subnet emission via stao
    alpha_in: Compact<AlphaBalance>,  // amount of alpha in reserve
    alpha_out: Compact<AlphaBalance>, // amount of alpha outstanding
    tao_in: Compact<TaoBalance>,      // amount of tao injected per block
    alpha_out_emission: Compact<AlphaBalance>, // amount injected in alpha reserves per block
    alpha_in_emission: Compact<AlphaBalance>, // amount injected outstanding per block
    tao_in_emission: Compact<TaoBalance>, // amount of tao injected per block
    pending_alpha_emission: Compact<AlphaBalance>, // pending alpha to be distributed
    pending_root_emission: Compact<TaoBalance>, // pending tao for root divs to be distributed
    subnet_volume: Compact<u128>,     // volume of the subnet in TAO
    moving_price: I96F32,             // subnet moving price.

    // Hparams for epoch
    rho: Compact<u16>,   // subnet rho param
    kappa: Compact<u16>, // subnet kappa param

    // Validator params
    min_allowed_weights: Compact<u16>, // min allowed weights per val
    max_weights_limit: Compact<u16>,   // max allowed weights per val
    weights_version: Compact<u64>,     // allowed weights version
    weights_rate_limit: Compact<u64>,  // rate limit on weights
    activity_cutoff: Compact<u64>,     // validator weights cut off period in blocks
    max_validators: Compact<u16>,      // max allowed validators

    // Registration
    num_uids: Compact<u16>,
    max_uids: Compact<u16>,
    burn: Compact<TaoBalance>,              // current burn cost
    difficulty: Compact<u64>,               // current difficulty
    registration_allowed: bool,             // allows registrations
    pow_registration_allowed: bool,         // pow registration enabled
    immunity_period: Compact<u16>,          // subnet miner immunity period
    min_difficulty: Compact<u64>,           // min pow difficulty
    max_difficulty: Compact<u64>,           // max pow difficulty
    min_burn: Compact<TaoBalance>,          // min tao burn
    max_burn: Compact<TaoBalance>,          // max tao burn
    adjustment_alpha: Compact<u64>,         // adjustment speed for registration params
    adjustment_interval: Compact<u16>,      // pow and burn adjustment interval
    target_regs_per_interval: Compact<u16>, // target registrations per interval
    max_regs_per_block: Compact<u16>,       // max registrations per block
    serving_rate_limit: Compact<u64>,       // axon serving rate limit

    // CR
    commit_reveal_weights_enabled: bool, // Is CR enabled
    commit_reveal_period: Compact<u64>,  // Commit reveal interval

    // Bonds
    liquid_alpha_enabled: bool,     // Bonds liquid enabled
    alpha_high: Compact<u16>,       // Alpha param high
    alpha_low: Compact<u16>,        // Alpha param low
    bonds_moving_avg: Compact<u64>, // Bonds moving avg

    // Metagraph info.
    hotkeys: Vec<AccountId>,                    // hotkey per UID
    coldkeys: Vec<AccountId>,                   // coldkey per UID
    identities: Vec<Option<ChainIdentityOfV2>>, // coldkeys identities
    axons: Vec<AxonInfo>,                       // UID axons
    active: Vec<bool>,                          // Avtive per UID
    validator_permit: Vec<bool>,                // Val permit per UID
    pruning_score: Vec<Compact<u16>>,           // Pruning per UID
    last_update: Vec<Compact<u64>>,             // Last update per UID
    emission: Vec<Compact<AlphaBalance>>,       // Emission per UID
    dividends: Vec<Compact<u16>>,               // Dividends per UID
    incentives: Vec<Compact<u16>>,              // Mining incentives per UID
    consensus: Vec<Compact<u16>>,               // Consensus per UID
    trust: Vec<Compact<u16>>,                   // Trust per UID
    rank: Vec<Compact<u16>>,                    // Rank per UID
    block_at_registration: Vec<Compact<u64>>,   // Reg block per UID
    alpha_stake: Vec<Compact<AlphaBalance>>,    // Alpha staked per UID
    tao_stake: Vec<Compact<TaoBalance>>,        // TAO staked per UID
    total_stake: Vec<Compact<TaoBalance>>,      // Total stake per UID

    // Dividend break down.
    tao_dividends_per_hotkey: Vec<(AccountId, Compact<TaoBalance>)>, // List of dividend payouts in tao via root.
    alpha_dividends_per_hotkey: Vec<(AccountId, Compact<AlphaBalance>)>, // List of dividend payout in alpha via subnet.
}

#[freeze_struct("5f9c8beab622882c")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SelectiveMetagraph<AccountId: TypeInfo + Encode + Decode + Clone> {
    // Subnet index
    netuid: Compact<NetUid>,

    // Name and symbol
    name: Option<Vec<Compact<u8>>>,              // name
    symbol: Option<Vec<Compact<u8>>>,            // token symbol
    identity: Option<Option<SubnetIdentityV3>>,  // identity information
    network_registered_at: Option<Compact<u64>>, // block at registration

    // Keys for owner.
    owner_hotkey: Option<AccountId>,  // hotkey
    owner_coldkey: Option<AccountId>, // coldkey

    // Tempo terms
    block: Option<Compact<u64>>,                  // block at call
    tempo: Option<Compact<u16>>,                  // epoch tempo
    last_step: Option<Compact<u64>>,              // last epoch
    blocks_since_last_step: Option<Compact<u64>>, // blocks since last epoch

    // Subnet emission terms
    subnet_emission: Option<Compact<u64>>, // subnet emission via stao
    alpha_in: Option<Compact<AlphaBalance>>, // amount of alpha in reserve
    alpha_out: Option<Compact<AlphaBalance>>, // amount of alpha outstanding
    tao_in: Option<Compact<TaoBalance>>,   // amount of tao injected per block
    alpha_out_emission: Option<Compact<AlphaBalance>>, // amount injected in alpha reserves per block
    alpha_in_emission: Option<Compact<AlphaBalance>>,  // amount injected outstanding per block
    tao_in_emission: Option<Compact<TaoBalance>>,      // amount of tao injected per block
    pending_alpha_emission: Option<Compact<AlphaBalance>>, // pending alpha to be distributed
    pending_root_emission: Option<Compact<TaoBalance>>, // panding tao for root divs to be distributed
    subnet_volume: Option<Compact<u128>>,               // volume of the subnet in TAO
    moving_price: Option<I96F32>,                       // subnet moving price.

    // Hparams for epoch
    rho: Option<Compact<u16>>,   // subnet rho param
    kappa: Option<Compact<u16>>, // subnet kappa param

    // Validator params
    min_allowed_weights: Option<Compact<u16>>, // min allowed weights per val
    max_weights_limit: Option<Compact<u16>>,   // max allowed weights per val
    weights_version: Option<Compact<u64>>,     // allowed weights version
    weights_rate_limit: Option<Compact<u64>>,  // rate limit on weights
    activity_cutoff: Option<Compact<u64>>, // validator weights cut off period in blocks (effective = factor × tempo / 1000)
    max_validators: Option<Compact<u16>>,  // max allowed validators

    // Registration
    num_uids: Option<Compact<u16>>,
    max_uids: Option<Compact<u16>>,
    burn: Option<Compact<TaoBalance>>,         // current burn cost
    difficulty: Option<Compact<u64>>,          // current difficulty
    registration_allowed: Option<bool>,        // allows registrations
    pow_registration_allowed: Option<bool>,    // pow registration enabled
    immunity_period: Option<Compact<u16>>,     // subnet miner immunity period
    min_difficulty: Option<Compact<u64>>,      // min pow difficulty
    max_difficulty: Option<Compact<u64>>,      // max pow difficulty
    min_burn: Option<Compact<TaoBalance>>,     // min tao burn
    max_burn: Option<Compact<TaoBalance>>,     // max tao burn
    adjustment_alpha: Option<Compact<u64>>,    // adjustment speed for registration params
    adjustment_interval: Option<Compact<u16>>, // pow and burn adjustment interval
    target_regs_per_interval: Option<Compact<u16>>, // target registrations per interval
    max_regs_per_block: Option<Compact<u16>>,  // max registrations per block
    serving_rate_limit: Option<Compact<u64>>,  // axon serving rate limit

    // CR
    commit_reveal_weights_enabled: Option<bool>, // Is CR enabled
    commit_reveal_period: Option<Compact<u64>>,  // Commit reveal interval

    // Bonds
    liquid_alpha_enabled: Option<bool>,     // Bonds liquid enabled
    alpha_high: Option<Compact<u16>>,       // Alpha param high
    alpha_low: Option<Compact<u16>>,        // Alpha param low
    bonds_moving_avg: Option<Compact<u64>>, // Bonds moving avg

    // Metagraph info.
    hotkeys: Option<Vec<AccountId>>,  // hotkey per UID
    coldkeys: Option<Vec<AccountId>>, // coldkey per UID
    identities: Option<Vec<Option<ChainIdentityOfV2>>>, // coldkeys identities
    axons: Option<Vec<AxonInfo>>,     // UID axons.
    active: Option<Vec<bool>>,        // Avtive per UID
    validator_permit: Option<Vec<bool>>, // Val permit per UID
    pruning_score: Option<Vec<Compact<u16>>>, // Pruning per UID
    last_update: Option<Vec<Compact<u64>>>, // Last update per UID
    emission: Option<Vec<Compact<AlphaBalance>>>, // Emission per UID
    dividends: Option<Vec<Compact<u16>>>, // Dividends per UID
    incentives: Option<Vec<Compact<u16>>>, // Mining incentives per UID
    consensus: Option<Vec<Compact<u16>>>, // Consensus per UID
    trust: Option<Vec<Compact<u16>>>, // Trust per UID
    rank: Option<Vec<Compact<u16>>>,  // Rank per UID
    block_at_registration: Option<Vec<Compact<u64>>>, // Reg block per UID
    alpha_stake: Option<Vec<Compact<AlphaBalance>>>, // Alpha staked per UID
    tao_stake: Option<Vec<Compact<TaoBalance>>>, // TAO staked per UID
    total_stake: Option<Vec<Compact<TaoBalance>>>, // Total stake per UID

    // Dividend break down.
    tao_dividends_per_hotkey: Option<Vec<(AccountId, Compact<TaoBalance>)>>, // List of dividend payouts in tao via root
    alpha_dividends_per_hotkey: Option<Vec<(AccountId, Compact<AlphaBalance>)>>, // List of dividend payout in alpha via subnet

    // validators
    validators: Option<Vec<Compact<u16>>>, // List of validators
    // commitments
    commitments: Option<Vec<(AccountId, Vec<Compact<u8>>)>>, // List of commitments
}

impl<AccountId> SelectiveMetagraph<AccountId>
where
    AccountId: TypeInfo + Encode + Decode + Clone,
{
    pub fn merge_value(&mut self, other: &Self, metagraph_index: usize) {
        match SelectiveMetagraphIndex::from_index(metagraph_index) {
            Some(SelectiveMetagraphIndex::Netuid) => self.netuid = other.netuid,
            Some(SelectiveMetagraphIndex::Name) => self.name = other.name.clone(),
            Some(SelectiveMetagraphIndex::Symbol) => self.symbol = other.symbol.clone(),
            Some(SelectiveMetagraphIndex::Identity) => self.identity = other.identity.clone(),
            Some(SelectiveMetagraphIndex::NetworkRegisteredAt) => {
                self.network_registered_at = other.network_registered_at
            }
            Some(SelectiveMetagraphIndex::OwnerHotkey) => {
                self.owner_hotkey = other.owner_hotkey.clone()
            }
            Some(SelectiveMetagraphIndex::OwnerColdkey) => {
                self.owner_coldkey = other.owner_coldkey.clone()
            }
            Some(SelectiveMetagraphIndex::Block) => self.block = other.block,
            Some(SelectiveMetagraphIndex::Tempo) => self.tempo = other.tempo,
            Some(SelectiveMetagraphIndex::LastStep) => self.last_step = other.last_step,
            Some(SelectiveMetagraphIndex::BlocksSinceLastStep) => {
                self.blocks_since_last_step = other.blocks_since_last_step
            }
            Some(SelectiveMetagraphIndex::SubnetEmission) => {
                self.subnet_emission = other.subnet_emission
            }
            Some(SelectiveMetagraphIndex::AlphaIn) => self.alpha_in = other.alpha_in,
            Some(SelectiveMetagraphIndex::AlphaOut) => self.alpha_out = other.alpha_out,
            Some(SelectiveMetagraphIndex::TaoIn) => self.tao_in = other.tao_in,
            Some(SelectiveMetagraphIndex::AlphaOutEmission) => {
                self.alpha_out_emission = other.alpha_out_emission
            }
            Some(SelectiveMetagraphIndex::AlphaInEmission) => {
                self.alpha_in_emission = other.alpha_in_emission
            }
            Some(SelectiveMetagraphIndex::TaoInEmission) => {
                self.tao_in_emission = other.tao_in_emission
            }
            Some(SelectiveMetagraphIndex::PendingAlphaEmission) => {
                self.pending_alpha_emission = other.pending_alpha_emission
            }
            Some(SelectiveMetagraphIndex::PendingRootEmission) => {
                self.pending_root_emission = other.pending_root_emission
            }
            Some(SelectiveMetagraphIndex::SubnetVolume) => self.subnet_volume = other.subnet_volume,
            Some(SelectiveMetagraphIndex::MovingPrice) => self.moving_price = other.moving_price,
            Some(SelectiveMetagraphIndex::Rho) => self.rho = other.rho,
            Some(SelectiveMetagraphIndex::Kappa) => self.kappa = other.kappa,
            Some(SelectiveMetagraphIndex::MinAllowedWeights) => {
                self.min_allowed_weights = other.min_allowed_weights
            }
            Some(SelectiveMetagraphIndex::MaxWeightsLimit) => {
                self.max_weights_limit = other.max_weights_limit
            }
            Some(SelectiveMetagraphIndex::WeightsVersion) => {
                self.weights_version = other.weights_version
            }
            Some(SelectiveMetagraphIndex::WeightsRateLimit) => {
                self.weights_rate_limit = other.weights_rate_limit
            }
            Some(SelectiveMetagraphIndex::ActivityCutoff) => {
                self.activity_cutoff = other.activity_cutoff
            }
            Some(SelectiveMetagraphIndex::MaxValidators) => {
                self.max_validators = other.max_validators
            }
            Some(SelectiveMetagraphIndex::NumUids) => self.num_uids = other.num_uids,
            Some(SelectiveMetagraphIndex::MaxUids) => self.max_uids = other.max_uids,
            Some(SelectiveMetagraphIndex::Burn) => self.burn = other.burn,
            Some(SelectiveMetagraphIndex::Difficulty) => self.difficulty = other.difficulty,
            Some(SelectiveMetagraphIndex::RegistrationAllowed) => {
                self.registration_allowed = other.registration_allowed
            }
            Some(SelectiveMetagraphIndex::PowRegistrationAllowed) => {
                self.pow_registration_allowed = other.pow_registration_allowed
            }
            Some(SelectiveMetagraphIndex::ImmunityPeriod) => {
                self.immunity_period = other.immunity_period
            }
            Some(SelectiveMetagraphIndex::MinDifficulty) => {
                self.min_difficulty = other.min_difficulty
            }
            Some(SelectiveMetagraphIndex::MaxDifficulty) => {
                self.max_difficulty = other.max_difficulty
            }
            Some(SelectiveMetagraphIndex::MinBurn) => self.min_burn = other.min_burn,
            Some(SelectiveMetagraphIndex::MaxBurn) => self.max_burn = other.max_burn,
            Some(SelectiveMetagraphIndex::AdjustmentAlpha) => {
                self.adjustment_alpha = other.adjustment_alpha
            }
            Some(SelectiveMetagraphIndex::AdjustmentInterval) => {
                self.adjustment_interval = other.adjustment_interval
            }
            Some(SelectiveMetagraphIndex::TargetRegsPerInterval) => {
                self.target_regs_per_interval = other.target_regs_per_interval
            }
            Some(SelectiveMetagraphIndex::MaxRegsPerBlock) => {
                self.max_regs_per_block = other.max_regs_per_block
            }
            Some(SelectiveMetagraphIndex::ServingRateLimit) => {
                self.serving_rate_limit = other.serving_rate_limit
            }
            Some(SelectiveMetagraphIndex::CommitRevealWeightsEnabled) => {
                self.commit_reveal_weights_enabled = other.commit_reveal_weights_enabled
            }
            Some(SelectiveMetagraphIndex::CommitRevealPeriod) => {
                self.commit_reveal_period = other.commit_reveal_period
            }
            Some(SelectiveMetagraphIndex::LiquidAlphaEnabled) => {
                self.liquid_alpha_enabled = other.liquid_alpha_enabled
            }
            Some(SelectiveMetagraphIndex::AlphaHigh) => self.alpha_high = other.alpha_high,
            Some(SelectiveMetagraphIndex::AlphaLow) => self.alpha_low = other.alpha_low,
            Some(SelectiveMetagraphIndex::BondsMovingAvg) => {
                self.bonds_moving_avg = other.bonds_moving_avg
            }
            Some(SelectiveMetagraphIndex::Hotkeys) => self.hotkeys = other.hotkeys.clone(),
            Some(SelectiveMetagraphIndex::Coldkeys) => self.coldkeys = other.coldkeys.clone(),
            Some(SelectiveMetagraphIndex::Identities) => self.identities = other.identities.clone(),
            Some(SelectiveMetagraphIndex::Axons) => self.axons = other.axons.clone(),
            Some(SelectiveMetagraphIndex::Active) => self.active = other.active.clone(),
            Some(SelectiveMetagraphIndex::ValidatorPermit) => {
                self.validator_permit = other.validator_permit.clone()
            }
            Some(SelectiveMetagraphIndex::PruningScore) => {
                self.pruning_score = other.pruning_score.clone()
            }
            Some(SelectiveMetagraphIndex::LastUpdate) => {
                self.last_update = other.last_update.clone()
            }
            Some(SelectiveMetagraphIndex::Emission) => self.emission = other.emission.clone(),
            Some(SelectiveMetagraphIndex::Dividends) => self.dividends = other.dividends.clone(),
            Some(SelectiveMetagraphIndex::Incentives) => self.incentives = other.incentives.clone(),
            Some(SelectiveMetagraphIndex::Consensus) => self.consensus = other.consensus.clone(),
            Some(SelectiveMetagraphIndex::Trust) => self.trust = other.trust.clone(),
            Some(SelectiveMetagraphIndex::Rank) => self.rank = other.rank.clone(),
            Some(SelectiveMetagraphIndex::BlockAtRegistration) => {
                self.block_at_registration = other.block_at_registration.clone()
            }
            Some(SelectiveMetagraphIndex::AlphaStake) => {
                self.alpha_stake = other.alpha_stake.clone()
            }
            Some(SelectiveMetagraphIndex::TaoStake) => self.tao_stake = other.tao_stake.clone(),
            Some(SelectiveMetagraphIndex::TotalStake) => {
                self.total_stake = other.total_stake.clone()
            }
            Some(SelectiveMetagraphIndex::TaoDividendsPerHotkey) => {
                self.tao_dividends_per_hotkey = other.tao_dividends_per_hotkey.clone()
            }
            Some(SelectiveMetagraphIndex::AlphaDividendsPerHotkey) => {
                self.alpha_dividends_per_hotkey = other.alpha_dividends_per_hotkey.clone()
            }
            Some(SelectiveMetagraphIndex::Validators) => self.validators = other.validators.clone(),
            Some(SelectiveMetagraphIndex::Commitments) => {
                self.commitments = other.commitments.clone()
            }
            None => {}
        };
    }
}

impl<AccountId> Default for SelectiveMetagraph<AccountId>
where
    AccountId: TypeInfo + Encode + Decode + Clone,
{
    fn default() -> Self {
        Self {
            netuid: NetUid::ROOT.into(),
            name: None,
            symbol: None,
            identity: None,
            network_registered_at: None,
            owner_hotkey: None,
            owner_coldkey: None,
            block: None,
            tempo: None,
            last_step: None,
            blocks_since_last_step: None,
            subnet_emission: None,
            alpha_in: None,
            alpha_out: None,
            tao_in: None,
            alpha_out_emission: None,
            alpha_in_emission: None,
            tao_in_emission: None,
            pending_alpha_emission: None,
            pending_root_emission: None,
            subnet_volume: None,
            moving_price: None,
            rho: None,
            kappa: None,
            min_allowed_weights: None,
            max_weights_limit: None,
            weights_version: None,
            weights_rate_limit: None,
            activity_cutoff: None,
            max_validators: None,
            num_uids: None,
            max_uids: None,
            burn: None,
            difficulty: None,
            registration_allowed: None,
            pow_registration_allowed: None,
            immunity_period: None,
            min_difficulty: None,
            max_difficulty: None,
            min_burn: None,
            max_burn: None,
            adjustment_alpha: None,
            adjustment_interval: None,
            target_regs_per_interval: None,
            max_regs_per_block: None,
            serving_rate_limit: None,
            commit_reveal_weights_enabled: None,
            commit_reveal_period: None,
            liquid_alpha_enabled: None,
            alpha_high: None,
            alpha_low: None,
            bonds_moving_avg: None,
            hotkeys: None,
            coldkeys: None,
            identities: None,
            axons: None,
            active: None,
            validator_permit: None,
            pruning_score: None,
            last_update: None,
            emission: None,
            dividends: None,
            incentives: None,
            consensus: None,
            trust: None,
            rank: None,
            block_at_registration: None,
            alpha_stake: None,
            tao_stake: None,
            total_stake: None,
            tao_dividends_per_hotkey: None,
            alpha_dividends_per_hotkey: None,
            validators: None,
            commitments: None,
        }
    }
}

pub enum SelectiveMetagraphIndex {
    Netuid,
    Name,
    Symbol,
    Identity,
    NetworkRegisteredAt,
    OwnerHotkey,
    OwnerColdkey,
    Block,
    Tempo,
    LastStep,
    BlocksSinceLastStep,
    SubnetEmission,
    AlphaIn,
    AlphaOut,
    TaoIn,
    AlphaOutEmission,
    AlphaInEmission,
    TaoInEmission,
    PendingAlphaEmission,
    PendingRootEmission,
    SubnetVolume,
    MovingPrice,
    Rho,
    Kappa,
    MinAllowedWeights,
    MaxWeightsLimit,
    WeightsVersion,
    WeightsRateLimit,
    ActivityCutoff,
    MaxValidators,
    NumUids,
    MaxUids,
    Burn,
    Difficulty,
    RegistrationAllowed,
    PowRegistrationAllowed,
    ImmunityPeriod,
    MinDifficulty,
    MaxDifficulty,
    MinBurn,
    MaxBurn,
    AdjustmentAlpha,
    AdjustmentInterval,
    TargetRegsPerInterval,
    MaxRegsPerBlock,
    ServingRateLimit,
    CommitRevealWeightsEnabled,
    CommitRevealPeriod,
    LiquidAlphaEnabled,
    AlphaHigh,
    AlphaLow,
    BondsMovingAvg,
    Hotkeys,
    Coldkeys,
    Identities,
    Axons,
    Active,
    ValidatorPermit,
    PruningScore,
    LastUpdate,
    Emission,
    Dividends,
    Incentives,
    Consensus,
    Trust,
    Rank,
    BlockAtRegistration,
    AlphaStake,
    TaoStake,
    TotalStake,
    TaoDividendsPerHotkey,
    AlphaDividendsPerHotkey,
    Validators,
    Commitments,
}

impl SelectiveMetagraphIndex {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(SelectiveMetagraphIndex::Netuid),
            1 => Some(SelectiveMetagraphIndex::Name),
            2 => Some(SelectiveMetagraphIndex::Symbol),
            3 => Some(SelectiveMetagraphIndex::Identity),
            4 => Some(SelectiveMetagraphIndex::NetworkRegisteredAt),
            5 => Some(SelectiveMetagraphIndex::OwnerHotkey),
            6 => Some(SelectiveMetagraphIndex::OwnerColdkey),
            7 => Some(SelectiveMetagraphIndex::Block),
            8 => Some(SelectiveMetagraphIndex::Tempo),
            9 => Some(SelectiveMetagraphIndex::LastStep),
            10 => Some(SelectiveMetagraphIndex::BlocksSinceLastStep),
            11 => Some(SelectiveMetagraphIndex::SubnetEmission),
            12 => Some(SelectiveMetagraphIndex::AlphaIn),
            13 => Some(SelectiveMetagraphIndex::AlphaOut),
            14 => Some(SelectiveMetagraphIndex::TaoIn),
            15 => Some(SelectiveMetagraphIndex::AlphaOutEmission),
            16 => Some(SelectiveMetagraphIndex::AlphaInEmission),
            17 => Some(SelectiveMetagraphIndex::TaoInEmission),
            18 => Some(SelectiveMetagraphIndex::PendingAlphaEmission),
            19 => Some(SelectiveMetagraphIndex::PendingRootEmission),
            20 => Some(SelectiveMetagraphIndex::SubnetVolume),
            21 => Some(SelectiveMetagraphIndex::MovingPrice),
            22 => Some(SelectiveMetagraphIndex::Rho),
            23 => Some(SelectiveMetagraphIndex::Kappa),
            24 => Some(SelectiveMetagraphIndex::MinAllowedWeights),
            25 => Some(SelectiveMetagraphIndex::MaxWeightsLimit),
            26 => Some(SelectiveMetagraphIndex::WeightsVersion),
            27 => Some(SelectiveMetagraphIndex::WeightsRateLimit),
            28 => Some(SelectiveMetagraphIndex::ActivityCutoff),
            29 => Some(SelectiveMetagraphIndex::MaxValidators),
            30 => Some(SelectiveMetagraphIndex::NumUids),
            31 => Some(SelectiveMetagraphIndex::MaxUids),
            32 => Some(SelectiveMetagraphIndex::Burn),
            33 => Some(SelectiveMetagraphIndex::Difficulty),
            34 => Some(SelectiveMetagraphIndex::RegistrationAllowed),
            35 => Some(SelectiveMetagraphIndex::PowRegistrationAllowed),
            36 => Some(SelectiveMetagraphIndex::ImmunityPeriod),
            37 => Some(SelectiveMetagraphIndex::MinDifficulty),
            38 => Some(SelectiveMetagraphIndex::MaxDifficulty),
            39 => Some(SelectiveMetagraphIndex::MinBurn),
            40 => Some(SelectiveMetagraphIndex::MaxBurn),
            41 => Some(SelectiveMetagraphIndex::AdjustmentAlpha),
            42 => Some(SelectiveMetagraphIndex::AdjustmentInterval),
            43 => Some(SelectiveMetagraphIndex::TargetRegsPerInterval),
            44 => Some(SelectiveMetagraphIndex::MaxRegsPerBlock),
            45 => Some(SelectiveMetagraphIndex::ServingRateLimit),
            46 => Some(SelectiveMetagraphIndex::CommitRevealWeightsEnabled),
            47 => Some(SelectiveMetagraphIndex::CommitRevealPeriod),
            48 => Some(SelectiveMetagraphIndex::LiquidAlphaEnabled),
            49 => Some(SelectiveMetagraphIndex::AlphaHigh),
            50 => Some(SelectiveMetagraphIndex::AlphaLow),
            51 => Some(SelectiveMetagraphIndex::BondsMovingAvg),
            52 => Some(SelectiveMetagraphIndex::Hotkeys),
            53 => Some(SelectiveMetagraphIndex::Coldkeys),
            54 => Some(SelectiveMetagraphIndex::Identities),
            55 => Some(SelectiveMetagraphIndex::Axons),
            56 => Some(SelectiveMetagraphIndex::Active),
            57 => Some(SelectiveMetagraphIndex::ValidatorPermit),
            58 => Some(SelectiveMetagraphIndex::PruningScore),
            59 => Some(SelectiveMetagraphIndex::LastUpdate),
            60 => Some(SelectiveMetagraphIndex::Emission),
            61 => Some(SelectiveMetagraphIndex::Dividends),
            62 => Some(SelectiveMetagraphIndex::Incentives),
            63 => Some(SelectiveMetagraphIndex::Consensus),
            64 => Some(SelectiveMetagraphIndex::Trust),
            65 => Some(SelectiveMetagraphIndex::Rank),
            66 => Some(SelectiveMetagraphIndex::BlockAtRegistration),
            67 => Some(SelectiveMetagraphIndex::AlphaStake),
            68 => Some(SelectiveMetagraphIndex::TaoStake),
            69 => Some(SelectiveMetagraphIndex::TotalStake),
            70 => Some(SelectiveMetagraphIndex::TaoDividendsPerHotkey),
            71 => Some(SelectiveMetagraphIndex::AlphaDividendsPerHotkey),
            72 => Some(SelectiveMetagraphIndex::Validators),
            73 => Some(SelectiveMetagraphIndex::Commitments),
            _ => None,
        }
    }
}
impl<T: Config> Pallet<T> {
    pub fn get_metagraph(netuid: NetUid) -> Option<Metagraph<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }

        let n: u16 = Self::get_subnetwork_n(netuid);
        let mut hotkeys: Vec<T::AccountId> = vec![];
        let mut coldkeys: Vec<T::AccountId> = vec![];
        let mut block_at_registration: Vec<Compact<u64>> = vec![];
        let mut identities: Vec<Option<ChainIdentityOfV2>> = vec![];
        let mut axons: Vec<AxonInfo> = vec![];
        for uid in 0..n {
            let hotkey = Keys::<T>::get(netuid, uid);
            let coldkey = Owner::<T>::get(hotkey.clone());
            hotkeys.push(hotkey.clone());
            coldkeys.push(coldkey.clone());
            block_at_registration.push(BlockAtRegistration::<T>::get(netuid, uid).into());
            identities.push(IdentitiesV2::<T>::get(coldkey.clone()));
            axons.push(Self::get_axon_info(netuid, &hotkey));
        }
        let mut tao_dividends_per_hotkey: Vec<(T::AccountId, Compact<TaoBalance>)> = vec![];
        let mut alpha_dividends_per_hotkey: Vec<(T::AccountId, Compact<AlphaBalance>)> = vec![];
        for hotkey in hotkeys.clone() {
            // Tao dividends were removed
            let tao_divs = TaoBalance::ZERO;
            let alpha_divs = AlphaDividendsPerSubnet::<T>::get(netuid, hotkey.clone());
            tao_dividends_per_hotkey.push((hotkey.clone(), tao_divs.into()));
            alpha_dividends_per_hotkey.push((hotkey.clone(), alpha_divs.into()));
        }
        let current_block: u64 = Pallet::<T>::get_current_block_as_u64();
        let last_step = LastMechansimStepBlock::<T>::get(netuid);
        let blocks_since_last_step: u64 = current_block.saturating_sub(last_step);
        let (total_stake_fl, alpha_stake_fl, tao_stake_fl): (
            Vec<I64F64>,
            Vec<I64F64>,
            Vec<I64F64>,
        ) = Self::get_stake_weights_for_network(netuid);

        let subnet_volume = SubnetVolume::<T>::get(netuid);
        Some(Metagraph {
            // Subnet index
            netuid: netuid.into(), // subnet index.

            // Name and symbol
            name: Self::get_name_for_subnet(netuid)
                .into_iter()
                .map(Compact)
                .collect(), // Name
            symbol: TokenSymbol::<T>::get(netuid)
                .into_iter()
                .map(Compact)
                .collect(), // Symbol.
            identity: SubnetIdentitiesV3::<T>::get(netuid), // identity information.
            network_registered_at: NetworkRegisteredAt::<T>::get(netuid).into(), // block at registration

            // Keys for owner.
            owner_hotkey: SubnetOwnerHotkey::<T>::get(netuid), // Owner hotkey
            owner_coldkey: SubnetOwner::<T>::get(netuid),      // Owner Coldkey

            // Tempo terms.
            block: current_block.into(),           // Block at call.
            tempo: Self::get_tempo(netuid).into(), // epoch tempo
            last_step: LastMechansimStepBlock::<T>::get(netuid).into(), // last epoch
            blocks_since_last_step: blocks_since_last_step.into(), // blocks since last epoch.

            // Subnet emission terms
            subnet_emission: 0.into(),                        // DEPRECATED
            alpha_in: SubnetAlphaIn::<T>::get(netuid).into(), // amount of alpha in reserve
            alpha_out: SubnetAlphaOut::<T>::get(netuid).into(), // amount of alpha outstanding
            tao_in: SubnetTAO::<T>::get(netuid).into(),       // amount of tao injected per block
            alpha_out_emission: SubnetAlphaOutEmission::<T>::get(netuid).into(), // amount injected in alpha reserves per block
            alpha_in_emission: SubnetAlphaInEmission::<T>::get(netuid).into(), // amount injected outstanding per block
            tao_in_emission: SubnetTaoInEmission::<T>::get(netuid).into(), // amount of tao injected per block
            pending_alpha_emission: PendingValidatorEmission::<T>::get(netuid)
                .saturating_add(PendingServerEmission::<T>::get(netuid))
                .into(), // pending alpha to be distributed
            pending_root_emission: TaoBalance::ZERO.into(), // panding tao for root divs to be distributed
            subnet_volume: subnet_volume.into(),
            moving_price: SubnetMovingPrice::<T>::get(netuid),

            // Hparams for epoch
            rho: Self::get_rho(netuid).into(), // subnet rho param
            kappa: Self::get_kappa(netuid).into(), // subnet kappa param

            // Validator params
            min_allowed_weights: Self::get_min_allowed_weights(netuid).into(), // min allowed weights per val
            max_weights_limit: Self::get_max_weight_limit(netuid).into(),      // max allowed weight
            weights_version: Self::get_weights_version_key(netuid).into(), // allowed weights version
            weights_rate_limit: Self::get_weights_set_rate_limit(netuid).into(), // rate limit on weights.
            activity_cutoff: Self::get_activity_cutoff_blocks(netuid).into(), // validator weights cut off period in blocks
            max_validators: Self::get_max_allowed_validators(netuid).into(), // max allowed validators.

            // Registration
            num_uids: Self::get_subnetwork_n(netuid).into(),
            max_uids: Self::get_max_allowed_uids(netuid).into(),
            registration_allowed: Self::get_network_registration_allowed(netuid), // allows registrations.
            pow_registration_allowed: Self::get_network_pow_registration_allowed(netuid), // allows pow registrations.
            difficulty: Self::get_difficulty_as_u64(netuid).into(), // current difficulty.
            burn: Self::get_burn(netuid).into(),
            immunity_period: Self::get_immunity_period(netuid).into(), // subnet miner immunity period
            min_difficulty: Self::get_min_difficulty(netuid).into(),   // min pow difficulty
            max_difficulty: Self::get_max_difficulty(netuid).into(),   // max pow difficulty
            min_burn: Self::get_min_burn(netuid).into(),               // min tao burn
            max_burn: Self::get_max_burn(netuid).into(),               // max tao burn
            adjustment_alpha: Self::get_adjustment_alpha(netuid).into(), // adjustment speed for registration params.
            adjustment_interval: Self::get_adjustment_interval(netuid).into(), // pow and burn adjustment interval
            target_regs_per_interval: Self::get_target_registrations_per_interval(netuid).into(), // target registrations per interval
            max_regs_per_block: Self::get_max_registrations_per_block(netuid).into(), // max registrations per block.
            serving_rate_limit: Self::get_serving_rate_limit(netuid).into(), // axon serving rate limit

            // CR
            commit_reveal_weights_enabled: Self::get_commit_reveal_weights_enabled(netuid), // Is CR enabled.
            commit_reveal_period: Self::get_reveal_period(netuid).into(), // Commit reveal interval

            // Bonds
            liquid_alpha_enabled: Self::get_liquid_alpha_enabled(netuid), // Bonds liquid enabled.
            alpha_high: Self::get_alpha_values(netuid).1.into(),          // Alpha param high
            alpha_low: Self::get_alpha_values(netuid).0.into(),           // Alpha param low
            bonds_moving_avg: Self::get_bonds_moving_average(netuid).into(), // Bonds moving avg

            // Metagraph info.
            hotkeys,  // hotkey per UID
            coldkeys, // coldkey per UID
            axons,    // Axon information per UID.
            identities,
            active: Active::<T>::get(netuid), // Active per UID
            validator_permit: ValidatorPermit::<T>::get(netuid), // Val permit per UID
            pruning_score: Vec::new(),        // Deprecated: no longer computed
            last_update: LastUpdate::<T>::get(NetUidStorageIndex::from(netuid))
                .into_iter()
                .map(Compact::from)
                .collect(), // Last update per UID
            emission: Emission::<T>::get(netuid)
                .into_iter()
                .map(Compact::from)
                .collect(), // Emission per UID
            dividends: Dividends::<T>::get(netuid)
                .into_iter()
                .map(Compact::from)
                .collect(), // Dividends per UID
            incentives: Incentive::<T>::get(NetUidStorageIndex::from(netuid))
                .into_iter()
                .map(Compact::from)
                .collect(), // Mining incentives per UID
            consensus: Consensus::<T>::get(netuid)
                .into_iter()
                .map(Compact::from)
                .collect(), // Consensus per UID
            trust: Vec::new(),                // Deprecated: no longer computed
            rank: Vec::new(),                 // Deprecated: no longer computed
            block_at_registration,            // Reg block per UID
            alpha_stake: alpha_stake_fl
                .iter()
                .map(|xi| Compact::from(AlphaBalance::from(fixed64_to_u64(*xi))))
                .collect::<Vec<Compact<AlphaBalance>>>(), // Alpha staked per UID
            tao_stake: tao_stake_fl
                .iter()
                .map(|xi| Compact::from(TaoBalance::from(fixed64_to_u64(*xi))))
                .collect::<Vec<Compact<TaoBalance>>>(), // TAO staked per UID
            total_stake: total_stake_fl
                .iter()
                .map(|xi| Compact::from(TaoBalance::from(fixed64_to_u64(*xi))))
                .collect::<Vec<Compact<TaoBalance>>>(), // Total stake per UID

            // Dividend break down.
            tao_dividends_per_hotkey,
            alpha_dividends_per_hotkey,
        })
    }
    pub fn get_all_metagraphs() -> Vec<Option<Metagraph<T::AccountId>>> {
        let netuids = Self::get_all_subnet_netuids();
        let mut metagraphs = Vec::<Option<Metagraph<T::AccountId>>>::new();
        for netuid in netuids.clone().iter() {
            metagraphs.push(Self::get_metagraph(*netuid));
        }
        metagraphs
    }

    pub fn get_mechagraph(netuid: NetUid, mecid: MechId) -> Option<Metagraph<T::AccountId>> {
        if Self::ensure_mechanism_exists(netuid, mecid).is_err() {
            return None;
        }

        // Get netuid metagraph
        let maybe_meta = Self::get_metagraph(netuid);
        if let Some(mut meta) = maybe_meta {
            let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

            // Update with mechanism information
            meta.netuid = NetUid::from(u16::from(netuid_index)).into();
            meta.last_update = LastUpdate::<T>::get(netuid_index)
                .into_iter()
                .map(Compact::from)
                .collect();
            meta.incentives = Incentive::<T>::get(netuid_index)
                .into_iter()
                .map(Compact::from)
                .collect();

            Some(meta)
        } else {
            None
        }
    }

    pub fn get_all_mechagraphs() -> Vec<Option<Metagraph<T::AccountId>>> {
        let netuids = Self::get_all_subnet_netuids();
        let mut metagraphs = Vec::<Option<Metagraph<T::AccountId>>>::new();
        for netuid in netuids.clone().iter() {
            let mechanism_count = u8::from(MechanismCountCurrent::<T>::get(netuid));
            for mecid in 0..mechanism_count {
                metagraphs.push(Self::get_mechagraph(*netuid, MechId::from(mecid)));
            }
        }
        metagraphs
    }

    pub fn get_selective_metagraph(
        netuid: NetUid,
        metagraph_indexes: Vec<u16>,
    ) -> Option<SelectiveMetagraph<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            None
        } else {
            let mut result = SelectiveMetagraph::default();
            for index in metagraph_indexes.iter() {
                let value = Self::get_single_selective_metagraph(netuid, *index);
                result.merge_value(&value, *index as usize);
            }
            Some(result)
        }
    }

    pub fn get_selective_mechagraph(
        netuid: NetUid,
        mecid: MechId,
        metagraph_indexes: Vec<u16>,
    ) -> Option<SelectiveMetagraph<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            None
        } else {
            let mut result = SelectiveMetagraph::default();

            for index in metagraph_indexes.iter() {
                let value = Self::get_single_selective_mechagraph(netuid, mecid, *index);
                result.merge_value(&value, *index as usize);
            }
            // always include netuid even the metagraph_indexes doesn't contain it
            result.netuid = netuid.into();

            Some(result)
        }
    }

    fn get_single_selective_metagraph(
        netuid: NetUid,
        metagraph_index: u16,
    ) -> SelectiveMetagraph<T::AccountId> {
        match SelectiveMetagraphIndex::from_index(metagraph_index as usize) {
            // Name and symbol
            Some(SelectiveMetagraphIndex::Netuid) => SelectiveMetagraph {
                netuid: netuid.into(),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Name) => SelectiveMetagraph {
                netuid: netuid.into(),
                name: Some(
                    Self::get_name_for_subnet(netuid)
                        .into_iter()
                        .map(Compact)
                        .collect(),
                ),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Symbol) => SelectiveMetagraph {
                netuid: netuid.into(),
                symbol: Some(
                    TokenSymbol::<T>::get(netuid)
                        .into_iter()
                        .map(Compact)
                        .collect(),
                ),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Identity) => SelectiveMetagraph {
                netuid: netuid.into(),
                identity: Some(SubnetIdentitiesV3::<T>::get(netuid)),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::NetworkRegisteredAt) => SelectiveMetagraph {
                netuid: netuid.into(),
                network_registered_at: Some(NetworkRegisteredAt::<T>::get(netuid).into()),
                ..Default::default()
            },

            // Keys for owner.
            Some(SelectiveMetagraphIndex::OwnerHotkey) => SelectiveMetagraph {
                netuid: netuid.into(),
                owner_hotkey: Some(SubnetOwnerHotkey::<T>::get(netuid)),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::OwnerColdkey) => SelectiveMetagraph {
                netuid: netuid.into(),
                owner_coldkey: Some(SubnetOwner::<T>::get(netuid)),
                ..Default::default()
            },

            // Tempo terms.
            Some(SelectiveMetagraphIndex::Block) => SelectiveMetagraph {
                netuid: netuid.into(),
                block: Some(Pallet::<T>::get_current_block_as_u64().into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Tempo) => SelectiveMetagraph {
                netuid: netuid.into(),
                tempo: Some(Self::get_tempo(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::LastStep) => SelectiveMetagraph {
                netuid: netuid.into(),
                last_step: Some(LastMechansimStepBlock::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::BlocksSinceLastStep) => {
                let current_block: u64 = Pallet::<T>::get_current_block_as_u64();
                let last_step = LastMechansimStepBlock::<T>::get(netuid);
                let blocks_since_last_step: u64 = current_block.saturating_sub(last_step);
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    blocks_since_last_step: Some(blocks_since_last_step.into()),
                    ..Default::default()
                }
            }

            // Subnet emission terms
            Some(SelectiveMetagraphIndex::SubnetEmission) => SelectiveMetagraph {
                netuid: netuid.into(),
                subnet_emission: Some(0.into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AlphaIn) => SelectiveMetagraph {
                netuid: netuid.into(),
                alpha_in: Some(SubnetAlphaIn::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AlphaOut) => SelectiveMetagraph {
                netuid: netuid.into(),
                alpha_out: Some(SubnetAlphaOut::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::TaoIn) => SelectiveMetagraph {
                netuid: netuid.into(),
                tao_in: Some(SubnetTAO::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AlphaOutEmission) => SelectiveMetagraph {
                netuid: netuid.into(),
                alpha_out_emission: Some(SubnetAlphaOutEmission::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AlphaInEmission) => SelectiveMetagraph {
                netuid: netuid.into(),
                alpha_in_emission: Some(SubnetAlphaInEmission::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::TaoInEmission) => SelectiveMetagraph {
                netuid: netuid.into(),
                tao_in_emission: Some(SubnetTaoInEmission::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::PendingAlphaEmission) => SelectiveMetagraph {
                netuid: netuid.into(),
                pending_alpha_emission: Some(
                    PendingValidatorEmission::<T>::get(netuid)
                        .saturating_add(PendingServerEmission::<T>::get(netuid))
                        .into(),
                ),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::PendingRootEmission) => SelectiveMetagraph {
                netuid: netuid.into(),
                pending_root_emission: Some(TaoBalance::from(0u64).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::SubnetVolume) => SelectiveMetagraph {
                netuid: netuid.into(),
                subnet_volume: Some(SubnetVolume::<T>::get(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MovingPrice) => SelectiveMetagraph {
                netuid: netuid.into(),
                moving_price: Some(SubnetMovingPrice::<T>::get(netuid)),
                ..Default::default()
            },

            // Hparams for epoch
            Some(SelectiveMetagraphIndex::Rho) => SelectiveMetagraph {
                netuid: netuid.into(),
                rho: Some(Self::get_rho(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Kappa) => SelectiveMetagraph {
                netuid: netuid.into(),
                kappa: Some(Self::get_kappa(netuid).into()),
                ..Default::default()
            },

            // Validator params
            Some(SelectiveMetagraphIndex::MinAllowedWeights) => SelectiveMetagraph {
                netuid: netuid.into(),
                min_allowed_weights: Some(Self::get_min_allowed_weights(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MaxWeightsLimit) => SelectiveMetagraph {
                netuid: netuid.into(),
                max_weights_limit: Some(Self::get_max_weight_limit(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::WeightsVersion) => SelectiveMetagraph {
                netuid: netuid.into(),
                weights_version: Some(Self::get_weights_version_key(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::WeightsRateLimit) => SelectiveMetagraph {
                netuid: netuid.into(),
                weights_rate_limit: Some(Self::get_weights_set_rate_limit(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::ActivityCutoff) => SelectiveMetagraph {
                netuid: netuid.into(),
                activity_cutoff: Some(Self::get_activity_cutoff_blocks(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MaxValidators) => SelectiveMetagraph {
                netuid: netuid.into(),
                max_validators: Some(Self::get_max_allowed_validators(netuid).into()),
                ..Default::default()
            },

            // Registration
            Some(SelectiveMetagraphIndex::NumUids) => SelectiveMetagraph {
                netuid: netuid.into(),
                num_uids: Some(Self::get_subnetwork_n(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MaxUids) => SelectiveMetagraph {
                netuid: netuid.into(),
                max_uids: Some(Self::get_max_allowed_uids(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::RegistrationAllowed) => SelectiveMetagraph {
                netuid: netuid.into(),
                registration_allowed: Some(Self::get_network_registration_allowed(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::PowRegistrationAllowed) => SelectiveMetagraph {
                netuid: netuid.into(),
                pow_registration_allowed: Some(
                    Self::get_network_pow_registration_allowed(netuid).into(),
                ),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Difficulty) => SelectiveMetagraph {
                netuid: netuid.into(),
                difficulty: Some(Self::get_difficulty_as_u64(netuid).into()),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::Burn) => SelectiveMetagraph {
                netuid: netuid.into(),
                burn: Some(Self::get_burn(netuid).into()),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::ImmunityPeriod) => SelectiveMetagraph {
                netuid: netuid.into(),
                immunity_period: Some(Self::get_immunity_period(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MinDifficulty) => SelectiveMetagraph {
                netuid: netuid.into(),
                min_difficulty: Some(Self::get_min_difficulty(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MaxDifficulty) => SelectiveMetagraph {
                netuid: netuid.into(),
                max_difficulty: Some(Self::get_max_difficulty(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MinBurn) => SelectiveMetagraph {
                netuid: netuid.into(),
                min_burn: Some(Self::get_min_burn(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MaxBurn) => SelectiveMetagraph {
                netuid: netuid.into(),
                max_burn: Some(Self::get_max_burn(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AdjustmentAlpha) => SelectiveMetagraph {
                netuid: netuid.into(),
                adjustment_alpha: Some(Self::get_adjustment_alpha(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AdjustmentInterval) => SelectiveMetagraph {
                netuid: netuid.into(),
                adjustment_interval: Some(Self::get_adjustment_interval(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::TargetRegsPerInterval) => SelectiveMetagraph {
                netuid: netuid.into(),
                target_regs_per_interval: Some(
                    Self::get_target_registrations_per_interval(netuid).into(),
                ),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::MaxRegsPerBlock) => SelectiveMetagraph {
                netuid: netuid.into(),
                max_regs_per_block: Some(Self::get_max_registrations_per_block(netuid).into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::ServingRateLimit) => SelectiveMetagraph {
                netuid: netuid.into(),
                serving_rate_limit: Some(Self::get_serving_rate_limit(netuid).into()),
                ..Default::default()
            },

            // CR
            Some(SelectiveMetagraphIndex::CommitRevealWeightsEnabled) => SelectiveMetagraph {
                netuid: netuid.into(),
                commit_reveal_weights_enabled: Some(Self::get_commit_reveal_weights_enabled(
                    netuid,
                )),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::CommitRevealPeriod) => SelectiveMetagraph {
                netuid: netuid.into(),
                commit_reveal_period: Some(Self::get_reveal_period(netuid).into()),
                ..Default::default()
            },

            // Bonds
            Some(SelectiveMetagraphIndex::LiquidAlphaEnabled) => SelectiveMetagraph {
                netuid: netuid.into(),
                liquid_alpha_enabled: Some(Self::get_liquid_alpha_enabled(netuid)),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AlphaHigh) => SelectiveMetagraph {
                netuid: netuid.into(),
                alpha_high: Some(Self::get_alpha_values(netuid).1.into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::AlphaLow) => SelectiveMetagraph {
                netuid: netuid.into(),
                alpha_low: Some(Self::get_alpha_values(netuid).0.into()),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::BondsMovingAvg) => SelectiveMetagraph {
                netuid: netuid.into(),
                bonds_moving_avg: Some(Self::get_bonds_moving_average(netuid).into()),
                ..Default::default()
            },

            // Metagraph info.
            Some(SelectiveMetagraphIndex::Hotkeys) => {
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut hotkeys: Vec<T::AccountId> = vec![];
                for uid in 0..n {
                    let hotkey = Keys::<T>::get(netuid, uid);
                    hotkeys.push(hotkey.clone());
                }

                SelectiveMetagraph {
                    netuid: netuid.into(),
                    hotkeys: Some(hotkeys),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::Coldkeys) => {
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut coldkeys: Vec<T::AccountId> = vec![];
                for uid in 0..n {
                    let hotkey = Keys::<T>::get(netuid, uid);
                    let coldkey = Owner::<T>::get(hotkey.clone());
                    coldkeys.push(coldkey.clone());
                }
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    coldkeys: Some(coldkeys),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::Identities) => {
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut identities: Vec<Option<ChainIdentityOfV2>> = vec![];
                for uid in 0..n {
                    let hotkey = Keys::<T>::get(netuid, uid);
                    let coldkey = Owner::<T>::get(hotkey.clone());
                    identities.push(IdentitiesV2::<T>::get(coldkey.clone()));
                }
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    identities: Some(identities),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::Axons) => {
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut axons: Vec<AxonInfo> = vec![];
                for uid in 0..n {
                    let hotkey = Keys::<T>::get(netuid, uid);
                    axons.push(Self::get_axon_info(netuid, &hotkey));
                }
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    axons: Some(axons),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::Active) => SelectiveMetagraph {
                netuid: netuid.into(),
                active: Some(Active::<T>::get(netuid)),
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::ValidatorPermit) => SelectiveMetagraph {
                netuid: netuid.into(),
                validator_permit: Some(ValidatorPermit::<T>::get(netuid)),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::PruningScore) => SelectiveMetagraph {
                netuid: netuid.into(),
                pruning_score: Some(Vec::new()), // Deprecated: no longer computed
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::LastUpdate) => SelectiveMetagraph {
                netuid: netuid.into(),
                last_update: Some(
                    LastUpdate::<T>::get(NetUidStorageIndex::from(netuid))
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::Emission) => SelectiveMetagraph {
                netuid: netuid.into(),
                emission: Some(
                    Emission::<T>::get(netuid)
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::Dividends) => SelectiveMetagraph {
                netuid: netuid.into(),
                dividends: Some(
                    Dividends::<T>::get(netuid)
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::Incentives) => SelectiveMetagraph {
                netuid: netuid.into(),
                incentives: Some(
                    Incentive::<T>::get(NetUidStorageIndex::from(netuid))
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::Consensus) => SelectiveMetagraph {
                netuid: netuid.into(),
                consensus: Some(
                    Consensus::<T>::get(netuid)
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::Trust) => SelectiveMetagraph {
                netuid: netuid.into(),
                trust: Some(Vec::new()), // Deprecated: no longer computed
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::Rank) => SelectiveMetagraph {
                netuid: netuid.into(),
                rank: Some(Vec::new()), // Deprecated: no longer computed
                ..Default::default()
            },
            Some(SelectiveMetagraphIndex::BlockAtRegistration) => {
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut block_at_registration: Vec<Compact<u64>> = vec![];
                for uid in 0..n {
                    block_at_registration.push(BlockAtRegistration::<T>::get(netuid, uid).into());
                }
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    block_at_registration: Some(block_at_registration),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::AlphaStake) => {
                let (_, alpha_stake_fl, _): (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) =
                    Self::get_stake_weights_for_network(netuid);
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    alpha_stake: Some(
                        alpha_stake_fl
                            .iter()
                            .map(|xi| Compact::from(AlphaBalance::from(fixed64_to_u64(*xi))))
                            .collect::<Vec<Compact<AlphaBalance>>>(),
                    ),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::TaoStake) => {
                let (_, _, tao_stake_fl): (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) =
                    Self::get_stake_weights_for_network(netuid);
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    tao_stake: Some(
                        tao_stake_fl
                            .iter()
                            .map(|xi| Compact::from(TaoBalance::from(fixed64_to_u64(*xi))))
                            .collect::<Vec<Compact<TaoBalance>>>(),
                    ),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::TotalStake) => {
                let (total_stake_fl, _, _): (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) =
                    Self::get_stake_weights_for_network(netuid);
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    total_stake: Some(
                        total_stake_fl
                            .iter()
                            .map(|xi| Compact::from(TaoBalance::from(fixed64_to_u64(*xi))))
                            .collect::<Vec<Compact<TaoBalance>>>(),
                    ),
                    ..Default::default()
                }
            }

            // Dividend break down.
            Some(SelectiveMetagraphIndex::TaoDividendsPerHotkey) => {
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut hotkeys: Vec<T::AccountId> = vec![];
                for uid in 0..n {
                    let hotkey = Keys::<T>::get(netuid, uid);
                    hotkeys.push(hotkey.clone());
                }
                let mut tao_dividends_per_hotkey: Vec<(T::AccountId, Compact<TaoBalance>)> = vec![];
                for hotkey in hotkeys.clone() {
                    // Tao dividends were removed
                    let tao_divs = TaoBalance::ZERO;
                    tao_dividends_per_hotkey.push((hotkey.clone(), tao_divs.into()));
                }
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    tao_dividends_per_hotkey: Some(tao_dividends_per_hotkey),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::AlphaDividendsPerHotkey) => {
                let mut alpha_dividends_per_hotkey: Vec<(T::AccountId, Compact<AlphaBalance>)> =
                    vec![];
                let n: u16 = Self::get_subnetwork_n(netuid);
                let mut hotkeys: Vec<T::AccountId> = vec![];

                for uid in 0..n {
                    let hotkey = Keys::<T>::get(netuid, uid);
                    hotkeys.push(hotkey.clone());
                }

                for hotkey in hotkeys.clone() {
                    let alpha_divs = AlphaDividendsPerSubnet::<T>::get(netuid, hotkey.clone());
                    alpha_dividends_per_hotkey.push((hotkey.clone(), alpha_divs.into()));
                }
                SelectiveMetagraph {
                    netuid: netuid.into(),
                    alpha_dividends_per_hotkey: Some(alpha_dividends_per_hotkey),
                    ..Default::default()
                }
            }
            Some(SelectiveMetagraphIndex::Validators) => Self::get_validators(netuid),
            Some(SelectiveMetagraphIndex::Commitments) => Self::get_commitments(netuid),
            None => SelectiveMetagraph {
                // Subnet index
                netuid: netuid.into(),
                ..Default::default()
            },
        }
    }

    fn get_single_selective_mechagraph(
        netuid: NetUid,
        mecid: MechId,
        metagraph_index: u16,
    ) -> SelectiveMetagraph<T::AccountId> {
        let netuid_index = Self::get_mechanism_storage_index(netuid, mecid);

        // Default to netuid, replace as needed for mecid
        match SelectiveMetagraphIndex::from_index(metagraph_index as usize) {
            Some(SelectiveMetagraphIndex::Incentives) => SelectiveMetagraph {
                netuid: netuid.into(),
                incentives: Some(
                    Incentive::<T>::get(netuid_index)
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            Some(SelectiveMetagraphIndex::LastUpdate) => SelectiveMetagraph {
                netuid: netuid.into(),
                last_update: Some(
                    LastUpdate::<T>::get(netuid_index)
                        .into_iter()
                        .map(Compact::from)
                        .collect(),
                ),
                ..Default::default()
            },

            _ => {
                let mut meta = Self::get_single_selective_metagraph(netuid, metagraph_index);
                // Replace netuid with index
                meta.netuid = NetUid::from(u16::from(netuid_index)).into();
                meta
            }
        }
    }

    fn get_validators(netuid: NetUid) -> SelectiveMetagraph<T::AccountId> {
        let stake_threshold = Self::get_stake_threshold();
        let hotkeys: Vec<(u16, T::AccountId)> =
            <Keys<T> as IterableStorageDoubleMap<NetUid, u16, T::AccountId>>::iter_prefix(netuid)
                .collect();
        let validator_permits: Vec<bool> = Self::get_validator_permit(netuid);

        // filter according to validator_permits
        let hotkeys: Vec<&(u16, T::AccountId)> = hotkeys
            .iter()
            .filter(|(uid, _)| *validator_permits.get(*uid as usize).unwrap_or(&false))
            .collect::<Vec<_>>();

        // map hotkeys to validators with stake
        let mut validators: Vec<(u16, I64F64)> = hotkeys
            .iter()
            .map(|(uid, hotkey)| {
                let stake = Self::get_stake_weights_for_hotkey_on_subnet(hotkey, netuid);
                (*uid, stake.0)
            })
            .collect();

        // sort validators by stake
        validators.sort_by(|a, b| a.1.cmp(&b.1));

        let validators: Vec<Compact<u16>> = validators
            .iter()
            .filter(|(_uid, stake)| *stake > stake_threshold)
            .map(|(uid, _)| Compact::from(*uid))
            .collect::<Vec<_>>();

        SelectiveMetagraph {
            // Subnet index
            netuid: netuid.into(),
            validators: Some(validators),
            ..Default::default()
        }
    }

    fn get_commitments(netuid: NetUid) -> SelectiveMetagraph<T::AccountId> {
        let commitments = <T as Config>::GetCommitments::get_commitments(netuid);
        let commitments: Vec<(T::AccountId, Vec<Compact<u8>>)> = commitments
            .iter()
            .map(|(account, commitment)| {
                let compact_commitment = commitment
                    .iter()
                    .map(|c| Compact::from(*c))
                    .collect::<Vec<Compact<u8>>>();
                (account.clone(), compact_commitment)
            })
            .collect();

        SelectiveMetagraph {
            commitments: Some(commitments),
            ..Default::default()
        }
    }
}

#[test]
fn test_selective_metagraph() {
    let mut metagraph = SelectiveMetagraph::<u32>::default();
    let expected = SelectiveMetagraph::<u32> {
        netuid: NetUid::ROOT.into(),
        name: None,
        symbol: None,
        identity: None,
        network_registered_at: None,
        owner_hotkey: None,
        owner_coldkey: None,
        block: None,
        tempo: None,
        last_step: None,
        blocks_since_last_step: None,
        subnet_emission: None,
        alpha_in: None,
        alpha_out: None,
        tao_in: None,
        alpha_out_emission: None,
        alpha_in_emission: None,
        tao_in_emission: None,
        pending_alpha_emission: None,
        pending_root_emission: None,
        subnet_volume: None,
        moving_price: None,
        rho: None,
        kappa: None,
        min_allowed_weights: None,
        max_weights_limit: None,
        weights_version: None,
        weights_rate_limit: None,
        activity_cutoff: None,
        max_validators: None,
        num_uids: None,
        max_uids: None,
        burn: None,
        difficulty: None,
        registration_allowed: None,
        pow_registration_allowed: None,
        immunity_period: None,
        min_difficulty: None,
        max_difficulty: None,
        min_burn: None,
        max_burn: None,
        adjustment_alpha: None,
        adjustment_interval: None,
        target_regs_per_interval: None,
        max_regs_per_block: None,
        serving_rate_limit: None,
        commit_reveal_weights_enabled: None,
        commit_reveal_period: None,
        liquid_alpha_enabled: None,
        alpha_high: None,
        alpha_low: None,
        bonds_moving_avg: None,
        hotkeys: None,
        coldkeys: None,
        identities: None,
        axons: None,
        active: None,
        validator_permit: None,
        pruning_score: None,
        last_update: None,
        emission: None,
        dividends: None,
        incentives: None,
        consensus: None,
        trust: None,
        rank: None,
        block_at_registration: None,
        alpha_stake: None,
        tao_stake: None,
        total_stake: None,
        tao_dividends_per_hotkey: None,
        alpha_dividends_per_hotkey: None,
        validators: None,
        commitments: None,
    };

    // test init value
    assert_eq!(metagraph, expected);

    let wrong_index: usize = 100;
    let metagraph_name = SelectiveMetagraph::<u32> {
        netuid: NetUid::ROOT.into(),
        name: Some(vec![1_u8].into_iter().map(Compact).collect()),
        ..Default::default()
    };

    // test merge function
    metagraph.merge_value(&metagraph_name, wrong_index);
    assert!(metagraph.name.is_none());

    let name_index: usize = 1;
    metagraph.merge_value(&metagraph_name, name_index);
    assert!(metagraph.name.is_some());

    let alph_low_index: usize = 50;
    let metagraph_alpha_low = SelectiveMetagraph::<u32> {
        netuid: NetUid::ROOT.into(),
        alpha_low: Some(0_u16.into()),
        ..Default::default()
    };
    assert!(metagraph.alpha_low.is_none());
    metagraph.merge_value(&metagraph_alpha_low, alph_low_index);
    assert!(metagraph.alpha_low.is_some());
}
