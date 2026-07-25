#![allow(clippy::crate_in_macro_def)]

use frame_support::pallet_macros::pallet_section;

/// [`pallet_section`] defining [`Config`] for the subtensor pallet (imported via [`import_section`]).
///
/// Associated type **names** are wired through the runtime; prefer definition-site docs over renames.
/// `#[pallet::constant]` items seed genesis / defaults (often mirrored by storage).
#[pallet_section]
mod config {

    use crate::{CommitmentsInterface, GetAlphaForTao, GetTaoForAlpha};
    use frame_support::PalletId;
    use frame_support::traits::LockableCurrency;
    use pallet_alpha_assets::AlphaAssetsInterface;
    use pallet_commitments::GetCommitments;
    use subtensor_runtime_common::AuthorshipInfo;
    use subtensor_swap_interface::{SwapEngine, SwapHandler};

    /// Runtime dependencies for SubtensorModule: currency, swap, commitments, scheduling, and genesis constants.
    ///
    /// Implemented by the node runtime; associated types and constants below are the wiring surface
    /// agents should search when tracing a Config bound or an `Initial*` default.
    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + pallet_drand::Config
        + pallet_crowdloan::Config
        + pallet_scheduler::Config
    {
        /// Runtime call type that can encode SubtensorModule calls (used by scheduler / proxy paths).
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = OriginFor<Self>>
            + From<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>
            + From<frame_system::Call<Self>>;

        /// Call type that may be dispatched without origin filters (sudo / privileged schedules).
        type SudoRuntimeCall: Parameter
            + UnfilteredDispatchable<RuntimeOrigin = OriginFor<Self>>
            + GetDispatchInfo;

        /// Fungible TAO currency used for neuron deposits, locks, and transfers (`TaoBalance` units = rao).
        type Currency: fungible::Balanced<Self::AccountId, Balance = TaoBalance>
            + fungible::Mutate<Self::AccountId>
            + LockableCurrency<Self::AccountId, Balance = TaoBalance>;

        /// Anonymous scheduler used for delayed dissolve / coldkey-swap style call dispatch.
        type Scheduler: ScheduleAnon<
                BlockNumberFor<Self>,
                LocalCallOf<Self>,
                PalletsOriginOf<Self>,
                Hasher = Self::Hashing,
            >;

        /// Preimage store for scheduled call payloads (hash lookup + store).
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// TAO↔alpha AMM: implements `SwapHandler` plus both directional `SwapEngine` adapters.
        type SwapInterface: SwapHandler
            + SwapEngine<GetAlphaForTao<Self>>
            + SwapEngine<GetTaoForAlpha<Self>>;

        /// Proxy pallet bridge for filtered proxy-call dispatch into subtensor.
        type ProxyInterface: crate::ProxyInterface<Self::AccountId>;

        /// Read path for on-chain commitments (weights / mechanism commit-reveal).
        type GetCommitments: GetCommitments<Self::AccountId>;

        /// Purge commitments when a subnet is dissolved.
        type CommitmentsInterface: CommitmentsInterface;

        /// Mint, burn, and recycle subnet alpha via the alpha-assets pallet.
        type AlphaAssets: AlphaAssetsInterface;

        /// Minimum blocks between EVM key associations for the same coldkey.
        type EvmKeyAssociateRateLimit: Get<u64>;

        /// Current block author account (used for authorship-gated rewards / accounting).
        type AuthorshipProvider: AuthorshipInfo<Self::AccountId>;

        /// Extrinsic weight implementations; method names must match call names (Tier D).
        type WeightInfo: crate::weights::WeightInfo;

        // Initial Value Constants

        /// Genesis default for total TAO issuance seed, in rao.
        #[pallet::constant]
        type InitialIssuance: Get<TaoBalance>;
        /// Genesis default for per-subnet minimum non-zero weight count.
        #[pallet::constant]
        type InitialMinAllowedWeights: Get<u16>;
        /// Genesis default emission-share parameter (u16 fixed-point used by early emission math).
        #[pallet::constant]
        type InitialEmissionValue: Get<u16>;
        /// Genesis default tempo (blocks per epoch) for new subnets.
        #[pallet::constant]
        type InitialTempo: Get<u16>;
        /// Genesis default PoW registration difficulty.
        #[pallet::constant]
        type InitialDifficulty: Get<u64>;
        /// Genesis default upper bound for adaptive PoW difficulty.
        #[pallet::constant]
        type InitialMaxDifficulty: Get<u64>;
        /// Genesis default lower bound for adaptive PoW difficulty.
        #[pallet::constant]
        type InitialMinDifficulty: Get<u64>;
        /// Genesis default RAO recycled into the network on registration, in rao.
        #[pallet::constant]
        type InitialRAORecycledForRegistration: Get<TaoBalance>;
        /// Genesis default registration burn cost, in rao.
        #[pallet::constant]
        type InitialBurn: Get<TaoBalance>;
        /// Genesis default upper bound for adaptive registration burn, in rao.
        #[pallet::constant]
        type InitialMaxBurn: Get<TaoBalance>;
        /// Genesis default lower bound for adaptive registration burn, in rao.
        #[pallet::constant]
        type InitialMinBurn: Get<TaoBalance>;
        /// Genesis default minimum stake required for weight-setting eligibility, in rao.
        #[pallet::constant]
        type InitialMinStake: Get<TaoBalance>;
        /// Genesis default minimum stake transfer / move amount, in rao.
        #[pallet::constant]
        type InitialMinTransfer: Get<TaoBalance>;
        /// Hard upper bound owners may set for min burn, in rao.
        #[pallet::constant]
        type MinBurnUpperBound: Get<TaoBalance>;
        /// Hard lower bound owners may set for max burn, in rao.
        #[pallet::constant]
        type MaxBurnLowerBound: Get<TaoBalance>;
        /// Hard lower bound for owner-set tempo (blocks per epoch).
        #[pallet::constant]
        type MinTempo: Get<u16>;
        /// Hard upper bound for owner-set tempo (blocks per epoch).
        #[pallet::constant]
        type MaxTempo: Get<u16>;
        /// Hard lower bound for activity-cutoff factor, in per-mille (‰).
        #[pallet::constant]
        type MinActivityCutoffFactorMilli: Get<u32>;
        /// Hard upper bound for activity-cutoff factor, in per-mille (‰).
        #[pallet::constant]
        type MaxActivityCutoffFactorMilli: Get<u32>;
        /// Genesis default difficulty/burn adjustment interval, in blocks.
        #[pallet::constant]
        type InitialAdjustmentInterval: Get<u16>;
        /// Genesis default bonds EMA moving-average parameter.
        #[pallet::constant]
        type InitialBondsMovingAverage: Get<u64>;
        /// Genesis default bonds penalty applied during consensus.
        #[pallet::constant]
        type InitialBondsPenalty: Get<u16>;
        /// Genesis default for whether bonds reset each epoch.
        #[pallet::constant]
        type InitialBondsResetOn: Get<bool>;
        /// Genesis default target registrations per adjustment interval.
        #[pallet::constant]
        type InitialTargetRegistrationsPerInterval: Get<u16>;
        /// Genesis default Yuma consensus `rho` constant.
        #[pallet::constant]
        type InitialRho: Get<u16>;
        /// Genesis default steepness for the alpha sigmoid in consensus.
        #[pallet::constant]
        type InitialAlphaSigmoidSteepness: Get<i16>;
        /// Genesis default Yuma consensus `kappa` constant.
        #[pallet::constant]
        type InitialKappa: Get<u16>;
        /// Genesis default minimum allowed UIDs on a subnet.
        #[pallet::constant]
        type InitialMinAllowedUids: Get<u16>;
        /// Genesis default maximum allowed UIDs on a subnet.
        #[pallet::constant]
        type InitialMaxAllowedUids: Get<u16>;
        /// Genesis default validator context pruning length (blocks / epochs retained).
        #[pallet::constant]
        type InitialValidatorPruneLen: Get<u64>;
        /// Genesis default scaling-law power for emission distribution.
        #[pallet::constant]
        type InitialScalingLawPower: Get<u16>;
        /// Genesis default neuron immunity period, in blocks.
        #[pallet::constant]
        type InitialImmunityPeriod: Get<u16>;
        /// Genesis default activity cutoff, in blocks (pruning inactivity window).
        #[pallet::constant]
        type InitialActivityCutoff: Get<u16>;
        /// Genesis default per-block registration cap per subnet.
        #[pallet::constant]
        type InitialMaxRegistrationsPerBlock: Get<u16>;
        /// Genesis default pruning score assigned to new neurons.
        #[pallet::constant]
        type InitialPruningScore: Get<u16>;
        /// Genesis default maximum validators allowed per subnet.
        #[pallet::constant]
        type InitialMaxAllowedValidators: Get<u16>;
        /// Genesis default (max) validator delegate take as u16 (`PerU16` scale).
        #[pallet::constant]
        type InitialDefaultDelegateTake: Get<u16>;
        /// Genesis default minimum validator delegate take as u16 (`PerU16` scale).
        #[pallet::constant]
        type InitialMinDelegateTake: Get<u16>;
        /// Genesis default (max) childkey take as u16 (`PerU16` scale).
        #[pallet::constant]
        type InitialDefaultChildKeyTake: Get<u16>;
        /// Genesis default minimum childkey take as u16 (`PerU16` scale).
        #[pallet::constant]
        type InitialMinChildKeyTake: Get<u16>;
        /// Genesis default maximum childkey take as u16 (`PerU16` scale).
        #[pallet::constant]
        type InitialMaxChildKeyTake: Get<u16>;
        /// Genesis default weights version key required for `set_weights`.
        #[pallet::constant]
        type InitialWeightsVersionKey: Get<u64>;
        /// Genesis default axon/prometheus serving rate limit, in blocks.
        #[pallet::constant]
        type InitialServingRateLimit: Get<u64>;
        /// Genesis default general transaction rate limit, in blocks.
        #[pallet::constant]
        type InitialTxRateLimit: Get<u64>;
        /// Genesis default rate limit for delegate-take updates, in blocks.
        #[pallet::constant]
        type InitialTxDelegateTakeRateLimit: Get<u64>;
        /// Genesis default rate limit for childkey-take updates, in blocks.
        #[pallet::constant]
        type InitialTxChildKeyTakeRateLimit: Get<u64>;
        /// Genesis default adjustment alpha for burn and PoW difficulty EMA.
        #[pallet::constant]
        type InitialAdjustmentAlpha: Get<u64>;
        /// Genesis default immunity period for newly registered subnets, in blocks.
        #[pallet::constant]
        type InitialNetworkImmunityPeriod: Get<u64>;
        /// Genesis default floor for subnet registration lock cost, in rao.
        #[pallet::constant]
        type InitialNetworkMinLockCost: Get<TaoBalance>;
        /// Genesis default subnet-owner emission cut as u16 (`PerU16` scale).
        #[pallet::constant]
        type InitialSubnetOwnerCut: Get<u16>;
        /// Genesis default interval over which subnet lock cost decays, in blocks.
        #[pallet::constant]
        type InitialNetworkLockReductionInterval: Get<u64>;
        /// Genesis default rate limit between subnet creations, in blocks.
        #[pallet::constant]
        type InitialNetworkRateLimit: Get<u64>;
        /// Fee charged for a global hotkey swap, in rao.
        #[pallet::constant]
        type KeySwapCost: Get<TaoBalance>;
        /// Upper bound for Liquid Alpha parameter (u16 scale).
        #[pallet::constant]
        type AlphaHigh: Get<u16>;
        /// Lower bound for Liquid Alpha parameter (u16 scale).
        #[pallet::constant]
        type AlphaLow: Get<u16>;
        /// Genesis default for whether Liquid Alpha consensus is enabled.
        #[pallet::constant]
        type LiquidAlphaOn: Get<bool>;
        /// Genesis default for whether Yuma3 consensus is enabled.
        #[pallet::constant]
        type Yuma3On: Get<bool>;
        /// Delay after announcing a coldkey swap before it may execute, in blocks.
        #[pallet::constant]
        type InitialColdkeySwapAnnouncementDelay: Get<BlockNumberFor<Self>>;
        /// Minimum delay before re-announcing a coldkey swap, in blocks.
        #[pallet::constant]
        type InitialColdkeySwapReannouncementDelay: Get<BlockNumberFor<Self>>;
        /// Scheduled delay before a dissolve-network call runs, in blocks.
        #[pallet::constant]
        type InitialDissolveNetworkScheduleDuration: Get<BlockNumberFor<Self>>;
        /// Genesis default TAO weight used in root / dual-token emission math (u64 fixed-point).
        #[pallet::constant]
        type InitialTaoWeight: Get<u64>;
        /// Genesis default EMA price halving period, in blocks.
        #[pallet::constant]
        type InitialEmaPriceHalvingPeriod: Get<u64>;
        /// Delay after subnet creation before `start_call` may enable emissions, in blocks.
        #[pallet::constant]
        type InitialStartCallDelay: Get<u64>;
        /// Fee charged for a subnet-scoped hotkey swap, in rao.
        #[pallet::constant]
        type KeySwapOnSubnetCost: Get<TaoBalance>;
        /// Interval (blocks) governing subnet-scoped hotkey-swap rate limits / cleanup slots.
        #[pallet::constant]
        type HotkeySwapOnSubnetInterval: Get<u64>;
        /// Blocks between lease dividend distribution runs.
        #[pallet::constant]
        type LeaseDividendsDistributionInterval: Get<BlockNumberFor<Self>>;
        /// Maximum share of UIDs that may be immune from pruning on a subnet.
        #[pallet::constant]
        type MaxImmuneUidsPercentage: Get<Percent>;
        /// Pallet account id used as the SubtensorModule sovereign account.
        #[pallet::constant]
        type SubtensorPalletId: Get<PalletId>;
        /// Pallet id of the burn sink account for recycled / burned TAO.
        #[pallet::constant]
        type BurnAccountId: Get<PalletId>;
        /// Cap on subnet epochs executed in one `block_step`; overflow deferred via `PendingEpochAt`.
        #[pallet::constant]
        type InitialMaxEpochsPerBlock: Get<u8>;
    }
}
