#![cfg_attr(not(feature = "std"), no_std)]
#![recursion_limit = "512"]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::zero_prefixed_literal)]
// Edit this file to define custom logic or remove it if it is not needed.
// Learn more about FRAME and the core library of Substrate FRAME pallets:
// <https://docs.substrate.io/reference/frame-pallets/>

use frame_system::{self as system, ensure_signed};
pub use pallet::*;

use codec::{Decode, Encode};
use frame_support::{
    dispatch::{self, DispatchResult, DispatchResultWithPostInfo},
    ensure,
    pallet_macros::import_section,
    pallet_prelude::*,
    traits::tokens::fungible,
};
use scale_info::TypeInfo;
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_std::marker::PhantomData;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token, TokenReserve};

// ============================
//	==== Benchmark Imports =====
// ============================
mod benchmarks;

// =========================
//	==== Pallet Imports =====
// =========================
pub mod coinbase;
pub mod epoch;
pub mod extensions;
pub mod guards;
pub mod macros;
pub mod migrations;
pub mod root_registered;
pub mod rpc_info;
pub mod staking;
pub mod subnets;
pub mod swap;
pub mod utils;
pub mod weights;
use crate::utils::rate_limiting::{Hyperparameter, TransactionType};
use macros::{config, dispatches, errors, events, genesis, hooks};

pub use extensions::*;
pub use guards::*;

#[cfg(test)]
pub(crate) mod tests;

// apparently this is stabilized since rust 1.36
extern crate alloc;

pub type OriginFor<T> = <T as frame_system::Config>::RuntimeOrigin;

pub const MAX_CRV3_COMMIT_SIZE_BYTES: u32 = 5000;

pub const ALPHA_MAP_BATCH_SIZE: usize = 30;

pub const MAX_NUM_ROOT_CLAIMS: u64 = 50;

pub const MAX_SUBNET_CLAIMS: usize = 5;

pub const MAX_ROOT_CLAIM_THRESHOLD: u64 = 10_000_000;

/// Account flag bit that opts into receiving locked alpha transfers.
pub const ACCOUNT_FLAGS_ACCEPT_LOCKED_ALPHA: u128 = 1u128 << 0;

#[allow(deprecated)]
#[deny(missing_docs)]
#[import_section(errors::errors)]
#[import_section(events::events)]
#[import_section(dispatches::dispatches)]
#[import_section(genesis::genesis)]
#[import_section(hooks::hooks)]
#[import_section(config::config)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use crate::RateLimitKey;
    use crate::migrations;
    use crate::root_registered::{EmaState, EmaValueProvider, InFlightEmaSample};
    use crate::staking::lock::LockState;
    use crate::subnets::leasing::{LeaseId, SubnetLeaseOf};
    use frame_support::Twox64Concat;
    use frame_support::{
        BoundedVec,
        dispatch::GetDispatchInfo,
        pallet_prelude::{DispatchResult, StorageMap, ValueQuery, *},
        traits::{
            OriginTrait, QueryPreimage, StorePreimage, UnfilteredDispatchable, tokens::fungible,
        },
    };
    use frame_system::pallet_prelude::*;
    use pallet_drand::types::RoundNumber;
    use runtime_common::prod_or_fast;
    use share_pool::SafeFloat;
    use sp_core::{ConstU32, H160, H256};
    use sp_runtime::traits::{Dispatchable, TrailingZeroInput};
    use sp_std::collections::btree_map::BTreeMap;
    use sp_std::collections::btree_set::BTreeSet;
    use sp_std::collections::vec_deque::VecDeque;
    use sp_std::vec;
    use sp_std::vec::Vec;
    use substrate_fixed::types::{I64F64, I96F32, U64F64, U96F32};
    use subtensor_macros::freeze_struct;
    use subtensor_runtime_common::{
        AlphaBalance, MechId, NetUid, NetUidStorageIndex, TaoBalance, Token,
    };

    /// Origin for the pallet
    pub type PalletsOriginOf<T> =
        <<T as frame_system::Config>::RuntimeOrigin as OriginTrait>::PalletsOrigin;

    /// Call type for the pallet
    pub type CallOf<T> = <T as frame_system::Config>::RuntimeCall;

    /// Tracks version for migrations. Should be monotonic with respect to the
    /// order of migrations. (i.e. always increasing)
    const STORAGE_VERSION: StorageVersion = StorageVersion::new(7);

    /// Minimum balance required to perform a coldkey swap
    pub const MIN_BALANCE_TO_PERFORM_COLDKEY_SWAP: TaoBalance = TaoBalance::new(100_000_000); // 0.1 TAO in RAO

    /// Minimum commit reveal periods
    pub const MIN_COMMIT_REVEAL_PEROIDS: u64 = 1;
    /// Maximum commit reveal periods
    pub const MAX_COMMIT_REVEAL_PEROIDS: u64 = 100;

    #[pallet::pallet]
    #[pallet::without_storage_info]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// Alias for the account ID.
    pub type AccountIdOf<T> = <T as frame_system::Config>::AccountId;

    /// Struct for Axon.
    pub type AxonInfoOf = AxonInfo;

    /// local one
    pub type LocalCallOf<T> = <T as Config>::RuntimeCall;

    /// Data structure for Axon information.
    #[crate::freeze_struct("3545cfb0cac4c1f5")]
    #[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
    pub struct AxonInfo {
        ///  Axon serving block.
        pub block: u64,
        ///  Axon version
        pub version: u32,
        ///  Axon u128 encoded ip address of type v6 or v4.
        pub ip: u128,
        ///  Axon u16 encoded port.
        pub port: u16,
        ///  Axon ip type, 4 for ipv4 and 6 for ipv6.
        pub ip_type: u8,
        ///  Axon protocol. TCP, UDP, other.
        pub protocol: u8,
        ///  Axon proto placeholder 1.
        pub placeholder1: u8,
        ///  Axon proto placeholder 2.
        pub placeholder2: u8,
    }

    /// Struct for NeuronCertificate.
    pub type NeuronCertificateOf = NeuronCertificate;
    /// Data structure for NeuronCertificate information.
    #[freeze_struct("1c232be200d9ec6c")]
    #[derive(Decode, Encode, Default, TypeInfo, PartialEq, Eq, Clone, Debug)]
    pub struct NeuronCertificate {
        ///  The neuron TLS public key
        pub public_key: BoundedVec<u8, ConstU32<64>>,
        ///  The algorithm used to generate the public key
        pub algorithm: u8,
    }

    impl TryFrom<Vec<u8>> for NeuronCertificate {
        type Error = ();

        fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
            if value.len() > 65 {
                return Err(());
            }
            // take the first byte as the algorithm
            let algorithm = value.first().ok_or(())?;
            // and the rest as the public_key
            let certificate = value.get(1..).ok_or(())?.to_vec();
            Ok(Self {
                public_key: BoundedVec::try_from(certificate).map_err(|_| ())?,
                algorithm: *algorithm,
            })
        }
    }

    ///  Struct for Prometheus.
    pub type PrometheusInfoOf = PrometheusInfo;

    /// Data structure for Prometheus information.
    #[crate::freeze_struct("5dde687e63baf0cd")]
    #[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
    pub struct PrometheusInfo {
        /// Prometheus serving block.
        pub block: u64,
        /// Prometheus version.
        pub version: u32,
        ///  Prometheus u128 encoded ip address of type v6 or v4.
        pub ip: u128,
        ///  Prometheus u16 encoded port.
        pub port: u16,
        /// Prometheus ip type, 4 for ipv4 and 6 for ipv6.
        pub ip_type: u8,
    }

    ///  Struct for ChainIdentities. (DEPRECATED for V2)
    pub type ChainIdentityOf = ChainIdentity;

    /// Data structure for Chain Identities. (DEPRECATED for V2)
    #[crate::freeze_struct("bbfd00438dbe2b58")]
    #[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
    pub struct ChainIdentity {
        /// The name of the chain identity
        pub name: Vec<u8>,
        /// The URL associated with the chain identity
        pub url: Vec<u8>,
        /// The image representation of the chain identity
        pub image: Vec<u8>,
        /// The Discord information for the chain identity
        pub discord: Vec<u8>,
        /// A description of the chain identity
        pub description: Vec<u8>,
        /// Additional information about the chain identity
        pub additional: Vec<u8>,
    }

    ///  Struct for ChainIdentities.
    pub type ChainIdentityOfV2 = ChainIdentityV2;

    /// Data structure for Chain Identities.
    #[crate::freeze_struct("ad72a270be7b59d7")]
    #[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
    pub struct ChainIdentityV2 {
        /// The name of the chain identity
        pub name: Vec<u8>,
        /// The URL associated with the chain identity
        pub url: Vec<u8>,
        /// The github repository associated with the identity
        pub github_repo: Vec<u8>,
        /// The image representation of the chain identity
        pub image: Vec<u8>,
        /// The Discord information for the chain identity
        pub discord: Vec<u8>,
        /// A description of the chain identity
        pub description: Vec<u8>,
        /// Additional information about the chain identity
        pub additional: Vec<u8>,
    }

    ///  Struct for SubnetIdentities. (DEPRECATED for V2)
    pub type SubnetIdentityOf = SubnetIdentity;
    /// Data structure for Subnet Identities. (DEPRECATED for V2)
    #[crate::freeze_struct("f448dc3dad763108")]
    #[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
    pub struct SubnetIdentity {
        /// The name of the subnet
        pub subnet_name: Vec<u8>,
        /// The github repository associated with the chain identity
        pub github_repo: Vec<u8>,
        /// The subnet's contact
        pub subnet_contact: Vec<u8>,
    }

    ///  Struct for SubnetIdentitiesV2. (DEPRECATED for V3)
    pub type SubnetIdentityOfV2 = SubnetIdentityV2;
    /// Data structure for Subnet Identities (DEPRECATED for V3)
    #[crate::freeze_struct("e002be4cd05d7b3e")]
    #[derive(Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug)]
    pub struct SubnetIdentityV2 {
        /// The name of the subnet
        pub subnet_name: Vec<u8>,
        /// The github repository associated with the subnet
        pub github_repo: Vec<u8>,
        /// The subnet's contact
        pub subnet_contact: Vec<u8>,
        /// The subnet's website
        pub subnet_url: Vec<u8>,
        /// The subnet's discord
        pub discord: Vec<u8>,
        /// The subnet's description
        pub description: Vec<u8>,
        /// Additional information about the subnet
        pub additional: Vec<u8>,
    }

    ///  Struct for SubnetIdentitiesV3.
    pub type SubnetIdentityOfV3 = SubnetIdentityV3;
    /// Data structure for Subnet Identities
    #[crate::freeze_struct("6a441335f985a0b")]
    #[derive(
        Encode, Decode, DecodeWithMemTracking, Default, TypeInfo, Clone, PartialEq, Eq, Debug,
    )]
    pub struct SubnetIdentityV3 {
        /// The name of the subnet
        pub subnet_name: Vec<u8>,
        /// The github repository associated with the subnet
        pub github_repo: Vec<u8>,
        /// The subnet's contact
        pub subnet_contact: Vec<u8>,
        /// The subnet's website
        pub subnet_url: Vec<u8>,
        /// The subnet's discord
        pub discord: Vec<u8>,
        /// The subnet's description
        pub description: Vec<u8>,
        /// The subnet's logo
        pub logo_url: Vec<u8>,
        /// Additional information about the subnet
        pub additional: Vec<u8>,
    }

    /// Enum for recycle or burn for the owner_uid(s)
    #[derive(TypeInfo, Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug)]
    pub enum RecycleOrBurnEnum {
        /// Burn the miner emission sent to the burn UID
        Burn,
        /// Recycle the miner emission sent to the recycle UID
        Recycle,
    }

    /// ============================
    /// ==== Staking + Accounts ====
    /// ============================

    #[derive(
        Encode, Decode, Default, TypeInfo, Clone, PartialEq, Eq, Debug, DecodeWithMemTracking,
    )]
    /// Enum for the per-coldkey root claim setting.
    pub enum RootClaimTypeEnum {
        /// Swap any alpha emission for TAO.
        #[default]
        Swap,
        /// Keep all alpha emission.
        Keep,
        /// Keep all alpha emission for specified subnets.
        KeepSubnets {
            /// Subnets to keep alpha emissions (swap everything else).
            subnets: BTreeSet<NetUid>,
        },
    }

    /// The Max Burn HalfLife Settable
    #[pallet::type_value]
    pub fn MaxBurnHalfLife<T: Config>() -> u16 {
        36_100
    }

    /// Default burn half-life (in blocks) for subnet registration price decay.
    #[pallet::type_value]
    pub fn DefaultBurnHalfLife<T: Config>() -> u16 {
        360
    }

    /// Default multiplier applied to the burn price after a successful registration.
    #[pallet::type_value]
    pub fn DefaultBurnIncreaseMult<T: Config>() -> U64F64 {
        U64F64::from_num(1.26)
    }

    /// Default Neuron Burn Cost
    #[pallet::type_value]
    pub fn DefaultNeuronBurnCost<T: Config>() -> TaoBalance {
        TaoBalance::from(1_000_000_000u64)
    }

    /// Default minimum root claim amount.
    /// This is the minimum amount of root claim that can be made.
    /// Any amount less than this will not be claimed.
    #[pallet::type_value]
    pub fn DefaultMinRootClaimAmount<T: Config>() -> I96F32 {
        500_000u64.into()
    }

    /// Default root claim type.
    /// This is the type of root claim that will be made.
    /// This is set by the user. Either swap to TAO or keep as alpha.
    #[pallet::type_value]
    pub fn DefaultRootClaimType<T: Config>() -> RootClaimTypeEnum {
        RootClaimTypeEnum::default()
    }

    /// Default number of root claims per claim call.
    /// Ideally this is calculated using the number of staking coldkey
    /// and the block time.
    #[pallet::type_value]
    pub fn DefaultNumRootClaim<T: Config>() -> u64 {
        // once per week (+ spare keys for skipped tries)
        5
    }

    /// Default value for zero.
    #[pallet::type_value]
    pub fn DefaultZeroU64<T: Config>() -> u64 {
        0
    }

    /// Default value for zero.
    #[pallet::type_value]
    pub fn DefaultZeroI64<T: Config>() -> i64 {
        0
    }
    /// Default value for Alpha currency.
    #[pallet::type_value]
    pub fn DefaultZeroAlpha<T: Config>() -> AlphaBalance {
        AlphaBalance::ZERO
    }

    /// Default value for Tao currency.
    #[pallet::type_value]
    pub fn DefaultZeroTao<T: Config>() -> TaoBalance {
        TaoBalance::ZERO
    }

    /// Default value for zero.
    #[pallet::type_value]
    pub fn DefaultZeroU128<T: Config>() -> u128 {
        0
    }

    /// Default value for zero.
    #[pallet::type_value]
    pub fn DefaultZeroU16<T: Config>() -> u16 {
        0
    }

    /// Default value for false.
    #[pallet::type_value]
    pub fn DefaultFalse<T: Config>() -> bool {
        false
    }

    /// Default value for false.
    #[pallet::type_value]
    pub fn DefaultTrue<T: Config>() -> bool {
        true
    }

    /// Total Rao in circulation.
    #[pallet::type_value]
    pub fn TotalSupply<T: Config>() -> u64 {
        21_000_000_000_000_000
    }

    /// Default Delegate Take.
    #[pallet::type_value]
    pub fn DefaultDelegateTake<T: Config>() -> u16 {
        T::InitialDefaultDelegateTake::get()
    }

    /// Default childkey take.
    #[pallet::type_value]
    pub fn DefaultChildKeyTake<T: Config>() -> u16 {
        T::InitialDefaultChildKeyTake::get()
    }

    /// Default minimum delegate take.
    #[pallet::type_value]
    pub fn DefaultMinDelegateTake<T: Config>() -> u16 {
        T::InitialMinDelegateTake::get()
    }

    /// Default minimum childkey take.
    #[pallet::type_value]
    pub fn DefaultMinChildKeyTake<T: Config>() -> u16 {
        T::InitialMinChildKeyTake::get()
    }

    /// Default maximum childkey take.
    #[pallet::type_value]
    pub fn DefaultMaxChildKeyTake<T: Config>() -> u16 {
        T::InitialMaxChildKeyTake::get()
    }

    /// Default account take.
    #[pallet::type_value]
    pub fn DefaultAccountTake<T: Config>() -> u64 {
        0
    }

    /// Default value for global weight.
    #[pallet::type_value]
    pub fn DefaultTaoWeight<T: Config>() -> u64 {
        T::InitialTaoWeight::get()
    }

    /// Default emission per block.
    #[pallet::type_value]
    pub fn DefaultBlockEmission<T: Config>() -> u64 {
        1_000_000_000
    }

    /// Default allowed delegation.
    #[pallet::type_value]
    pub fn DefaultAllowsDelegation<T: Config>() -> bool {
        false
    }

    /// Default total issuance.
    #[pallet::type_value]
    pub fn DefaultTotalIssuance<T: Config>() -> TaoBalance {
        T::InitialIssuance::get().into()
    }

    /// Default account, derived from zero trailing bytes.
    #[pallet::type_value]
    pub fn DefaultAccount<T: Config>() -> T::AccountId {
        #[allow(clippy::expect_used)]
        T::AccountId::decode(&mut TrailingZeroInput::zeroes())
            .expect("trailing zeroes always produce a valid account ID; qed")
    }
    // pub fn DefaultStakeInterval<T: Config>() -> u64 {
    //     360
    // } (DEPRECATED)

    /// Default account linkage
    #[pallet::type_value]
    pub fn DefaultAccountLinkage<T: Config>() -> Vec<(u64, T::AccountId)> {
        vec![]
    }

    /// Default pending childkeys
    #[pallet::type_value]
    pub fn DefaultPendingChildkeys<T: Config>() -> (Vec<(u64, T::AccountId)>, u64) {
        (vec![], 0)
    }

    /// Default account linkage
    #[pallet::type_value]
    pub fn DefaultProportion<T: Config>() -> u64 {
        0
    }

    /// Default accumulated emission for a hotkey
    #[pallet::type_value]
    pub fn DefaultAccumulatedEmission<T: Config>() -> u64 {
        0
    }

    /// Default last adjustment block.
    #[pallet::type_value]
    pub fn DefaultLastAdjustmentBlock<T: Config>() -> u64 {
        0
    }

    /// Default last adjustment block.
    #[pallet::type_value]
    pub fn DefaultRegistrationsThisBlock<T: Config>() -> u16 {
        0
    }

    /// Default EMA price halving blocks
    #[pallet::type_value]
    pub fn DefaultEMAPriceMovingBlocks<T: Config>() -> u64 {
        T::InitialEmaPriceHalvingPeriod::get()
    }

    /// Default registrations this block.
    #[pallet::type_value]
    pub fn DefaultBurn<T: Config>() -> TaoBalance {
        T::InitialBurn::get().into()
    }

    /// Default burn token.
    #[pallet::type_value]
    pub fn DefaultMinBurn<T: Config>() -> TaoBalance {
        T::InitialMinBurn::get().into()
    }

    /// Default min burn token.
    #[pallet::type_value]
    pub fn DefaultMaxBurn<T: Config>() -> TaoBalance {
        T::InitialMaxBurn::get().into()
    }

    /// Default max burn token.
    #[pallet::type_value]
    pub fn DefaultDifficulty<T: Config>() -> u64 {
        T::InitialDifficulty::get()
    }

    /// Default difficulty value.
    #[pallet::type_value]
    pub fn DefaultMinDifficulty<T: Config>() -> u64 {
        T::InitialMinDifficulty::get()
    }

    /// Default min difficulty value.
    #[pallet::type_value]
    pub fn DefaultMaxDifficulty<T: Config>() -> u64 {
        T::InitialMaxDifficulty::get()
    }

    /// Default max difficulty value.
    #[pallet::type_value]
    pub fn DefaultMaxRegistrationsPerBlock<T: Config>() -> u16 {
        T::InitialMaxRegistrationsPerBlock::get()
    }

    /// Default max registrations per block.
    #[pallet::type_value]
    pub fn DefaultRAORecycledForRegistration<T: Config>() -> TaoBalance {
        T::InitialRAORecycledForRegistration::get().into()
    }

    /// Default number of networks.
    #[pallet::type_value]
    pub fn DefaultN<T: Config>() -> u16 {
        0
    }

    /// Default value for hotkeys.
    #[pallet::type_value]
    pub fn DefaultHotkeys<T: Config>() -> Vec<u16> {
        vec![]
    }

    /// Default value if network is added.
    #[pallet::type_value]
    pub fn DefaultNeworksAdded<T: Config>() -> bool {
        false
    }

    /// Default value for network member.
    #[pallet::type_value]
    pub fn DefaultIsNetworkMember<T: Config>() -> bool {
        false
    }

    /// Default value for registration allowed.
    #[pallet::type_value]
    pub fn DefaultRegistrationAllowed<T: Config>() -> bool {
        true
    }

    /// Default value for network registered at.
    #[pallet::type_value]
    pub fn DefaultNetworkRegisteredAt<T: Config>() -> u64 {
        0
    }

    /// Default value for network immunity period.
    #[pallet::type_value]
    pub fn DefaultNetworkImmunityPeriod<T: Config>() -> u64 {
        T::InitialNetworkImmunityPeriod::get()
    }

    /// Default value for network min lock cost.
    #[pallet::type_value]
    pub fn DefaultNetworkMinLockCost<T: Config>() -> TaoBalance {
        T::InitialNetworkMinLockCost::get().into()
    }

    /// Default value for network lock reduction interval.
    #[pallet::type_value]
    pub fn DefaultNetworkLockReductionInterval<T: Config>() -> u64 {
        T::InitialNetworkLockReductionInterval::get()
    }

    /// Default value for subnet owner cut.
    #[pallet::type_value]
    pub fn DefaultSubnetOwnerCut<T: Config>() -> u16 {
        T::InitialSubnetOwnerCut::get()
    }

    /// Default value for recycle or burn.
    #[pallet::type_value]
    pub fn DefaultRecycleOrBurn<T: Config>() -> RecycleOrBurnEnum {
        RecycleOrBurnEnum::Burn // default to burn
    }

    /// Default value for network rate limit.
    #[pallet::type_value]
    pub fn DefaultNetworkRateLimit<T: Config>() -> u64 {
        if cfg!(feature = "pow-faucet") {
            return 0;
        }
        T::InitialNetworkRateLimit::get()
    }

    /// Default value for network rate limit.
    #[pallet::type_value]
    pub fn DefaultNetworkRegistrationStartBlock<T: Config>() -> u64 {
        0
    }

    /// Default value for TAO-in refund deployment block.
    #[pallet::type_value]
    pub fn DefaultTaoInRefundDeploymentBlock() -> u64 {
        0
    }

    /// Default value for weights version key rate limit.
    /// In units of tempos.
    #[pallet::type_value]
    pub fn DefaultWeightsVersionKeyRateLimit<T: Config>() -> u64 {
        5 // 5 tempos
    }

    /// Default value for pending emission.
    #[pallet::type_value]
    pub fn DefaultPendingEmission<T: Config>() -> AlphaBalance {
        0.into()
    }

    /// Default value for blocks since last step.
    #[pallet::type_value]
    pub fn DefaultBlocksSinceLastStep<T: Config>() -> u64 {
        0
    }

    /// Default value for last mechanism step block.
    #[pallet::type_value]
    pub fn DefaultLastMechanismStepBlock<T: Config>() -> u64 {
        0
    }

    /// Default value for subnet owner.
    #[pallet::type_value]
    pub fn DefaultSubnetOwner<T: Config>() -> T::AccountId {
        #[allow(clippy::expect_used)]
        T::AccountId::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
            .expect("trailing zeroes always produce a valid account ID; qed")
    }

    /// Default value for subnet locked.
    #[pallet::type_value]
    pub fn DefaultSubnetLocked<T: Config>() -> u64 {
        0
    }

    /// Default value for network tempo
    #[pallet::type_value]
    pub fn DefaultTempo<T: Config>() -> u16 {
        T::InitialTempo::get()
    }

    /// Default value for weights set rate limit.
    #[pallet::type_value]
    pub fn DefaultWeightsSetRateLimit<T: Config>() -> u64 {
        100
    }

    /// Default block number at registration.
    #[pallet::type_value]
    pub fn DefaultBlockAtRegistration<T: Config>() -> u64 {
        0
    }

    /// Default value for rho parameter.
    #[pallet::type_value]
    pub fn DefaultRho<T: Config>() -> u16 {
        T::InitialRho::get()
    }

    /// Default value for alpha sigmoid steepness.
    #[pallet::type_value]
    pub fn DefaultAlphaSigmoidSteepness<T: Config>() -> i16 {
        T::InitialAlphaSigmoidSteepness::get()
    }

    /// Default value for kappa parameter.
    #[pallet::type_value]
    pub fn DefaultKappa<T: Config>() -> u16 {
        T::InitialKappa::get()
    }

    /// Default value for network min allowed UIDs.
    #[pallet::type_value]
    pub fn DefaultMinAllowedUids<T: Config>() -> u16 {
        T::InitialMinAllowedUids::get()
    }

    /// Default maximum allowed UIDs.
    #[pallet::type_value]
    pub fn DefaultMaxAllowedUids<T: Config>() -> u16 {
        T::InitialMaxAllowedUids::get()
    }

    /// -- Rate limit for set max allowed UIDs
    #[pallet::type_value]
    pub fn MaxUidsTrimmingRateLimit<T: Config>() -> u64 {
        prod_or_fast!(30 * 7200, 1)
    }

    /// Default immunity period.
    #[pallet::type_value]
    pub fn DefaultImmunityPeriod<T: Config>() -> u16 {
        T::InitialImmunityPeriod::get()
    }

    /// Default activity cutoff.
    #[pallet::type_value]
    pub fn DefaultActivityCutoff<T: Config>() -> u16 {
        T::InitialActivityCutoff::get()
    }

    /// Default weights version key.
    #[pallet::type_value]
    pub fn DefaultWeightsVersionKey<T: Config>() -> u64 {
        T::InitialWeightsVersionKey::get()
    }

    /// Default minimum allowed weights.
    #[pallet::type_value]
    pub fn DefaultMinAllowedWeights<T: Config>() -> u16 {
        T::InitialMinAllowedWeights::get()
    }
    /// Default maximum allowed validators.
    #[pallet::type_value]
    pub fn DefaultMaxAllowedValidators<T: Config>() -> u16 {
        T::InitialMaxAllowedValidators::get()
    }

    /// Default adjustment interval.
    #[pallet::type_value]
    pub fn DefaultAdjustmentInterval<T: Config>() -> u16 {
        T::InitialAdjustmentInterval::get()
    }

    /// Default bonds moving average.
    #[pallet::type_value]
    pub fn DefaultBondsMovingAverage<T: Config>() -> u64 {
        T::InitialBondsMovingAverage::get()
    }

    /// Default bonds penalty.
    #[pallet::type_value]
    pub fn DefaultBondsPenalty<T: Config>() -> u16 {
        T::InitialBondsPenalty::get()
    }

    /// Default value for bonds reset - will not reset bonds
    #[pallet::type_value]
    pub fn DefaultBondsResetOn<T: Config>() -> bool {
        T::InitialBondsResetOn::get()
    }

    /// Default validator prune length.
    #[pallet::type_value]
    pub fn DefaultValidatorPruneLen<T: Config>() -> u64 {
        T::InitialValidatorPruneLen::get()
    }

    /// Default scaling law power.
    #[pallet::type_value]
    pub fn DefaultScalingLawPower<T: Config>() -> u16 {
        T::InitialScalingLawPower::get()
    }

    /// Default target registrations per interval.
    #[pallet::type_value]
    pub fn DefaultTargetRegistrationsPerInterval<T: Config>() -> u16 {
        T::InitialTargetRegistrationsPerInterval::get()
    }

    /// Default adjustment alpha.
    #[pallet::type_value]
    pub fn DefaultAdjustmentAlpha<T: Config>() -> u64 {
        T::InitialAdjustmentAlpha::get()
    }

    /// Default minimum stake for weights.
    #[pallet::type_value]
    pub fn DefaultStakeThreshold<T: Config>() -> u64 {
        0
    }

    /// Default Reveal Period Epochs
    #[pallet::type_value]
    pub fn DefaultRevealPeriodEpochs<T: Config>() -> u64 {
        1
    }

    /// Value definition for vector of u16.
    #[pallet::type_value]
    pub fn EmptyU16Vec<T: Config>() -> Vec<u16> {
        vec![]
    }

    /// Value definition for vector of u64.
    #[pallet::type_value]
    pub fn EmptyU64Vec<T: Config>() -> Vec<u64> {
        vec![]
    }

    /// Value definition for vector of bool.
    #[pallet::type_value]
    pub fn EmptyBoolVec<T: Config>() -> Vec<bool> {
        vec![]
    }

    /// Value definition for bonds with type vector of (u16, u16).
    #[pallet::type_value]
    pub fn DefaultBonds<T: Config>() -> Vec<(u16, u16)> {
        vec![]
    }

    /// Value definition for weights with vector of (u16, u16).
    #[pallet::type_value]
    pub fn DefaultWeights<T: Config>() -> Vec<(u16, u16)> {
        vec![]
    }

    /// Default value for key with type T::AccountId derived from trailing zeroes.
    #[pallet::type_value]
    pub fn DefaultKey<T: Config>() -> T::AccountId {
        #[allow(clippy::expect_used)]
        T::AccountId::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
            .expect("trailing zeroes always produce a valid account ID; qed")
    }
    // pub fn DefaultHotkeyEmissionTempo<T: Config>() -> u64 {
    //     T::InitialHotkeyEmissionTempo::get()
    // } (DEPRECATED)

    /// Default per-block epoch cap, seeded from the runtime-configured initial value.
    #[pallet::type_value]
    pub fn DefaultMaxEpochsPerBlock<T: Config>() -> u8 {
        T::InitialMaxEpochsPerBlock::get()
    }

    /// Default value for rate limiting
    #[pallet::type_value]
    pub fn DefaultTxRateLimit<T: Config>() -> u64 {
        T::InitialTxRateLimit::get()
    }

    /// Default value for delegate take rate limiting
    #[pallet::type_value]
    pub fn DefaultTxDelegateTakeRateLimit<T: Config>() -> u64 {
        T::InitialTxDelegateTakeRateLimit::get()
    }

    /// Default value for chidlkey take rate limiting
    #[pallet::type_value]
    pub fn DefaultTxChildKeyTakeRateLimit<T: Config>() -> u64 {
        T::InitialTxChildKeyTakeRateLimit::get()
    }

    /// Default value for last extrinsic block.
    #[pallet::type_value]
    pub fn DefaultLastTxBlock<T: Config>() -> u64 {
        0
    }

    /// Default value for serving rate limit.
    #[pallet::type_value]
    pub fn DefaultServingRateLimit<T: Config>() -> u64 {
        T::InitialServingRateLimit::get()
    }

    /// Default value for weight commit/reveal enabled.
    #[pallet::type_value]
    pub fn DefaultCommitRevealWeightsEnabled<T: Config>() -> bool {
        true
    }

    /// Default value for weight commit/reveal version.
    #[pallet::type_value]
    pub fn DefaultCommitRevealWeightsVersion<T: Config>() -> u16 {
        4
    }

    /// -- ITEM (switches liquid alpha on)
    #[pallet::type_value]
    pub fn DefaultLiquidAlpha<T: Config>() -> bool {
        false
    }

    /// -- ITEM (switches liquid alpha on)
    #[pallet::type_value]
    pub fn DefaultYuma3<T: Config>() -> bool {
        false
    }

    /// (alpha_low: 0.7, alpha_high: 0.9)
    #[pallet::type_value]
    pub fn DefaultAlphaValues<T: Config>() -> (u16, u16) {
        (45875, 58982)
    }

    /// Default value for coldkey swap announcement delay.
    #[pallet::type_value]
    pub fn DefaultColdkeySwapAnnouncementDelay<T: Config>() -> BlockNumberFor<T> {
        T::InitialColdkeySwapAnnouncementDelay::get()
    }

    /// Default value for coldkey swap reannouncement delay.
    #[pallet::type_value]
    pub fn DefaultColdkeySwapReannouncementDelay<T: Config>() -> BlockNumberFor<T> {
        T::InitialColdkeySwapReannouncementDelay::get()
    }

    /// Default value for applying pending items (e.g. childkeys).
    #[pallet::type_value]
    pub fn DefaultPendingCooldown<T: Config>() -> u64 {
        prod_or_fast!(7_200, 15)
    }

    /// Default minimum stake.
    #[pallet::type_value]
    pub fn DefaultMinStake<T: Config>() -> TaoBalance {
        T::InitialMinStake::get().into()
    }

    /// Default unicode vector for tau symbol.
    #[pallet::type_value]
    pub fn DefaultUnicodeVecU8<T: Config>() -> Vec<u8> {
        b"\xF0\x9D\x9C\x8F".to_vec() // Unicode for tau (𝜏)
    }

    /// Default value for dissolve network schedule duration
    #[pallet::type_value]
    pub fn DefaultDissolveNetworkScheduleDuration<T: Config>() -> BlockNumberFor<T> {
        T::InitialDissolveNetworkScheduleDuration::get()
    }

    /// Default moving alpha for the moving price.
    #[pallet::type_value]
    pub fn DefaultMovingAlpha<T: Config>() -> I96F32 {
        // Moving average take 30 days to reach 50% of the price
        // and 3.5 months to reach 90%.
        I96F32::saturating_from_num(0.000003)
    }

    /// Default subnet moving price.
    #[pallet::type_value]
    pub fn DefaultMovingPrice<T: Config>() -> I96F32 {
        I96F32::saturating_from_num(0.0)
    }

    /// Default subnet root proportion.
    #[pallet::type_value]
    pub fn DefaultRootProp<T: Config>() -> U96F32 {
        U96F32::saturating_from_num(0.0)
    }

    /// Default subnet root claimable
    #[pallet::type_value]
    pub fn DefaultRootClaimable<T: Config>() -> BTreeMap<NetUid, I96F32> {
        Default::default()
    }

    /// Default value for Share Pool variables
    #[pallet::type_value]
    pub fn DefaultSharePoolZero<T: Config>() -> U64F64 {
        U64F64::saturating_from_num(0)
    }

    /// Default value for minimum activity cutoff
    #[pallet::type_value]
    pub fn DefaultMinActivityCutoff<T: Config>() -> u16 {
        360
    }

    /// Default value for setting subnet owner hotkey rate limit
    #[pallet::type_value]
    pub fn DefaultSetSNOwnerHotkeyRateLimit<T: Config>() -> u64 {
        50400
    }

    /// Default last Alpha map key for iteration
    #[pallet::type_value]
    pub fn DefaultAlphaIterationLastKey<T: Config>() -> Option<Vec<u8>> {
        None
    }

    /// Default number of terminal blocks in a tempo during which admin operations are prohibited
    #[pallet::type_value]
    pub fn DefaultAdminFreezeWindow<T: Config>() -> u16 {
        10
    }

    /// Default number of tempos for owner hyperparameter update rate limit
    #[pallet::type_value]
    pub fn DefaultOwnerHyperparamRateLimit<T: Config>() -> u16 {
        2
    }

    /// Default value for ck burn, 18%.
    #[pallet::type_value]
    pub fn DefaultCKBurn<T: Config>() -> u64 {
        0
    }

    /// Default value for subnet limit.
    #[pallet::type_value]
    pub fn DefaultSubnetLimit<T: Config>() -> u16 {
        128
    }

    /// Default value for MinNonImmuneUids.
    #[pallet::type_value]
    pub fn DefaultMinNonImmuneUids<T: Config>() -> u16 {
        10u16
    }

    /// Default value for AutoParentDelegationEnabled.
    #[pallet::type_value]
    pub fn DefaultAutoParentDelegationEnabled<T: Config>() -> bool {
        true
    }

    #[pallet::storage]
    pub type MinActivityCutoff<T: Config> =
        StorageValue<_, u16, ValueQuery, DefaultMinActivityCutoff<T>>;

    /// Global window (in blocks) at the end of each tempo where admin ops are disallowed
    #[pallet::storage]
    pub type AdminFreezeWindow<T: Config> =
        StorageValue<_, u16, ValueQuery, DefaultAdminFreezeWindow<T>>;

    /// Global number of epochs used to rate limit subnet owner hyperparameter updates
    #[pallet::storage]
    pub type OwnerHyperparamRateLimit<T: Config> =
        StorageValue<_, u16, ValueQuery, DefaultOwnerHyperparamRateLimit<T>>;

    /// Duration of dissolve network schedule before execution
    #[pallet::storage]
    pub type DissolveNetworkScheduleDuration<T: Config> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery, DefaultDissolveNetworkScheduleDuration<T>>;

    /// --- DMap ( netuid, coldkey ) --> blocknumber | last hotkey swap on network.
    #[pallet::storage]
    pub type LastHotkeySwapOnNetuid<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        u64,
        ValueQuery,
        DefaultZeroU64<T>,
    >;

    /// Ensures unique IDs for StakeJobs storage map
    #[pallet::storage]
    pub type NextStakeJobId<T> = StorageValue<_, u64, ValueQuery, DefaultZeroU64<T>>;

    /// ============================
    /// ==== Staking Variables ====
    /// ============================
    /// The Subtensor [`TotalIssuance`] represents the total issuance of tokens on the Bittensor network.
    ///
    /// It is comprised of three parts:
    /// - The total amount of issued tokens, tracked in the TotalIssuance of the Balances pallet
    /// - The total amount of tokens staked in the system, tracked in [`TotalStake`]
    /// - The total amount of tokens locked up for subnet reg, tracked in [`TotalSubnetLocked`] attained by iterating over subnet lock.
    ///
    /// Eventually, Bittensor should migrate to using Holds afterwhich time we will not require this
    /// separate accounting.
    /// --- ITEM --> Global weight
    #[pallet::storage]
    pub type TaoWeight<T> = StorageValue<_, u64, ValueQuery, DefaultTaoWeight<T>>;

    /// --- ITEM --> CK burn
    #[pallet::storage]
    pub type CKBurn<T> = StorageValue<_, u64, ValueQuery, DefaultCKBurn<T>>;

    /// --- ITEM ( default_delegate_take )
    #[pallet::storage]
    pub type MaxDelegateTake<T> = StorageValue<_, u16, ValueQuery, DefaultDelegateTake<T>>;

    /// --- ITEM ( min_delegate_take )
    #[pallet::storage]
    pub type MinDelegateTake<T> = StorageValue<_, u16, ValueQuery, DefaultMinDelegateTake<T>>;

    /// --- ITEM ( default_childkey_take )
    #[pallet::storage]
    pub type MaxChildkeyTake<T> = StorageValue<_, u16, ValueQuery, DefaultMaxChildKeyTake<T>>;

    /// --- ITEM ( min_childkey_take )
    #[pallet::storage]
    pub type MinChildkeyTake<T> = StorageValue<_, u16, ValueQuery, DefaultMinChildKeyTake<T>>;

    /// MAP ( netuid ) --> take | Returns the subnet-specific minimum childkey take.
    #[pallet::storage]
    pub type MinChildkeyTakePerSubnet<T: Config> = StorageMap<_, Identity, NetUid, u16, ValueQuery>;

    /// MAP ( hot ) --> cold | Returns the controlling coldkey for a hotkey
    #[pallet::storage]
    pub type Owner<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, T::AccountId, ValueQuery, DefaultAccount<T>>;

    /// MAP ( coldkey ) --> flags | Account-level flags. Defaults to zero.
    #[pallet::storage]
    pub type AccountFlags<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u128, ValueQuery>;

    /// MAP ( hot ) --> take | Returns the hotkey delegation take. And signals that this key is open for delegation
    #[pallet::storage]
    pub type Delegates<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u16, ValueQuery, DefaultDelegateTake<T>>;

    /// DMAP ( hot, netuid ) --> take | Returns the hotkey childkey take for a specific subnet
    #[pallet::storage]
    pub type ChildkeyTake<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId, // First key: hotkey
        Identity,
        NetUid, // Second key: netuid
        u16,    // Value: take
        ValueQuery,
    >;

    /// DMAP ( netuid, parent ) --> (Vec<(proportion,child)>, cool_down_block)
    #[pallet::storage]
    pub type PendingChildKeys<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        (Vec<(u64, T::AccountId)>, u64),
        ValueQuery,
        DefaultPendingChildkeys<T>,
    >;

    /// DMAP ( parent, netuid ) --> Vec<(proportion,child)>
    #[pallet::storage]
    pub type ChildKeys<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        Vec<(u64, T::AccountId)>,
        ValueQuery,
        DefaultAccountLinkage<T>,
    >;

    /// DMAP ( child, netuid ) --> Vec<(proportion,parent)>
    #[pallet::storage]
    pub type ParentKeys<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        Vec<(u64, T::AccountId)>,
        ValueQuery,
        DefaultAccountLinkage<T>,
    >;

    /// --- DMAP ( netuid, hotkey ) --> u64 | Last alpha dividend this hotkey got on tempo.
    #[pallet::storage]
    pub type AlphaDividendsPerSubnet<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        AlphaBalance,
        ValueQuery,
        DefaultZeroAlpha<T>,
    >;

    /// --- DMAP ( netuid, hotkey ) --> u64 | Last root alpha dividend this hotkey got on tempo.
    #[pallet::storage]
    pub type RootAlphaDividendsPerSubnet<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        AlphaBalance,
        ValueQuery,
        DefaultZeroAlpha<T>,
    >;

    /// ==================
    /// ==== Coinbase ====
    /// ==================
    /// --- ITEM ( global_block_emission )
    #[deprecated(note = "Use calculate_block_emission() or the block emission RPC instead.")]
    #[pallet::storage]
    pub type BlockEmission<T> = StorageValue<_, u64, ValueQuery, DefaultBlockEmission<T>>;

    /// --- DMap ( hot, netuid ) --> emission | last hotkey emission on network.
    #[pallet::storage]
    pub type LastHotkeyEmissionOnNetuid<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        AlphaBalance,
        ValueQuery,
        DefaultZeroAlpha<T>,
    >;
    /// ==========================
    /// ==== Staking Counters ====
    /// ==========================
    /// The Subtensor [`TotalIssuance`] represents the total issuance of tokens on the Bittensor network.
    ///
    /// It is comprised of three parts:
    /// - The total amount of issued tokens, tracked in the TotalIssuance of the Balances pallet
    /// - The total amount of tokens staked in the system, tracked in [`TotalStake`]
    /// - The total amount of tokens locked up for subnet reg, tracked in [`TotalSubnetLocked`] attained by iterating over subnet lock.
    ///
    /// Eventually, Bittensor should migrate to using Holds afterwhich time we will not require this
    /// separate accounting.
    /// --- ITEM ( maximum_number_of_networks )
    #[pallet::storage]
    pub type SubnetLimit<T> = StorageValue<_, u16, ValueQuery, DefaultSubnetLimit<T>>;

    /// --- ITEM ( total_issuance )
    #[pallet::storage]
    pub type TotalIssuance<T> = StorageValue<_, TaoBalance, ValueQuery, DefaultTotalIssuance<T>>;

    /// --- ITEM ( total_stake )
    #[pallet::storage]
    pub type TotalStake<T> = StorageValue<_, TaoBalance, ValueQuery, DefaultZeroTao<T>>;

    /// --- ITEM ( moving_alpha ) -- subnet moving alpha.
    #[pallet::storage]
    pub type SubnetMovingAlpha<T> = StorageValue<_, I96F32, ValueQuery, DefaultMovingAlpha<T>>;

    /// --- MAP ( netuid ) --> moving_price | The subnet moving price.
    #[pallet::storage]
    pub type SubnetMovingPrice<T: Config> =
        StorageMap<_, Identity, NetUid, I96F32, ValueQuery, DefaultMovingPrice<T>>;

    /// --- MAP ( netuid ) --> root_prop | The subnet root proportion.
    #[pallet::storage]
    pub type RootProp<T: Config> =
        StorageMap<_, Identity, NetUid, U96F32, ValueQuery, DefaultRootProp<T>>;

    /// --- MAP ( netuid ) --> total_volume | The total amount of TAO bought and sold since the start of the network.
    #[pallet::storage]
    pub type SubnetVolume<T: Config> =
        StorageMap<_, Identity, NetUid, u128, ValueQuery, DefaultZeroU128<T>>;

    /// --- MAP ( netuid ) --> tao_in_subnet | Returns the amount of TAO in the subnet.
    #[pallet::storage]
    pub type SubnetTAO<T: Config> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultZeroTao<T>>;

    /// --- MAP ( netuid ) --> alpha_in_emission | Returns the amount of alph in  emission into the pool per block.
    #[pallet::storage]
    pub type SubnetAlphaInEmission<T: Config> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( netuid ) --> subnet_emission_enabled
    ///
    /// When false, subnet pool-side emission is disabled for this subnet:
    /// `alpha_in`, `tao_in`, and `excess_tao` chain buys are all treated as zero.
    /// `alpha_out`, owner cut, root proportion, pending server emission, and pending
    /// validator emission are intentionally left unchanged.
    ///
    /// Defaults to true so existing subnets keep current behavior.
    #[pallet::storage]
    pub type SubnetEmissionEnabled<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultTrue<T>>;

    /// --- MAP ( netuid ) --> alpha_out_emission | Returns the amount of alpha out emission into the network per block.
    #[pallet::storage]
    pub type SubnetAlphaOutEmission<T: Config> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( netuid ) --> tao_in_emission | Returns the amount of tao emitted into this subent on the last block.
    #[pallet::storage]
    pub type SubnetTaoInEmission<T: Config> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultZeroTao<T>>;

    /// --- MAP ( netuid ) --> excess_tao | Returns the excess TAO swapped (chain buys) into this subnet on the last block.
    #[pallet::storage]
    pub type SubnetExcessTao<T: Config> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultZeroTao<T>>;

    /// --- MAP ( netuid ) --> root_sell_tao | Returns the TAO received from root dividend sells on this subnet on the last block.
    #[pallet::storage]
    pub type SubnetRootSellTao<T: Config> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultZeroTao<T>>;

    /// --- MAP ( netuid ) --> alpha_supply_in_pool | Returns the amount of alpha in the pool.
    #[pallet::storage]
    pub type SubnetAlphaIn<T: Config> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( netuid ) --> alpha_supply_in_subnet | Returns the amount of alpha in the subnet.
    #[pallet::storage]
    pub type SubnetAlphaOut<T: Config> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;
    /// --- MAP ( netuid ) --> protocol_alpha | Returns the protocol-owned alpha cached for the subnet.
    #[pallet::storage]
    pub type SubnetProtocolAlpha<T: Config> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( cold ) --> Vec<hot> | Maps coldkey to hotkeys that stake to it
    #[pallet::storage]
    pub type StakingHotkeys<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, Vec<T::AccountId>, ValueQuery>;

    /// --- MAP ( cold ) --> Vec<hot> | Returns the vector of hotkeys controlled by this coldkey.
    #[pallet::storage]
    pub type OwnedHotkeys<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, Vec<T::AccountId>, ValueQuery>;

    /// Number of hotkeys controlled by this coldkey that are currently registered on the root subnet.
    #[pallet::storage]
    pub type RootRegisteredHotkeyCount<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    /// EMA state for each root-registered coldkey.
    #[pallet::storage]
    pub type RootRegisteredEma<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, EmaState, ValueQuery>;

    /// Fixed coldkey snapshot used by the current EMA sampling cycle.
    #[pallet::storage]
    pub type CurrentCycleMembers<T: Config> =
        StorageValue<_, BoundedVec<T::AccountId, ConstU32<64>>, ValueQuery>;

    /// Internal: the EMA value provider for the runtime.
    pub type EmaProviderOf<T> = <T as Config>::EmaValueProvider;

    /// Internal: provider-owned progress for the coldkey currently being sampled.
    pub type EmaProgressOf<T> = <EmaProviderOf<T> as EmaValueProvider<AccountIdOf<T>>>::Progress;

    /// Internal: in-flight sample for the current coldkey. Present only
    /// while `T::EmaValueProvider` has returned `SampleStep::Continue`.
    pub type InFlightEmaSampleOf<T> = InFlightEmaSample<AccountIdOf<T>, EmaProgressOf<T>>;

    /// Cursor and in-flight provider progress for the EMA sampling cycle.
    #[pallet::storage]
    pub type EmaSamplerState<T: Config> =
        StorageValue<_, (u32, Option<InFlightEmaSampleOf<T>>), ValueQuery>;

    /// --- DMAP ( cold, netuid )--> hot | Returns the hotkey a coldkey will autostake to with mining rewards.
    #[pallet::storage]
    pub type AutoStakeDestination<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        T::AccountId,
        OptionQuery,
    >;

    /// --- DMAP ( hot, netuid )--> Vec<cold> | Returns a list of coldkeys that are autostaking to a hotkey
    #[pallet::storage]
    pub type AutoStakeDestinationColdkeys<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        Vec<T::AccountId>,
        ValueQuery,
    >;

    /// The delay after an announcement before a coldkey swap can be performed.
    #[pallet::storage]
    pub type ColdkeySwapAnnouncementDelay<T: Config> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery, DefaultColdkeySwapAnnouncementDelay<T>>;

    /// The delay after the initial delay has passed before a new announcement can be made.
    #[pallet::storage]
    pub type ColdkeySwapReannouncementDelay<T: Config> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery, DefaultColdkeySwapReannouncementDelay<T>>;

    /// A map of the coldkey swap announcements from a coldkey
    /// to the block number the coldkey swap can be performed.
    #[pallet::storage]
    pub type ColdkeySwapAnnouncements<T: Config> =
        StorageMap<_, Twox64Concat, T::AccountId, (BlockNumberFor<T>, T::Hash), OptionQuery>;

    /// A map of the coldkey swap disputes from a coldkey to the
    /// block number the coldkey swap was disputed.
    #[pallet::storage]
    pub type ColdkeySwapDisputes<T: Config> =
        StorageMap<_, Twox64Concat, T::AccountId, BlockNumberFor<T>, OptionQuery>;

    /// --- DMAP ( hot, netuid ) --> alpha | Returns the total amount of alpha a hotkey owns.
    #[pallet::storage]
    pub type TotalHotkeyAlpha<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        AlphaBalance,
        ValueQuery,
        DefaultZeroAlpha<T>,
    >;

    /// --- DMAP ( hot, netuid ) --> alpha | Returns the total amount of alpha a hotkey owned in the last epoch.
    #[pallet::storage]
    pub type TotalHotkeyAlphaLastEpoch<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        AlphaBalance,
        ValueQuery,
        DefaultZeroAlpha<T>,
    >;

    /// DMAP ( hot, netuid ) --> total_alpha_shares | Returns the number of alpha shares for a hotkey on a subnet.
    #[pallet::storage]
    pub type TotalHotkeyShares<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        U64F64,
        ValueQuery,
        DefaultSharePoolZero<T>,
    >;

    /// --- NMAP ( hot, cold, netuid ) --> alpha | Returns the alpha shares for a hotkey, coldkey, netuid triplet.
    #[pallet::storage]
    pub type Alpha<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, T::AccountId>, // hot
            NMapKey<Blake2_128Concat, T::AccountId>, // cold
            NMapKey<Identity, NetUid>,               // subnet
        ),
        U64F64, // Shares
        ValueQuery,
    >;

    /// DMAP ( hot, netuid ) --> total_alpha_shares | Returns the number of alpha shares for a hotkey on a subnet, stores SafeFloat.
    #[pallet::storage]
    pub type TotalHotkeySharesV2<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId, // hot
        Identity,
        NetUid,    // subnet
        SafeFloat, // Hotkey shares in unlimited precision
        ValueQuery,
    >;

    /// --- NMAP ( hot, cold, netuid ) --> alpha | Returns the alpha shares for a hotkey, coldkey, netuid triplet, stores SafeFloat.
    #[pallet::storage]
    pub type AlphaV2<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, T::AccountId>, // hot
            NMapKey<Blake2_128Concat, T::AccountId>, // cold
            NMapKey<Identity, NetUid>,               // subnet
        ),
        SafeFloat, // Shares in unlimited precision
        ValueQuery,
    >;

    /// --- DMAP ( coldkey, netuid, hotkey ) --> LockState | Exponential lock per coldkey per subnet.
    #[pallet::storage]
    pub type Lock<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, T::AccountId>, // coldkey
            NMapKey<Identity, NetUid>,               // subnet
            NMapKey<Blake2_128Concat, T::AccountId>, // hotkey
        ),
        LockState,
        OptionQuery,
    >;

    /// --- NMAP ( netuid, hotkey, coldkey ) --> () | Reverse index for non-zero locks targeting this hotkey on this subnet.
    #[pallet::storage]
    pub type LockingColdkeys<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUid>,               // subnet
            NMapKey<Blake2_128Concat, T::AccountId>, // hotkey
            NMapKey<Blake2_128Concat, T::AccountId>, // coldkey
        ),
        (),
        OptionQuery,
    >;

    /// --- DMAP ( netuid, hotkey ) --> LockState | Total lock per hotkey per subnet.
    #[pallet::storage]
    pub type HotkeyLock<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid, // subnet
        Blake2_128Concat,
        T::AccountId, // hotkey
        LockState,    // Total merged lock
        OptionQuery,
    >;

    /// --- DMAP ( netuid, hotkey ) --> LockState | Total decaying non-owner lock per hotkey per subnet.
    #[pallet::storage]
    pub type DecayingHotkeyLock<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid, // subnet
        Blake2_128Concat,
        T::AccountId, // hotkey
        LockState,    // Total merged decaying lock
        OptionQuery,
    >;

    /// --- MAP ( netuid ) --> LockState | Total perpetual lock to the owner hotkey for a subnet.
    #[pallet::storage]
    pub type OwnerLock<T: Config> = StorageMap<_, Identity, NetUid, LockState, OptionQuery>;

    /// --- MAP ( netuid ) --> LockState | Total decaying lock to the owner hotkey for a subnet.
    #[pallet::storage]
    pub type DecayingOwnerLock<T: Config> = StorageMap<_, Identity, NetUid, LockState, OptionQuery>;

    /// --- DMAP ( coldkey, netuid ) --> false | When present and false, this coldkey's lock is perpetual.
    /// Missing entries mean the lock decays by default.
    #[pallet::storage]
    pub type DecayingLock<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, T::AccountId, Identity, NetUid, bool, OptionQuery>;

    /// Default value for owner cut auto-locking.
    #[pallet::type_value]
    pub fn DefaultOwnerCutAutoLockEnabled<T: Config>() -> bool {
        false
    }

    /// --- MAP ( netuid ) --> bool | Whether subnet owner cut should be auto-locked.
    /// Missing entries default to true, so auto-locking is enabled unless explicitly disabled.
    #[pallet::storage]
    pub type OwnerCutAutoLockEnabled<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultOwnerCutAutoLockEnabled<T>>;

    /// Default unlock timescale: 50% lock back in ~90 days.
    #[pallet::type_value]
    pub fn DefaultUnlockRate<T: Config>() -> u64 {
        934_866
    }

    /// Default maturity timescale: 50% conviction in ~90 days.
    #[pallet::type_value]
    pub fn DefaultMaturityRate<T: Config>() -> u64 {
        934_866
    }

    /// --- ITEM( maturity_rate ) | Decay timescale in blocks for lock conviction.
    #[pallet::storage]
    pub type MaturityRate<T: Config> = StorageValue<_, u64, ValueQuery, DefaultMaturityRate<T>>;

    /// --- ITEM( unlock_rate ) | Decay timescale in blocks for locked mass.
    #[pallet::storage]
    pub type UnlockRate<T: Config> = StorageValue<_, u64, ValueQuery, DefaultUnlockRate<T>>;

    /// Contains last Alpha storage map key to iterate (check first)
    #[pallet::storage]
    pub type AlphaMapLastKey<T: Config> =
        StorageValue<_, Option<Vec<u8>>, ValueQuery, DefaultAlphaIterationLastKey<T>>;

    /// Contains last AlphaV2 storage map key to iterate (check first)
    #[pallet::storage]
    pub type AlphaV2MapLastKey<T: Config> =
        StorageValue<_, Option<Vec<u8>>, ValueQuery, DefaultAlphaIterationLastKey<T>>;

    /// --- MAP ( netuid ) --> token_symbol | Returns the token symbol for a subnet.
    #[pallet::storage]
    pub type TokenSymbol<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<u8>, ValueQuery, DefaultUnicodeVecU8<T>>;

    /// --- MAP ( netuid ) --> subnet_tao_flow | Returns the TAO inflow-outflow balance.
    #[pallet::storage]
    pub type SubnetTaoFlow<T: Config> =
        StorageMap<_, Identity, NetUid, i64, ValueQuery, DefaultZeroI64<T>>;

    /// --- MAP ( netuid ) --> subnet_ema_tao_flow | Returns the EMA of TAO inflow-outflow balance.
    #[pallet::storage]
    pub type SubnetEmaTaoFlow<T: Config> =
        StorageMap<_, Identity, NetUid, (u64, I64F64), OptionQuery>;

    /// --- ITEM --> net_tao_flow_enabled | When true, emission shares use net flow (user - protocol). When false, uses gross user flow only.
    #[pallet::type_value]
    pub fn DefaultNetTaoFlowEnabled<T: Config>() -> bool {
        true
    }
    #[pallet::storage]
    pub type NetTaoFlowEnabled<T: Config> =
        StorageValue<_, bool, ValueQuery, DefaultNetTaoFlowEnabled<T>>;

    /// --- MAP ( netuid ) --> subnet_protocol_flow | Per-block accumulator for protocol cost (emission + chain buys - root sells).
    #[pallet::storage]
    pub type SubnetProtocolFlow<T: Config> =
        StorageMap<_, Identity, NetUid, i64, ValueQuery, DefaultZeroI64<T>>;

    /// --- MAP ( netuid ) --> subnet_ema_protocol_flow | EMA of protocol cost flow, same smoothing as SubnetEmaTaoFlow.
    #[pallet::storage]
    pub type SubnetEmaProtocolFlow<T: Config> =
        StorageMap<_, Identity, NetUid, (u64, I64F64), OptionQuery>;

    /// Default value for flow cutoff.
    #[pallet::type_value]
    pub fn DefaultFlowCutoff<T: Config>() -> I64F64 {
        I64F64::saturating_from_num(0)
    }
    #[pallet::storage]
    /// --- ITEM --> TAO Flow Cutoff
    pub type TaoFlowCutoff<T: Config> = StorageValue<_, I64F64, ValueQuery, DefaultFlowCutoff<T>>;
    #[pallet::type_value]
    /// Default value for flow normalization exponent.
    pub fn DefaultFlowNormExponent<T: Config>() -> U64F64 {
        U64F64::saturating_from_num(1)
    }
    #[pallet::storage]
    /// --- ITEM --> Flow Normalization Exponent (p)
    pub type FlowNormExponent<T: Config> =
        StorageValue<_, U64F64, ValueQuery, DefaultFlowNormExponent<T>>;
    #[pallet::type_value]
    /// Default value for flow EMA smoothing.
    pub fn DefaultFlowEmaSmoothingFactor<T: Config>() -> u64 {
        // Example values:
        //   half-life            factor value        i64 normalized (x 2^63)
        //   216000 (1 month) --> 0.000003209009576 ( 29_597_889_189_277)
        //    50400 (1 week)  --> 0.000013752825678 (126_847_427_788_335)
        29_597_889_189_277
    }
    #[pallet::type_value]
    /// Flow EMA smoothing half-life.
    pub fn FlowHalfLife<T: Config>() -> u64 {
        216_000
    }
    #[pallet::storage]
    /// --- ITEM --> Flow EMA smoothing factor (flow alpha), u64 normalized
    pub type FlowEmaSmoothingFactor<T: Config> =
        StorageValue<_, u64, ValueQuery, DefaultFlowEmaSmoothingFactor<T>>;

    /// ============================
    /// ==== Global Parameters =====
    /// ============================
    /// --- StorageItem Global Used Work.
    #[pallet::storage]
    pub type UsedWork<T: Config> = StorageMap<_, Identity, Vec<u8>, u64, ValueQuery>;

    /// --- ITEM( global_max_registrations_per_block )
    #[pallet::storage]
    pub type MaxRegistrationsPerBlock<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMaxRegistrationsPerBlock<T>>;

    /// --- ITEM( total_number_of_existing_networks )
    #[pallet::storage]
    pub type TotalNetworks<T> = StorageValue<_, u16, ValueQuery>;

    /// ITEM( network_immunity_period )
    #[pallet::storage]
    pub type NetworkImmunityPeriod<T> =
        StorageValue<_, u64, ValueQuery, DefaultNetworkImmunityPeriod<T>>;

    /// ITEM( start_call_delay )
    #[pallet::storage]
    pub type StartCallDelay<T: Config> = StorageValue<_, u64, ValueQuery, T::InitialStartCallDelay>;

    /// ITEM( min_network_lock_cost )
    #[pallet::storage]
    pub type NetworkMinLockCost<T> =
        StorageValue<_, TaoBalance, ValueQuery, DefaultNetworkMinLockCost<T>>;

    /// ITEM( last_network_lock_cost )
    #[pallet::storage]
    pub type NetworkLastLockCost<T> =
        StorageValue<_, TaoBalance, ValueQuery, DefaultNetworkMinLockCost<T>>;

    /// ITEM( network_lock_reduction_interval )
    #[pallet::storage]
    pub type NetworkLockReductionInterval<T> =
        StorageValue<_, u64, ValueQuery, DefaultNetworkLockReductionInterval<T>>;

    /// ITEM( subnet_owner_cut )
    #[pallet::storage]
    pub type SubnetOwnerCut<T> = StorageValue<_, u16, ValueQuery, DefaultSubnetOwnerCut<T>>;

    /// Default value for subnet owner cut enabled flag.
    #[pallet::type_value]
    pub fn DefaultOwnerCutEnabled<T: Config>() -> bool {
        true
    }

    /// --- MAP ( netuid ) --> owner_cut_enabled
    #[pallet::storage]
    pub type OwnerCutEnabled<T> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultOwnerCutEnabled<T>>;

    /// ITEM( network_rate_limit )
    #[pallet::storage]
    pub type NetworkRateLimit<T> = StorageValue<_, u64, ValueQuery, DefaultNetworkRateLimit<T>>;

    /// --- ITEM( nominator_min_required_stake ) --- Factor of DefaultMinStake in per-mill format.
    #[pallet::storage]
    pub type NominatorMinRequiredStake<T> = StorageValue<_, u64, ValueQuery, DefaultZeroU64<T>>;

    /// ITEM( weights_version_key_rate_limit ) --- Rate limit in tempos.
    #[pallet::storage]
    pub type WeightsVersionKeyRateLimit<T> =
        StorageValue<_, u64, ValueQuery, DefaultWeightsVersionKeyRateLimit<T>>;

    /// ============================
    /// ==== Rate Limiting =====
    /// ============================
    /// --- MAP ( RateLimitKey ) --> Block number in which the last rate limited operation occured
    #[pallet::storage]
    pub type LastRateLimitedBlock<T: Config> =
        StorageMap<_, Identity, RateLimitKey<T::AccountId>, u64, ValueQuery, DefaultZeroU64<T>>;

    /// ============================
    /// ==== Subnet Locks =====
    /// ============================
    /// --- MAP ( netuid ) --> transfer_toggle
    #[pallet::storage]
    pub type TransferToggle<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultTrue<T>>;

    /// --- MAP ( netuid ) --> total_subnet_locked
    #[pallet::storage]
    pub type SubnetLocked<T: Config> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultZeroTao<T>>;

    /// --- MAP ( netuid ) --> largest_locked
    #[pallet::storage]
    pub type LargestLocked<T: Config> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultZeroU64<T>>;

    /// =================
    /// ==== Tempos =====
    /// =================
    /// --- MAP ( netuid ) --> tempo
    #[pallet::storage]
    pub type Tempo<T> = StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultTempo<T>>;

    /// Lower bound for owner-set tempo. Also the fixed cooldown for `set_tempo`.
    pub const MIN_TEMPO: u16 = 360;
    /// Upper bound for owner-set tempo (≈ 7 days at 12 s/block).
    pub const MAX_TEMPO: u16 = 50_400;
    /// Lower bound for activity-cutoff factor (per-mille). 1_000 = one full tempo.
    pub const MIN_ACTIVITY_CUTOFF_FACTOR_MILLI: u32 = 1_000;
    /// Upper bound for activity-cutoff factor (per-mille). 50_000 = 50 tempos.
    pub const MAX_ACTIVITY_CUTOFF_FACTOR_MILLI: u32 = 50_000;
    /// Default activity-cutoff factor (per-mille). 13_889 ≈ legacy 5000-block cutoff
    /// at default tempo 360 (`13_889 * 360 / 1000 = 5_000`, exact via ceiling rounding).
    pub const INITIAL_ACTIVITY_CUTOFF_FACTOR_MILLI: u32 = 13_889;

    /// Default value for activity-cutoff factor (per-mille).
    #[pallet::type_value]
    pub fn DefaultActivityCutoffFactorMilli<T: Config>() -> u32 {
        INITIAL_ACTIVITY_CUTOFF_FACTOR_MILLI
    }

    /// --- MAP ( netuid ) --> last epoch attempt block (consumed slot).
    /// Drives normal-cadence scheduling and the admin freeze window.
    /// Advances on every `should_run_epoch == true` slot — including consistency-skipped slots —
    /// and on a successful `set_tempo` (cycle reset).
    #[pallet::storage]
    pub type LastEpochBlock<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultZeroU64<T>>;

    /// --- MAP ( netuid ) --> block at which a manually triggered epoch should fire.
    /// `0` means no trigger pending. Cleared after the triggered epoch runs.
    #[pallet::storage]
    pub type PendingEpochAt<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultZeroU64<T>>;

    /// --- MAP ( netuid ) --> monotonic epoch counter.
    /// Incremented by exactly one each time the subnet's epoch slot is consumed in `run_coinbase`
    #[pallet::storage]
    pub type SubnetEpochIndex<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultZeroU64<T>>;

    /// --- MAP ( netuid ) --> activity-cutoff factor in per-mille epochs (1/1000 granularity).
    /// Effective cutoff in blocks = `(factor × tempo) / 1000`, clamped to ≥ 1.
    #[pallet::storage]
    pub type ActivityCutoffFactorMilli<T> =
        StorageMap<_, Identity, NetUid, u32, ValueQuery, DefaultActivityCutoffFactorMilli<T>>;

    /// ============================
    /// ==== Subnet Parameters =====
    /// ============================
    /// --- MAP ( netuid ) --> block number of first emission
    #[pallet::storage]
    pub type FirstEmissionBlockNumber<T: Config> =
        StorageMap<_, Identity, NetUid, u64, OptionQuery>;

    /// --- MAP ( netuid ) --> subnet mechanism
    #[pallet::storage]
    pub type SubnetMechanism<T: Config> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultZeroU16<T>>;

    /// --- MAP ( netuid ) --> subnetwork_n (Number of UIDs in the network).
    #[pallet::storage]
    pub type SubnetworkN<T: Config> = StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultN<T>>;

    /// --- MAP ( netuid ) --> network_is_added
    #[pallet::storage]
    pub type NetworksAdded<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultNeworksAdded<T>>;

    /// --- DMAP ( hotkey, netuid ) --> bool
    #[pallet::storage]
    pub type IsNetworkMember<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        bool,
        ValueQuery,
        DefaultIsNetworkMember<T>,
    >;

    /// --- MAP ( netuid ) --> network_registration_allowed
    #[pallet::storage]
    pub type NetworkRegistrationAllowed<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultRegistrationAllowed<T>>;

    /// --- MAP ( netuid ) --> network_pow_allowed
    #[pallet::storage]
    pub type NetworkPowRegistrationAllowed<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultRegistrationAllowed<T>>;

    /// --- MAP ( netuid ) --> block_created
    #[pallet::storage]
    pub type NetworkRegisteredAt<T: Config> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultNetworkRegisteredAt<T>>;

    /// --- MAP ( netuid ) --> registered_subnet_counter
    ///
    /// Monotonic counter incremented on every successful `do_register_network`
    /// for a given netuid. Consumers that persist per-netuid state keyed by
    /// `(user, netuid)` (e.g. the staking precompile `AllowancesStorage`) can
    /// mix the current counter value into their storage key so that entries
    /// written under a previous registration of the same netuid become
    /// unreachable after the netuid is re-registered, without requiring
    /// unbounded storage iteration on deregistration.
    #[pallet::storage]
    pub type RegisteredSubnetCounter<T: Config> = StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    /// --- MAP ( netuid ) --> pending_server_emission
    #[pallet::storage]
    pub type PendingServerEmission<T> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( netuid ) --> pending_validator_emission
    #[pallet::storage]
    pub type PendingValidatorEmission<T> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( netuid ) --> pending_root_alpha_emission
    #[pallet::storage]
    pub type PendingRootAlphaDivs<T> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- MAP ( netuid ) --> pending_owner_cut
    #[pallet::storage]
    pub type PendingOwnerCut<T> =
        StorageMap<_, Identity, NetUid, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// Default miner-burned proportion.
    #[pallet::type_value]
    pub fn DefaultMinerBurned<T: Config>() -> U96F32 {
        U96F32::saturating_from_num(0.0)
    }
    /// --- MAP ( netuid ) --> miner_burned | Proportion (0..1) of this tempo's miner
    /// (incentive) emission that was withheld from miners during emission distribution
    /// because the recipient hotkey is owned by the subnet owner (immune key). Counts
    /// emission that is either recycled or burned, so the value is independent of the
    /// subnet's RecycleOrBurn configuration.
    #[pallet::storage]
    pub type MinerBurned<T> =
        StorageMap<_, Identity, NetUid, U96F32, ValueQuery, DefaultMinerBurned<T>>;

    /// --- MAP ( netuid ) --> blocks_since_last_step
    #[pallet::storage]
    pub type BlocksSinceLastStep<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultBlocksSinceLastStep<T>>;

    /// --- MAP ( netuid ) --> last_mechanism_step_block
    #[pallet::storage]
    pub type LastMechansimStepBlock<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultLastMechanismStepBlock<T>>;

    /// --- MAP ( netuid ) --> subnet_owner
    #[pallet::storage]
    pub type SubnetOwner<T: Config> =
        StorageMap<_, Identity, NetUid, T::AccountId, ValueQuery, DefaultSubnetOwner<T>>;

    /// --- MAP ( netuid ) --> subnet_owner_hotkey
    #[pallet::storage]
    pub type SubnetOwnerHotkey<T: Config> =
        StorageMap<_, Identity, NetUid, T::AccountId, ValueQuery, DefaultSubnetOwner<T>>;

    /// --- MAP ( netuid ) --> recycle_or_burn
    #[pallet::storage]
    pub type RecycleOrBurn<T: Config> =
        StorageMap<_, Identity, NetUid, RecycleOrBurnEnum, ValueQuery, DefaultRecycleOrBurn<T>>;

    /// --- MAP ( netuid ) --> serving_rate_limit
    #[pallet::storage]
    pub type ServingRateLimit<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultServingRateLimit<T>>;

    /// --- MAP ( netuid ) --> Rho
    #[pallet::storage]
    pub type Rho<T> = StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultRho<T>>;

    /// --- MAP ( netuid ) --> AlphaSigmoidSteepness
    #[pallet::storage]
    pub type AlphaSigmoidSteepness<T> =
        StorageMap<_, Identity, NetUid, i16, ValueQuery, DefaultAlphaSigmoidSteepness<T>>;

    /// --- MAP ( netuid ) --> Kappa
    #[pallet::storage]
    pub type Kappa<T> = StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultKappa<T>>;

    /// --- MAP ( netuid ) --> registrations_this_interval
    #[pallet::storage]
    pub type RegistrationsThisInterval<T: Config> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery>;

    /// --- MAP ( netuid ) --> pow_registrations_this_interval
    #[pallet::storage]
    pub type POWRegistrationsThisInterval<T: Config> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery>;

    /// --- MAP ( netuid ) --> burn_registrations_this_interval
    #[pallet::storage]
    pub type BurnRegistrationsThisInterval<T: Config> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery>;

    /// --- MAP ( netuid ) --> min_allowed_uids
    #[pallet::storage]
    pub type MinAllowedUids<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMinAllowedUids<T>>;

    /// --- MAP ( netuid ) --> max_allowed_uids
    #[pallet::storage]
    pub type MaxAllowedUids<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMaxAllowedUids<T>>;

    /// --- MAP ( netuid ) --> immunity_period
    #[pallet::storage]
    pub type ImmunityPeriod<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultImmunityPeriod<T>>;

    /// --- MAP ( netuid ) --> activity_cutoff
    // #[deprecated(note = "Replaced by `ActivityCutoffFactorMilli` (per-mille of `Tempo`).")]
    #[pallet::storage]
    pub type ActivityCutoff<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultActivityCutoff<T>>;
    #[pallet::type_value]
    /// Default maximum weights limit.
    pub fn DefaultMaxWeightsLimit<T: Config>() -> u16 {
        u16::MAX
    }

    /// --- MAP ( netuid ) --> max_weight_limit
    #[pallet::storage]
    pub type MaxWeightsLimit<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMaxWeightsLimit<T>>;

    /// --- MAP ( netuid ) --> weights_version_key
    #[pallet::storage]
    pub type WeightsVersionKey<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultWeightsVersionKey<T>>;

    /// --- MAP ( netuid ) --> min_allowed_weights
    #[pallet::storage]
    pub type MinAllowedWeights<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMinAllowedWeights<T>>;

    /// --- MAP ( netuid ) --> max_allowed_validators
    #[pallet::storage]
    pub type MaxAllowedValidators<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMaxAllowedValidators<T>>;

    /// --- MAP ( netuid ) --> adjustment_interval
    #[pallet::storage]
    pub type AdjustmentInterval<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultAdjustmentInterval<T>>;

    /// --- MAP ( netuid ) --> bonds_moving_average
    #[pallet::storage]
    pub type BondsMovingAverage<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultBondsMovingAverage<T>>;

    /// --- MAP ( netuid ) --> bonds_penalty
    #[pallet::storage]
    pub type BondsPenalty<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultBondsPenalty<T>>;

    /// --- MAP ( netuid ) --> bonds_reset
    #[pallet::storage]
    pub type BondsResetOn<T> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultBondsResetOn<T>>;

    /// --- MAP ( netuid ) --> weights_set_rate_limit
    #[pallet::storage]
    pub type WeightsSetRateLimit<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultWeightsSetRateLimit<T>>;

    /// --- MAP ( netuid ) --> validator_prune_len
    #[pallet::storage]
    pub type ValidatorPruneLen<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultValidatorPruneLen<T>>;

    /// --- MAP ( netuid ) --> scaling_law_power
    #[pallet::storage]
    pub type ScalingLawPower<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultScalingLawPower<T>>;

    /// --- MAP ( netuid ) --> target_registrations_this_interval
    #[pallet::storage]
    pub type TargetRegistrationsPerInterval<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultTargetRegistrationsPerInterval<T>>;

    /// --- MAP ( netuid ) --> adjustment_alpha
    #[pallet::storage]
    pub type AdjustmentAlpha<T: Config> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultAdjustmentAlpha<T>>;

    /// --- MAP ( netuid ) --> commit reveal v2 weights are enabled
    #[pallet::storage]
    pub type CommitRevealWeightsEnabled<T> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultCommitRevealWeightsEnabled<T>>;

    /// --- MAP ( netuid ) --> Burn
    #[pallet::storage]
    pub type Burn<T> = StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultBurn<T>>;

    /// --- MAP ( netuid ) --> Difficulty
    #[pallet::storage]
    pub type Difficulty<T> = StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultDifficulty<T>>;

    /// --- MAP ( netuid ) --> MinBurn
    #[pallet::storage]
    pub type MinBurn<T> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultMinBurn<T>>;

    /// --- MAP ( netuid ) --> MaxBurn
    #[pallet::storage]
    pub type MaxBurn<T> =
        StorageMap<_, Identity, NetUid, TaoBalance, ValueQuery, DefaultMaxBurn<T>>;

    /// --- MAP ( netuid ) --> MinDifficulty
    #[pallet::storage]
    pub type MinDifficulty<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultMinDifficulty<T>>;

    /// --- MAP ( netuid ) --> MaxDifficulty
    #[pallet::storage]
    pub type MaxDifficulty<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultMaxDifficulty<T>>;

    /// --- MAP ( netuid ) -->  Block at last adjustment.
    #[pallet::storage]
    pub type LastAdjustmentBlock<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultLastAdjustmentBlock<T>>;

    /// --- MAP ( netuid ) --> Registrations of this Block.
    #[pallet::storage]
    pub type RegistrationsThisBlock<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultRegistrationsThisBlock<T>>;

    /// --- MAP ( netuid ) --> Halving time of average moving price.
    #[pallet::storage]
    pub type EMAPriceHalvingBlocks<T> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultEMAPriceMovingBlocks<T>>;

    /// --- MAP ( netuid ) --> global_RAO_recycled_for_registration
    #[pallet::storage]
    pub type RAORecycledForRegistration<T> = StorageMap<
        _,
        Identity,
        NetUid,
        TaoBalance,
        ValueQuery,
        DefaultRAORecycledForRegistration<T>,
    >;

    /// --- ITEM ( max_epochs_per_block )
    #[pallet::storage]
    pub type MaxEpochsPerBlock<T> = StorageValue<_, u8, ValueQuery, DefaultMaxEpochsPerBlock<T>>;

    /// --- ITEM ( tx_rate_limit )
    #[pallet::storage]
    pub type TxRateLimit<T> = StorageValue<_, u64, ValueQuery, DefaultTxRateLimit<T>>;

    /// --- ITEM ( tx_delegate_take_rate_limit )
    #[pallet::storage]
    pub type TxDelegateTakeRateLimit<T> =
        StorageValue<_, u64, ValueQuery, DefaultTxDelegateTakeRateLimit<T>>;

    /// --- ITEM ( tx_childkey_take_rate_limit )
    #[pallet::storage]
    pub type TxChildkeyTakeRateLimit<T> =
        StorageValue<_, u64, ValueQuery, DefaultTxChildKeyTakeRateLimit<T>>;

    /// --- MAP ( netuid ) --> Whether or not Liquid Alpha is enabled
    #[pallet::storage]
    pub type LiquidAlphaOn<T> =
        StorageMap<_, Blake2_128Concat, NetUid, bool, ValueQuery, DefaultLiquidAlpha<T>>;

    /// --- MAP ( netuid ) --> Whether or not Yuma3 is enabled
    #[pallet::storage]
    pub type Yuma3On<T> =
        StorageMap<_, Blake2_128Concat, NetUid, bool, ValueQuery, DefaultYuma3<T>>;

    ///  MAP ( netuid ) --> (alpha_low, alpha_high)
    #[pallet::storage]
    pub type AlphaValues<T> =
        StorageMap<_, Identity, NetUid, (u16, u16), ValueQuery, DefaultAlphaValues<T>>;

    /// --- MAP ( netuid ) --> If subtoken trading enabled
    #[pallet::storage]
    pub type SubtokenEnabled<T> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultFalse<T>>;

    // =======================================
    // ==== VotingPower Storage  ====
    // =======================================

    #[pallet::type_value]
    /// Default VotingPower EMA alpha value (0.1 represented as u64 with 18 decimals)
    /// alpha = 0.1 means slow response, 10% weight to new values per epoch
    pub fn DefaultVotingPowerEmaAlpha<T: Config>() -> u64 {
        0_003_570_000_000_000_000 // 0.00357 * 10^18 = 2 weeks e-folding (time-constant) @ 361
        // blocks per tempo
        // After 2 weeks  -> EMA reaches 63.2% of a step change
        // After ~4 weeks -> 86.5%
        // After ~6 weeks -> 95%
    }

    #[pallet::storage]
    /// --- DMAP ( netuid, hotkey ) --> voting_power | EMA of stake for voting
    /// This tracks stake EMA updated every epoch when VotingPowerTrackingEnabled is true.
    /// Used by smart contracts to determine validator voting power for subnet governance.
    pub type VotingPower<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    #[pallet::storage]
    /// --- MAP ( netuid ) --> bool | Whether voting power tracking is enabled for this subnet.
    /// When enabled, VotingPower EMA is updated every epoch. Default is false.
    /// When disabled with disable_at_block set, tracking continues until that block.
    pub type VotingPowerTrackingEnabled<T: Config> =
        StorageMap<_, Identity, NetUid, bool, ValueQuery, DefaultFalse<T>>;

    #[pallet::storage]
    /// --- MAP ( netuid ) --> block_number | Block at which voting power tracking will be disabled.
    /// When set (non-zero), tracking continues until this block, then automatically disables
    /// and clears VotingPower entries for the subnet. Provides a 14-day grace period.
    pub type VotingPowerDisableAtBlock<T: Config> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery>;

    #[pallet::storage]
    /// --- MAP ( netuid ) --> u64 | EMA alpha value for voting power calculation.
    /// Higher alpha = faster response to stake changes.
    /// Stored as u64 with 18 decimal precision (1.0 = 10^18).
    /// Only settable by sudo/root.
    pub type VotingPowerEmaAlpha<T: Config> =
        StorageMap<_, Identity, NetUid, u64, ValueQuery, DefaultVotingPowerEmaAlpha<T>>;

    #[pallet::type_value]
    /// Default value for burn keys limit
    pub fn DefaultImmuneOwnerUidsLimit<T: Config>() -> u16 {
        1
    }

    /// Maximum value for burn keys limit
    #[pallet::type_value]
    pub fn MaxImmuneOwnerUidsLimit<T: Config>() -> u16 {
        10
    }

    /// Minimum value for burn keys limit
    #[pallet::type_value]
    pub fn MinImmuneOwnerUidsLimit<T: Config>() -> u16 {
        1
    }

    /// --- MAP ( netuid ) --> Burn key limit
    #[pallet::storage]
    pub type ImmuneOwnerUidsLimit<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultImmuneOwnerUidsLimit<T>>;

    /// =======================================
    /// ==== Subnetwork Consensus Storage  ====
    /// =======================================
    /// --- DMAP ( netuid ) --> stake_weight | weight for stake used in YC.
    #[pallet::storage]
    pub type StakeWeight<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<u16>, ValueQuery, EmptyU16Vec<T>>;

    /// --- DMAP ( netuid, hotkey ) --> uid
    #[pallet::storage]
    pub type Uids<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, u16, OptionQuery>;

    /// --- DMAP ( netuid, uid ) --> hotkey
    #[pallet::storage]
    pub type Keys<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Identity,
        u16,
        T::AccountId,
        ValueQuery,
        DefaultKey<T>,
    >;

    /// --- MAP ( netuid ) --> (hotkey, se, ve)
    #[pallet::storage]
    pub type LoadedEmission<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<(T::AccountId, u64, u64)>, OptionQuery>;

    /// --- MAP ( netuid ) --> active
    #[pallet::storage]
    pub type Active<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<bool>, ValueQuery, EmptyBoolVec<T>>;

    /// --- MAP ( netuid ) --> consensus
    #[pallet::storage]
    pub type Consensus<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<u16>, ValueQuery, EmptyU16Vec<T>>;

    /// --- MAP ( netuid ) --> incentive
    #[pallet::storage]
    pub type Incentive<T: Config> =
        StorageMap<_, Identity, NetUidStorageIndex, Vec<u16>, ValueQuery, EmptyU16Vec<T>>;

    /// --- MAP ( netuid ) --> dividends
    #[pallet::storage]
    pub type Dividends<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<u16>, ValueQuery, EmptyU16Vec<T>>;

    /// --- MAP ( netuid ) --> emission
    #[pallet::storage]
    pub type Emission<T: Config> = StorageMap<_, Identity, NetUid, Vec<AlphaBalance>, ValueQuery>;

    /// --- MAP ( netuid ) --> last_update
    #[pallet::storage]
    pub type LastUpdate<T: Config> =
        StorageMap<_, Identity, NetUidStorageIndex, Vec<u64>, ValueQuery, EmptyU64Vec<T>>;

    /// --- MAP ( netuid ) --> validator_trust
    #[pallet::storage]
    pub type ValidatorTrust<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<u16>, ValueQuery, EmptyU16Vec<T>>;

    /// --- MAP ( netuid ) --> validator_permit
    #[pallet::storage]
    pub type ValidatorPermit<T: Config> =
        StorageMap<_, Identity, NetUid, Vec<bool>, ValueQuery, EmptyBoolVec<T>>;

    /// --- DMAP ( netuid, uid ) --> weights
    #[pallet::storage]
    pub type Weights<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUidStorageIndex,
        Identity,
        u16,
        Vec<(u16, u16)>,
        ValueQuery,
        DefaultWeights<T>,
    >;

    /// --- DMAP ( netuid, uid ) --> bonds
    #[pallet::storage]
    pub type Bonds<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUidStorageIndex,
        Identity,
        u16,
        Vec<(u16, u16)>,
        ValueQuery,
        DefaultBonds<T>,
    >;

    /// --- DMAP ( netuid, uid ) --> block_at_registration
    #[pallet::storage]
    pub type BlockAtRegistration<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Identity,
        u16,
        u64,
        ValueQuery,
        DefaultBlockAtRegistration<T>,
    >;

    /// --- MAP ( netuid, hotkey ) --> axon_info
    #[pallet::storage]
    pub type Axons<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        AxonInfoOf,
        OptionQuery,
    >;

    /// --- MAP ( netuid, hotkey ) --> certificate
    #[pallet::storage]
    pub type NeuronCertificates<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        NeuronCertificateOf,
        OptionQuery,
    >;

    /// --- MAP ( netuid, hotkey ) --> prometheus_info
    #[pallet::storage]
    pub type Prometheus<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        PrometheusInfoOf,
        OptionQuery,
    >;

    /// --- MAP ( coldkey ) --> identity
    #[pallet::storage]
    pub type IdentitiesV2<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, ChainIdentityOfV2, OptionQuery>;

    /// --- MAP ( netuid ) --> SubnetIdentityOfV3
    #[pallet::storage]
    pub type SubnetIdentitiesV3<T: Config> =
        StorageMap<_, Blake2_128Concat, NetUid, SubnetIdentityOfV3, OptionQuery>;

    /// =================================
    /// ==== Axon / Promo Endpoints =====
    /// =================================
    /// --- NMAP ( hot, netuid, name ) --> last_block | Returns the last block of a transaction for a given key, netuid, and name.
    #[pallet::storage]
    pub type TransactionKeyLastBlock<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Blake2_128Concat, T::AccountId>, // hot
            NMapKey<Identity, NetUid>,               // netuid
            NMapKey<Identity, u16>,                  // extrinsic enum.
        ),
        u64,
        ValueQuery,
    >;

    /// --- MAP ( key ) --> last_block
    #[deprecated]
    #[pallet::storage]
    pub type LastTxBlock<T: Config> =
        StorageMap<_, Identity, T::AccountId, u64, ValueQuery, DefaultLastTxBlock<T>>;

    /// --- MAP ( key ) --> last_tx_block_childkey_take
    #[deprecated]
    #[pallet::storage]
    pub type LastTxBlockChildKeyTake<T: Config> =
        StorageMap<_, Identity, T::AccountId, u64, ValueQuery, DefaultLastTxBlock<T>>;

    /// --- MAP ( key ) --> last_tx_block_delegate_take
    #[deprecated]
    #[pallet::storage]
    pub type LastTxBlockDelegateTake<T: Config> =
        StorageMap<_, Identity, T::AccountId, u64, ValueQuery, DefaultLastTxBlock<T>>;

    /// ITEM( weights_min_stake )
    // FIXME: this storage is used interchangably for alpha/tao
    #[pallet::storage]
    pub type StakeThreshold<T> = StorageValue<_, u64, ValueQuery, DefaultStakeThreshold<T>>;

    /// --- MAP (netuid, who) --> VecDeque<(hash, commit_epoch, commit_block, _unused)>
    /// Stores a queue of commit-reveal-v2 commits for an account on a given netuid.
    #[pallet::storage]
    pub type WeightCommits<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        NetUidStorageIndex,
        Twox64Concat,
        T::AccountId,
        VecDeque<(H256, u64, u64, u64)>,
        OptionQuery,
    >;

    /// MAP (netuid, epoch) → VecDeque<(who, commit_block, ciphertext, reveal_round)>
    /// Stores a queue of weight commits for an account on a given subnet.
    #[pallet::storage]
    pub type TimelockedWeightCommits<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        NetUidStorageIndex,
        Twox64Concat,
        u64, // epoch key
        VecDeque<(
            T::AccountId,
            u64, // commit_block
            BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
            RoundNumber,
        )>,
        ValueQuery,
    >;

    /// MAP (netuid, epoch) → VecDeque<(who, ciphertext, reveal_round)>
    /// DEPRECATED for CRV3WeightCommitsV2
    #[pallet::storage]
    pub type CRV3WeightCommits<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        NetUidStorageIndex,
        Twox64Concat,
        u64, // epoch key
        VecDeque<(
            T::AccountId,
            BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
            RoundNumber,
        )>,
        ValueQuery,
    >;

    /// MAP (netuid, epoch) → VecDeque<(who, commit_block, ciphertext, reveal_round)>
    /// DEPRECATED for TimelockedWeightCommits
    #[pallet::storage]
    pub type CRV3WeightCommitsV2<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        NetUidStorageIndex,
        Twox64Concat,
        u64, // epoch key
        VecDeque<(
            T::AccountId,
            u64, // commit_block
            BoundedVec<u8, ConstU32<MAX_CRV3_COMMIT_SIZE_BYTES>>,
            RoundNumber,
        )>,
        ValueQuery,
    >;

    /// --- Map (netuid) --> Number of epochs allowed for commit reveal periods
    #[pallet::storage]
    pub type RevealPeriodEpochs<T: Config> =
        StorageMap<_, Twox64Concat, NetUid, u64, ValueQuery, DefaultRevealPeriodEpochs<T>>;

    /// --- Map (coldkey, hotkey) --> u64 the last block at which stake was added/removed.
    #[pallet::storage]
    pub type LastColdkeyHotkeyStakeBlock<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        T::AccountId,
        Twox64Concat,
        T::AccountId,
        u64,
        OptionQuery,
    >;

    #[pallet::storage] // --- MAP(netuid ) --> Root claim threshold
    pub type RootClaimableThreshold<T: Config> =
        StorageMap<_, Blake2_128Concat, NetUid, I96F32, ValueQuery, DefaultMinRootClaimAmount<T>>;

    #[pallet::storage] // --- MAP ( hot ) --> MAP(netuid ) --> claimable_dividends | Root claimable dividends.
    pub type RootClaimable<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        BTreeMap<NetUid, I96F32>,
        ValueQuery,
        DefaultRootClaimable<T>,
    >;

    // Already claimed root alpha.
    #[pallet::storage]
    pub type RootClaimed<T: Config> = StorageNMap<
        _,
        (
            NMapKey<Identity, NetUid>,               // subnet
            NMapKey<Blake2_128Concat, T::AccountId>, // hot
            NMapKey<Blake2_128Concat, T::AccountId>, // cold
        ),
        u128,
        ValueQuery,
    >;
    #[pallet::storage] // -- MAP ( cold ) --> root_claim_type enum
    pub type RootClaimType<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        RootClaimTypeEnum,
        ValueQuery,
        DefaultRootClaimType<T>,
    >;
    #[pallet::storage] // --- MAP ( u64 ) --> coldkey | Maps coldkeys that have stake to an index
    pub type StakingColdkeysByIndex<T: Config> =
        StorageMap<_, Identity, u64, T::AccountId, OptionQuery>;

    #[pallet::storage] // --- MAP ( coldkey ) --> index | Maps index that have stake to a coldkey
    pub type StakingColdkeys<T: Config> = StorageMap<_, Identity, T::AccountId, u64, OptionQuery>;

    #[pallet::storage] // --- Value --> num_staking_coldkeys
    pub type NumStakingColdkeys<T: Config> = StorageValue<_, u64, ValueQuery, DefaultZeroU64<T>>;
    #[pallet::storage] // --- Value --> num_root_claim | Number of coldkeys to claim each auto-claim.
    pub type NumRootClaim<T: Config> = StorageValue<_, u64, ValueQuery, DefaultNumRootClaim<T>>;

    /// =============================
    /// ==== EVM related storage ====
    /// =============================
    /// --- DMAP (netuid, uid) --> (H160, last_block_where_ownership_was_proven)
    #[pallet::storage]
    pub type AssociatedEvmAddress<T: Config> =
        StorageDoubleMap<_, Twox64Concat, NetUid, Twox64Concat, u16, (H160, u64), OptionQuery>;

    /// ========================
    /// ==== Subnet Leasing ====
    /// ========================
    /// --- MAP ( lease_id ) --> subnet lease | The subnet lease for a given lease id.
    #[pallet::storage]
    pub type SubnetLeases<T: Config> =
        StorageMap<_, Twox64Concat, LeaseId, SubnetLeaseOf<T>, OptionQuery>;

    /// --- DMAP ( lease_id, contributor ) --> shares | The shares of a contributor for a given lease.
    #[pallet::storage]
    pub type SubnetLeaseShares<T: Config> =
        StorageDoubleMap<_, Twox64Concat, LeaseId, Identity, T::AccountId, U64F64, ValueQuery>;

    /// --- MAP ( netuid ) --> lease_id | The lease id for a given netuid.
    #[pallet::storage]
    pub type SubnetUidToLeaseId<T: Config> =
        StorageMap<_, Twox64Concat, NetUid, LeaseId, OptionQuery>;

    /// --- ITEM ( next_lease_id ) | The next lease id.
    #[pallet::storage]
    pub type NextSubnetLeaseId<T: Config> = StorageValue<_, LeaseId, ValueQuery, ConstU32<0>>;

    /// --- MAP ( lease_id ) --> accumulated_dividends | The accumulated dividends for a given lease that needs to be distributed.
    #[pallet::storage]
    pub type AccumulatedLeaseDividends<T: Config> =
        StorageMap<_, Twox64Concat, LeaseId, AlphaBalance, ValueQuery, DefaultZeroAlpha<T>>;

    /// --- ITEM ( CommitRevealWeightsVersion )
    #[pallet::storage]
    pub type CommitRevealWeightsVersion<T> =
        StorageValue<_, u16, ValueQuery, DefaultCommitRevealWeightsVersion<T>>;

    /// ITEM( NetworkRegistrationStartBlock )
    #[pallet::storage]
    pub type NetworkRegistrationStartBlock<T> =
        StorageValue<_, u64, ValueQuery, DefaultNetworkRegistrationStartBlock<T>>;

    /// ITEM( TaoInRefundDeploymentBlock )
    #[pallet::storage]
    pub type TaoInRefundDeploymentBlock<T> =
        StorageValue<_, u64, ValueQuery, DefaultTaoInRefundDeploymentBlock>;

    /// --- MAP ( netuid ) --> minimum required number of non-immortal & non-immune UIDs
    #[pallet::storage]
    pub type MinNonImmuneUids<T: Config> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultMinNonImmuneUids<T>>;

    /// ============================
    /// ==== Subnet Mechanisms =====
    /// ============================
    /// -- ITEM (Default number of sub-subnets)
    #[pallet::type_value]
    pub fn DefaultMechanismCount<T: Config>() -> MechId {
        MechId::from(1)
    }

    /// -- ITEM (Maximum number of mechanisms)
    #[pallet::type_value]
    pub fn DefaultMaxMechanismCount<T: Config>() -> MechId {
        MechId::from(2)
    }

    /// ITEM( max_mechanism_count )
    #[pallet::storage]
    pub type MaxMechanismCount<T> =
        StorageValue<_, MechId, ValueQuery, DefaultMaxMechanismCount<T>>;

    /// -- ITEM (Rate limit for mechanism count updates)
    #[pallet::type_value]
    pub fn MechanismCountSetRateLimit<T: Config>() -> u64 {
        prod_or_fast!(7_200, 1)
    }

    /// -- ITEM (Rate limit for mechanism emission distribution updates)
    #[pallet::type_value]
    pub fn MechanismEmissionRateLimit<T: Config>() -> u64 {
        prod_or_fast!(7_200, 1)
    }

    /// --- MAP ( netuid ) --> Current number of subnet mechanisms
    #[pallet::storage]
    pub type MechanismCountCurrent<T: Config> =
        StorageMap<_, Twox64Concat, NetUid, MechId, ValueQuery, DefaultMechanismCount<T>>;

    /// --- MAP ( netuid ) --> Normalized vector of emission split proportion between subnet mechanisms
    #[pallet::storage]
    pub type MechanismEmissionSplit<T: Config> =
        StorageMap<_, Twox64Concat, NetUid, Vec<u16>, OptionQuery>;

    /// --- MAP ( netuid ) --> BurnHalfLife (blocks)
    #[pallet::storage]
    pub type BurnHalfLife<T> =
        StorageMap<_, Identity, NetUid, u16, ValueQuery, DefaultBurnHalfLife<T>>;

    /// --- MAP ( netuid ) --> BurnIncreaseMult
    #[pallet::storage]
    pub type BurnIncreaseMult<T> =
        StorageMap<_, Identity, NetUid, U64F64, ValueQuery, DefaultBurnIncreaseMult<T>>;

    /// --- MAP ( hotkey ) --> parent_delegation_enabled
    ///
    /// When `true`, this root validator allows auto parent delegation.
    /// Defaults to `true`; validators can opt out at any time
    /// by calling `set_auto_parent_delegation_enabled(false)`.
    #[pallet::storage]
    pub type AutoParentDelegationEnabled<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        bool,
        ValueQuery,
        DefaultAutoParentDelegationEnabled<T>, // default = true
    >;

    /// ==================
    /// ==== Genesis =====
    /// ==================
    /// --- Storage for migration run status
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> = StorageMap<_, Identity, Vec<u8>, bool, ValueQuery>;

    /// Default value for pending childkey cooldown (settable by root).
    /// Uses the same value as DefaultPendingCooldown for consistency.
    #[pallet::type_value]
    pub fn DefaultPendingChildKeyCooldown<T: Config>() -> u64 {
        DefaultPendingCooldown::<T>::get()
    }

    /// Storage value for pending childkey cooldown, settable by root.
    #[pallet::storage]
    pub type PendingChildKeyCooldown<T: Config> =
        StorageValue<_, u64, ValueQuery, DefaultPendingChildKeyCooldown<T>>;

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        /// Stakes record in genesis.
        pub stakes: Vec<(T::AccountId, Vec<(T::AccountId, (u64, u16))>)>,
        /// The total issued balance in genesis
        pub balances_issuance: TaoBalance,
        /// The delay before a subnet can call start
        pub start_call_delay: Option<u64>,
    }

    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                stakes: Default::default(),
                balances_issuance: TaoBalance::ZERO,
                start_call_delay: None,
            }
        }
    }

    // ---- Subtensor helper functions.
    impl<T: Config> Pallet<T> {
        /// Is the caller allowed to set weights
        pub fn check_weights_min_stake(hotkey: &T::AccountId, netuid: NetUid) -> bool {
            // Allow the subnet owner hotkey to set weights regardless of stake.
            if let Some(owner_uid) = Self::get_owner_uid(netuid)
                && Uids::<T>::get(netuid, hotkey) == Some(owner_uid)
            {
                return true;
            }

            // Blacklist weights transactions for low stake peers.
            let (total_stake, _, _) = Self::get_stake_weights_for_hotkey_on_subnet(hotkey, netuid);
            total_stake >= Self::get_stake_threshold()
        }
        /// Helper function to check if register is allowed
        pub fn checked_allowed_register(netuid: NetUid) -> bool {
            if netuid.is_root() {
                return false;
            }
            if !Self::if_subnet_exist(netuid) {
                return false;
            }
            if !Self::get_network_registration_allowed(netuid) {
                return false;
            }
            if Self::get_registrations_this_block(netuid)
                >= Self::get_max_registrations_per_block(netuid)
            {
                return false;
            }
            if Self::get_registrations_this_interval(netuid)
                >= Self::get_target_registrations_per_interval(netuid).saturating_mul(3)
            {
                return false;
            }
            true
        }

        /// Ensure subtoken enalbed
        pub fn ensure_subtoken_enabled(subnet: NetUid) -> Result<(), Error<T>> {
            ensure!(
                SubtokenEnabled::<T>::get(subnet),
                Error::<T>::SubtokenDisabled
            );
            Ok(())
        }
    }
}

use sp_std::vec;

// TODO: unravel this rats nest, for some reason rustc thinks this is unused even though it's
// used not 25 lines below
#[allow(unused)]
use sp_std::vec::Vec;
use subtensor_macros::freeze_struct;

#[derive(Clone)]
pub struct TaoBalanceReserve<T: Config>(PhantomData<T>);

impl<T: Config> TokenReserve<TaoBalance> for TaoBalanceReserve<T> {
    #![deny(clippy::expect_used)]
    fn reserve(netuid: NetUid) -> TaoBalance {
        SubnetTAO::<T>::get(netuid)
    }

    fn increase_provided(netuid: NetUid, tao: TaoBalance) {
        Pallet::<T>::increase_provided_tao_reserve(netuid, tao);
    }

    fn decrease_provided(netuid: NetUid, tao: TaoBalance) {
        Pallet::<T>::decrease_provided_tao_reserve(netuid, tao);
    }
}

#[derive(Clone)]
pub struct AlphaBalanceReserve<T: Config>(PhantomData<T>);

impl<T: Config> TokenReserve<AlphaBalance> for AlphaBalanceReserve<T> {
    #![deny(clippy::expect_used)]
    fn reserve(netuid: NetUid) -> AlphaBalance {
        SubnetAlphaIn::<T>::get(netuid)
    }

    fn increase_provided(netuid: NetUid, alpha: AlphaBalance) {
        Pallet::<T>::increase_provided_alpha_reserve(netuid, alpha);
    }

    fn decrease_provided(netuid: NetUid, alpha: AlphaBalance) {
        Pallet::<T>::decrease_provided_alpha_reserve(netuid, alpha);
    }
}

pub type GetAlphaForTao<T> =
    subtensor_swap_interface::GetAlphaForTao<TaoBalanceReserve<T>, AlphaBalanceReserve<T>>;
pub type GetTaoForAlpha<T> =
    subtensor_swap_interface::GetTaoForAlpha<AlphaBalanceReserve<T>, TaoBalanceReserve<T>>;

impl<T: Config + pallet_balances::Config<Balance = TaoBalance>>
    subtensor_runtime_common::SubnetInfo<T::AccountId> for Pallet<T>
{
    #![deny(clippy::expect_used)]
    fn exists(netuid: NetUid) -> bool {
        Self::if_subnet_exist(netuid)
    }

    fn mechanism(netuid: NetUid) -> u16 {
        SubnetMechanism::<T>::get(netuid)
    }

    fn is_owner(account_id: &T::AccountId, netuid: NetUid) -> bool {
        SubnetOwner::<T>::get(netuid) == *account_id
    }

    fn is_subtoken_enabled(netuid: NetUid) -> bool {
        SubtokenEnabled::<T>::get(netuid)
    }

    fn get_validator_trust(netuid: NetUid) -> Vec<u16> {
        ValidatorTrust::<T>::get(netuid)
    }

    fn get_validator_permit(netuid: NetUid) -> Vec<bool> {
        ValidatorPermit::<T>::get(netuid)
    }

    fn hotkey_of_uid(netuid: NetUid, uid: u16) -> Option<T::AccountId> {
        Keys::<T>::try_get(netuid, uid).ok()
    }
}

impl<T: Config + pallet_balances::Config<Balance = TaoBalance>>
    subtensor_runtime_common::BalanceOps<T::AccountId> for Pallet<T>
{
    #![deny(clippy::expect_used)]
    fn tao_balance(account_id: &T::AccountId) -> TaoBalance {
        pallet_balances::Pallet::<T>::free_balance(account_id).into()
    }

    fn alpha_balance(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> AlphaBalance {
        Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid)
    }

    fn increase_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<(), DispatchError> {
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Increse alpha out counter
        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(alpha);
        });

        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, alpha);

        Ok(())
    }

    fn decrease_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<(), DispatchError> {
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        ensure!(
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid) >= alpha,
            Error::<T>::InsufficientBalance
        );

        // Decrese alpha out counter
        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_sub(alpha);
        });

        Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, alpha);

        Ok(())
    }
}

/// Enum that defines types of rate limited operations for
/// storing last block when this operation occured
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub enum RateLimitKey<AccountId> {
    // The setting sn owner hotkey operation is rate limited per netuid
    #[codec(index = 0)]
    SetSNOwnerHotkey(NetUid),
    // Generic rate limit for subnet-owner hyperparameter updates (per netuid)
    #[codec(index = 1)]
    OwnerHyperparamUpdate(NetUid, Hyperparameter),
    // Subnet registration rate limit
    #[codec(index = 2)]
    NetworkLastRegistered,
    // Last tx block limit per account ID
    #[codec(index = 3)]
    LastTxBlock(AccountId),
    // Last tx block child key limit per account ID
    #[codec(index = 4)]
    LastTxBlockChildKeyTake(AccountId),
    // Last tx block delegate key limit per account ID
    #[codec(index = 5)]
    LastTxBlockDelegateTake(AccountId),
    // "Add stake and burn" rate limit
    #[codec(index = 6)]
    AddStakeBurn(NetUid),
}

pub trait ProxyInterface<AccountId> {
    fn add_lease_beneficiary_proxy(beneficiary: &AccountId, lease: &AccountId) -> DispatchResult;
    fn remove_lease_beneficiary_proxy(beneficiary: &AccountId, lease: &AccountId)
    -> DispatchResult;
}

impl<T> ProxyInterface<T> for () {
    fn add_lease_beneficiary_proxy(_: &T, _: &T) -> DispatchResult {
        Ok(())
    }

    fn remove_lease_beneficiary_proxy(_: &T, _: &T) -> DispatchResult {
        Ok(())
    }
}

/// Pallets that hold per-subnet commitments implement this to purge all state for `netuid`.
pub trait CommitmentsInterface {
    fn purge_netuid(netuid: NetUid);
}
