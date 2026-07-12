#![allow(clippy::crate_in_macro_def)]

use frame_support::pallet_macros::pallet_section;
/// A [`pallet_section`] that defines the errors for a pallet.
/// This can later be imported into the pallet using [`import_section`].
#[pallet_section]
mod config {

    use crate::{CommitmentsInterface, GetAlphaForTao, GetTaoForAlpha};
    use frame_support::PalletId;
    use frame_support::traits::LockableCurrency;
    use pallet_alpha_assets::AlphaAssetsInterface;
    use pallet_commitments::GetCommitments;
    use subtensor_runtime_common::AuthorshipInfo;
    use subtensor_swap_interface::{SwapEngine, SwapHandler};

    /// Configure the pallet by specifying the parameters and types on which it depends.
    #[pallet::config]
    pub trait Config:
        frame_system::Config
        + pallet_drand::Config
        + pallet_crowdloan::Config
        + pallet_scheduler::Config
    {
        /// call type
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = OriginFor<Self>>
            + From<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>
            + From<frame_system::Call<Self>>;

        /// A sudo-able call.
        type SudoRuntimeCall: Parameter
            + UnfilteredDispatchable<RuntimeOrigin = OriginFor<Self>>
            + GetDispatchInfo;

        ///  Currency type that will be used to place deposits on neurons
        type Currency: fungible::Balanced<Self::AccountId, Balance = TaoBalance>
            + fungible::Mutate<Self::AccountId>
            + LockableCurrency<Self::AccountId, Balance = TaoBalance>;

        /// The scheduler type used for scheduling delayed calls.
        type Scheduler: ScheduleAnon<
                BlockNumberFor<Self>,
                LocalCallOf<Self>,
                PalletsOriginOf<Self>,
                Hasher = Self::Hashing,
            >;

        /// the preimage to store the call data.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        /// Implementor of `SwapHandler` interface from `subtensor_swap_interface`
        type SwapInterface: SwapHandler
            + SwapEngine<GetAlphaForTao<Self>>
            + SwapEngine<GetTaoForAlpha<Self>>;

        /// Interface to allow interacting with the proxy pallet.
        type ProxyInterface: crate::ProxyInterface<Self::AccountId>;

        /// Interface to get commitments.
        type GetCommitments: GetCommitments<Self::AccountId>;

        ///  Interface to clean commitments on network dissolution.
        type CommitmentsInterface: CommitmentsInterface;

        /// Interface to mint, burn, and recycle subnet alpha.
        type AlphaAssets: AlphaAssetsInterface;

        /// Rate limit for associating an EVM key.
        type EvmKeyAssociateRateLimit: Get<u64>;

        /// Provider of current block author
        type AuthorshipProvider: AuthorshipInfo<Self::AccountId>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: crate::weights::WeightInfo;

        /// =================================
        /// ==== Initial Value Constants ====
        /// =================================

        /// Initial currency issuance.
        #[pallet::constant]
        type InitialIssuance: Get<TaoBalance>;
        /// Initial min allowed weights setting.
        #[pallet::constant]
        type InitialMinAllowedWeights: Get<u16>;
        /// Initial Emission Ratio.
        #[pallet::constant]
        type InitialEmissionValue: Get<u16>;
        /// Tempo for each network.
        #[pallet::constant]
        type InitialTempo: Get<u16>;
        /// Initial Difficulty.
        #[pallet::constant]
        type InitialDifficulty: Get<u64>;
        /// Initial Max Difficulty.
        #[pallet::constant]
        type InitialMaxDifficulty: Get<u64>;
        /// Initial Min Difficulty.
        #[pallet::constant]
        type InitialMinDifficulty: Get<u64>;
        /// Initial RAO Recycled.
        #[pallet::constant]
        type InitialRAORecycledForRegistration: Get<TaoBalance>;
        /// Initial Burn.
        #[pallet::constant]
        type InitialBurn: Get<TaoBalance>;
        /// Initial Max Burn.
        #[pallet::constant]
        type InitialMaxBurn: Get<TaoBalance>;
        /// Initial Min Burn.
        #[pallet::constant]
        type InitialMinBurn: Get<TaoBalance>;
        /// Initial minimum stake.
        #[pallet::constant]
        type InitialMinStake: Get<TaoBalance>;
        /// Min  burn upper bound.
        #[pallet::constant]
        type MinBurnUpperBound: Get<TaoBalance>;
        /// Max burn lower bound.
        #[pallet::constant]
        type MaxBurnLowerBound: Get<TaoBalance>;
        /// Lower bound for owner-set tempo.
        #[pallet::constant]
        type MinTempo: Get<u16>;
        /// Upper bound for owner-set tempo.
        #[pallet::constant]
        type MaxTempo: Get<u16>;
        /// Lower bound for the activity-cutoff factor (per-mille).
        #[pallet::constant]
        type MinActivityCutoffFactorMilli: Get<u32>;
        /// Upper bound for the activity-cutoff factor (per-mille).
        #[pallet::constant]
        type MaxActivityCutoffFactorMilli: Get<u32>;
        /// Initial adjustment interval.
        #[pallet::constant]
        type InitialAdjustmentInterval: Get<u16>;
        /// Initial bonds moving average.
        #[pallet::constant]
        type InitialBondsMovingAverage: Get<u64>;
        /// Initial bonds penalty.
        #[pallet::constant]
        type InitialBondsPenalty: Get<u16>;
        /// Initial bonds reset.
        #[pallet::constant]
        type InitialBondsResetOn: Get<bool>;
        /// Initial target registrations per interval.
        #[pallet::constant]
        type InitialTargetRegistrationsPerInterval: Get<u16>;
        /// Rho constant.
        #[pallet::constant]
        type InitialRho: Get<u16>;
        /// AlphaSigmoidSteepness constant.
        #[pallet::constant]
        type InitialAlphaSigmoidSteepness: Get<i16>;
        /// Kappa constant.
        #[pallet::constant]
        type InitialKappa: Get<u16>;
        /// Initial minimum allowed network UIDs
        #[pallet::constant]
        type InitialMinAllowedUids: Get<u16>;
        /// Initial maximum allowed network UIDs
        #[pallet::constant]
        type InitialMaxAllowedUids: Get<u16>;
        /// Initial validator context pruning length.
        #[pallet::constant]
        type InitialValidatorPruneLen: Get<u64>;
        /// Initial scaling law power.
        #[pallet::constant]
        type InitialScalingLawPower: Get<u16>;
        /// Immunity Period Constant.
        #[pallet::constant]
        type InitialImmunityPeriod: Get<u16>;
        /// Activity constant.
        #[pallet::constant]
        type InitialActivityCutoff: Get<u16>;
        /// Initial max registrations per block.
        #[pallet::constant]
        type InitialMaxRegistrationsPerBlock: Get<u16>;
        /// Initial pruning score for each neuron.
        #[pallet::constant]
        type InitialPruningScore: Get<u16>;
        /// Initial maximum allowed validators per network.
        #[pallet::constant]
        type InitialMaxAllowedValidators: Get<u16>;
        /// Initial default delegation take.
        #[pallet::constant]
        type InitialDefaultDelegateTake: Get<u16>;
        /// Initial minimum delegation take.
        #[pallet::constant]
        type InitialMinDelegateTake: Get<u16>;
        /// Initial default childkey take.
        #[pallet::constant]
        type InitialDefaultChildKeyTake: Get<u16>;
        /// Initial minimum childkey take.
        #[pallet::constant]
        type InitialMinChildKeyTake: Get<u16>;
        /// Initial maximum childkey take.
        #[pallet::constant]
        type InitialMaxChildKeyTake: Get<u16>;
        /// Initial weights version key.
        #[pallet::constant]
        type InitialWeightsVersionKey: Get<u64>;
        /// Initial serving rate limit.
        #[pallet::constant]
        type InitialServingRateLimit: Get<u64>;
        /// Initial transaction rate limit.
        #[pallet::constant]
        type InitialTxRateLimit: Get<u64>;
        /// Initial delegate take transaction rate limit.
        #[pallet::constant]
        type InitialTxDelegateTakeRateLimit: Get<u64>;
        /// Initial childkey take transaction rate limit.
        #[pallet::constant]
        type InitialTxChildKeyTakeRateLimit: Get<u64>;
        /// Initial adjustment alpha on burn and pow.
        #[pallet::constant]
        type InitialAdjustmentAlpha: Get<u64>;
        /// Initial network immunity period
        #[pallet::constant]
        type InitialNetworkImmunityPeriod: Get<u64>;
        /// Initial network minimum burn cost
        #[pallet::constant]
        type InitialNetworkMinLockCost: Get<TaoBalance>;
        /// Initial network subnet cut.
        #[pallet::constant]
        type InitialSubnetOwnerCut: Get<u16>;
        /// Initial lock reduction interval.
        #[pallet::constant]
        type InitialNetworkLockReductionInterval: Get<u64>;
        /// Initial network creation rate limit
        #[pallet::constant]
        type InitialNetworkRateLimit: Get<u64>;
        /// Cost of swapping a hotkey.
        #[pallet::constant]
        type KeySwapCost: Get<TaoBalance>;
        /// The upper bound for the alpha parameter. Used for Liquid Alpha.
        #[pallet::constant]
        type AlphaHigh: Get<u16>;
        /// The lower bound for the alpha parameter. Used for Liquid Alpha.
        #[pallet::constant]
        type AlphaLow: Get<u16>;
        /// A flag to indicate if Liquid Alpha is enabled.
        #[pallet::constant]
        type LiquidAlphaOn: Get<bool>;
        /// A flag to indicate if Yuma3 is enabled.
        #[pallet::constant]
        type Yuma3On: Get<bool>;
        /// Coldkey swap announcement delay.
        #[pallet::constant]
        type InitialColdkeySwapAnnouncementDelay: Get<BlockNumberFor<Self>>;
        /// Coldkey swap reannouncement delay.
        #[pallet::constant]
        type InitialColdkeySwapReannouncementDelay: Get<BlockNumberFor<Self>>;
        /// Dissolve network schedule duration
        #[pallet::constant]
        type InitialDissolveNetworkScheduleDuration: Get<BlockNumberFor<Self>>;
        /// Initial TAO weight.
        #[pallet::constant]
        type InitialTaoWeight: Get<u64>;
        /// Initial EMA price halving period
        #[pallet::constant]
        type InitialEmaPriceHalvingPeriod: Get<u64>;
        /// Delay after which a new subnet can dispatch start call extrinsic.
        #[pallet::constant]
        type InitialStartCallDelay: Get<u64>;
        /// Cost of swapping a hotkey in a subnet.
        #[pallet::constant]
        type KeySwapOnSubnetCost: Get<TaoBalance>;
        /// Block number for a coldkey swap the hotkey in specific subnet.
        #[pallet::constant]
        type HotkeySwapOnSubnetInterval: Get<u64>;
        /// Number of blocks between dividends distribution.
        #[pallet::constant]
        type LeaseDividendsDistributionInterval: Get<BlockNumberFor<Self>>;
        /// Maximum percentage of immune UIDs.
        #[pallet::constant]
        type MaxImmuneUidsPercentage: Get<Percent>;
        /// Pallet account ID
        #[pallet::constant]
        type SubtensorPalletId: Get<PalletId>;
        /// Burn account ID
        #[pallet::constant]
        type BurnAccountId: Get<PalletId>;
        /// Initial default per-block cap on number of subnet epochs that may
        /// execute in a single `block_step`; the rest are deferred 1 block forward via
        /// `PendingEpochAt`.
        #[pallet::constant]
        type InitialMaxEpochsPerBlock: Get<u8>;
    }
}
