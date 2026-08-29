#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use core::marker::PhantomData;

use crate::extensions::*;
pub use address_mapping::AddressMappingPrecompile;
pub use alpha::AlphaPrecompile;
pub use balance::BalancePrecompile;
pub use balance_transfer::BalanceTransferPrecompile;
pub use crowdloan::CrowdloanPrecompile;
pub use drand::DrandPrecompile;
pub use ed25519::Ed25519Verify;
pub use extensions::PrecompileExt;
use fp_evm::{ExitError, PrecompileFailure};
use frame_support::traits::IsSubType;
use frame_support::{
    dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo},
    pallet_prelude::Decode,
};
pub use leasing::LeasingPrecompile;
pub use metagraph::MetagraphPrecompile;
pub use neuron::NeuronPrecompile;
use pallet_admin_utils::PrecompileEnum;
use pallet_evm::{
    AddressMapping, IsPrecompileResult, Precompile, PrecompileHandle, PrecompileResult,
    PrecompileSet,
};
use pallet_evm_precompile_bn128::{Bn128Add, Bn128Mul, Bn128Pairing};
use pallet_evm_precompile_dispatch::Dispatch;
use pallet_evm_precompile_modexp::Modexp;
use pallet_evm_precompile_sha3fips::Sha3FIPS256;
use pallet_evm_precompile_simple::{ECRecover, ECRecoverPublicKey, Identity, Ripemd160, Sha256};
use pallet_subtensor_proxy as pallet_proxy;
pub use proxy::ProxyPrecompile;
pub use registry::PrecompileRegistry;
pub use runtime_configuration::RuntimeConfigurationPrecompile;
pub use scheduler::SchedulerPrecompile;
use sp_core::{H160, U256, crypto::ByteArray};
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable, StaticLookup};
pub use sr25519::Sr25519Verify;
pub use staking::{StakingPrecompile, StakingPrecompileV2};
pub use storage_query::StorageQueryPrecompile;
pub use subnet::SubnetPrecompile;
use subtensor_runtime_common::ProxyType;
pub use timestamp::TimestampPrecompile;
pub use uid_lookup::UidLookupPrecompile;
pub use voting_power::VotingPowerPrecompile;

mod address_mapping;
mod alpha;
mod balance;
mod balance_transfer;
mod crowdloan;
mod drand;
mod ed25519;
mod extensions;
mod leasing;
mod metagraph;
mod neuron;
mod proxy;
mod registry;
mod runtime_configuration;
mod scheduler;
mod sr25519;
mod staking;
mod storage_query;
mod subnet;
mod timestamp;
mod uid_lookup;
mod voting_power;

#[cfg(test)]
mod mock;

pub struct Precompiles<R>(PhantomData<R>);

impl<R> Default for Precompiles<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_balances::Config
        + pallet_admin_utils::Config
        + pallet_subtensor::Config
        + pallet_subtensor_swap::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_crowdloan::Config
        + pallet_drand::Config
        + pallet_evm_chain_id::Config
        + pallet_scheduler::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + pallet_timestamp::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray + Into<[u8; 32]>,
    R::Hash: AsRef<[u8]>,
    <R as pallet_timestamp::Config>::Moment: TryInto<u64>,
    pallet_scheduler::BlockNumberFor<R>: TryFrom<u64> + TryInto<u64>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + From<pallet_balances::Call<R>>
        + From<pallet_admin_utils::Call<R>>
        + From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as pallet_balances::Config>::Balance: Into<U256> + TryFrom<U256>,
    runtime_configuration::ProxyBalanceOf<R>: Into<U256>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<R> Precompiles<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_balances::Config
        + pallet_admin_utils::Config
        + pallet_subtensor::Config
        + pallet_subtensor_swap::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_crowdloan::Config
        + pallet_drand::Config
        + pallet_evm_chain_id::Config
        + pallet_scheduler::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + pallet_timestamp::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray + Into<[u8; 32]>,
    R::Hash: AsRef<[u8]>,
    <R as pallet_timestamp::Config>::Moment: TryInto<u64>,
    pallet_scheduler::BlockNumberFor<R>: TryFrom<u64> + TryInto<u64>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + From<pallet_balances::Call<R>>
        + From<pallet_admin_utils::Call<R>>
        + From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as pallet_balances::Config>::Balance: Into<U256> + TryFrom<U256>,
    runtime_configuration::ProxyBalanceOf<R>: Into<U256>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn used_addresses() -> [H160; 33] {
        [
            hash(1),
            hash(2),
            hash(3),
            hash(4),
            hash(5),
            hash(6),
            hash(7),
            hash(8),
            hash(9),
            hash(1024),
            hash(1025),
            hash(Ed25519Verify::<R::AccountId>::INDEX),
            hash(Sr25519Verify::<R::AccountId>::INDEX),
            hash(BalanceTransferPrecompile::<R>::INDEX),
            hash(StakingPrecompile::<R>::INDEX),
            hash(SubnetPrecompile::<R>::INDEX),
            hash(MetagraphPrecompile::<R>::INDEX),
            hash(NeuronPrecompile::<R>::INDEX),
            hash(StakingPrecompileV2::<R>::INDEX),
            hash(StorageQueryPrecompile::<R>::INDEX),
            hash(UidLookupPrecompile::<R>::INDEX),
            hash(AlphaPrecompile::<R>::INDEX),
            hash(CrowdloanPrecompile::<R>::INDEX),
            hash(LeasingPrecompile::<R>::INDEX),
            hash(VotingPowerPrecompile::<R>::INDEX),
            hash(ProxyPrecompile::<R>::INDEX),
            hash(AddressMappingPrecompile::<R>::INDEX),
            hash(BalancePrecompile::<R>::INDEX),
            hash(SchedulerPrecompile::<R>::INDEX),
            hash(DrandPrecompile::<R>::INDEX),
            hash(TimestampPrecompile::<R>::INDEX),
            hash(RuntimeConfigurationPrecompile::<R>::INDEX),
            hash(PrecompileRegistry::<R>::INDEX),
        ]
    }
}
impl<R> PrecompileSet for Precompiles<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_balances::Config
        + pallet_admin_utils::Config
        + pallet_subtensor::Config
        + pallet_subtensor_swap::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_crowdloan::Config
        + pallet_drand::Config
        + pallet_evm_chain_id::Config
        + pallet_scheduler::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + pallet_timestamp::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray + Into<[u8; 32]>,
    R::Hash: AsRef<[u8]>,
    <R as pallet_timestamp::Config>::Moment: TryInto<u64>,
    pallet_scheduler::BlockNumberFor<R>: TryFrom<u64> + TryInto<u64>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + From<pallet_balances::Call<R>>
        + From<pallet_admin_utils::Call<R>>
        + From<pallet_crowdloan::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>
        + Decode,
    <<R as frame_system::Config>::RuntimeCall as Dispatchable>::RuntimeOrigin:
        From<Option<pallet_evm::AccountIdOf<R>>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as pallet_balances::Config>::Balance: Into<U256> + TryFrom<U256>,
    runtime_configuration::ProxyBalanceOf<R>: Into<U256>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    fn execute(&self, handle: &mut impl PrecompileHandle) -> Option<PrecompileResult> {
        let code_address = handle.code_address();
        if !accepts_foreign_frame(code_address) && code_address != handle.context().address {
            return Some(Err(PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    "Cannot be called with DELEGATECALL or CALLCODE".into(),
                ),
            }));
        }

        match code_address {
            // Ethereum precompiles :
            a if a == hash(1) => Some(ECRecover::execute(handle)),
            a if a == hash(2) => Some(Sha256::execute(handle)),
            a if a == hash(3) => Some(Ripemd160::execute(handle)),
            a if a == hash(4) => Some(Identity::execute(handle)),
            a if a == hash(5) => Some(Modexp::execute(handle)),
            a if a == hash(6) => Some(Dispatch::<R>::execute(handle)),
            a if a == hash(7) => Some(Bn128Mul::execute(handle)),
            a if a == hash(8) => Some(Bn128Pairing::execute(handle)),
            a if a == hash(9) => Some(Bn128Add::execute(handle)),
            // Non-Frontier specific nor Ethereum precompiles :
            a if a == hash(1024) => Some(Sha3FIPS256::execute(handle)),
            a if a == hash(1025) => Some(ECRecoverPublicKey::execute(handle)),
            a if a == hash(Ed25519Verify::<R::AccountId>::INDEX) => {
                Some(Ed25519Verify::<R::AccountId>::execute(handle))
            }
            a if a == hash(Sr25519Verify::<R::AccountId>::INDEX) => {
                Some(Sr25519Verify::<R::AccountId>::execute(handle))
            }
            // Subtensor specific precompiles :
            a if a == hash(BalanceTransferPrecompile::<R>::INDEX) => {
                BalanceTransferPrecompile::<R>::try_execute::<R>(
                    handle,
                    PrecompileEnum::BalanceTransfer,
                )
            }
            a if a == hash(StakingPrecompile::<R>::INDEX) => {
                StakingPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Staking)
            }
            a if a == hash(StakingPrecompileV2::<R>::INDEX) => {
                StakingPrecompileV2::<R>::try_execute::<R>(handle, PrecompileEnum::Staking)
            }
            a if a == hash(SubnetPrecompile::<R>::INDEX) => {
                SubnetPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Subnet)
            }
            a if a == hash(MetagraphPrecompile::<R>::INDEX) => {
                MetagraphPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Metagraph)
            }
            a if a == hash(NeuronPrecompile::<R>::INDEX) => {
                NeuronPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Neuron)
            }
            a if a == hash(UidLookupPrecompile::<R>::INDEX) => {
                UidLookupPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::UidLookup)
            }
            a if a == hash(StorageQueryPrecompile::<R>::INDEX) => {
                Some(StorageQueryPrecompile::<R>::execute(handle))
            }
            a if a == hash(AlphaPrecompile::<R>::INDEX) => {
                AlphaPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Alpha)
            }
            a if a == hash(CrowdloanPrecompile::<R>::INDEX) => {
                CrowdloanPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Crowdloan)
            }
            a if a == hash(LeasingPrecompile::<R>::INDEX) => {
                LeasingPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Leasing)
            }
            a if a == hash(VotingPowerPrecompile::<R>::INDEX) => {
                VotingPowerPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::VotingPower)
            }
            a if a == hash(ProxyPrecompile::<R>::INDEX) => {
                ProxyPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Proxy)
            }
            a if a == hash(AddressMappingPrecompile::<R>::INDEX) => {
                AddressMappingPrecompile::<R>::try_execute::<R>(
                    handle,
                    PrecompileEnum::AddressMapping,
                )
            }
            a if a == hash(BalancePrecompile::<R>::INDEX) => {
                BalancePrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::AccountBalance)
            }
            a if a == hash(SchedulerPrecompile::<R>::INDEX) => {
                SchedulerPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Scheduler)
            }
            a if a == hash(DrandPrecompile::<R>::INDEX) => {
                DrandPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Drand)
            }
            a if a == hash(TimestampPrecompile::<R>::INDEX) => {
                TimestampPrecompile::<R>::try_execute::<R>(handle, PrecompileEnum::Timestamp)
            }
            a if a == hash(RuntimeConfigurationPrecompile::<R>::INDEX) => {
                RuntimeConfigurationPrecompile::<R>::try_execute::<R>(
                    handle,
                    PrecompileEnum::RuntimeConfiguration,
                )
            }
            a if a == hash(PrecompileRegistry::<R>::INDEX) => {
                PrecompileRegistry::<R>::try_execute::<R>(
                    handle,
                    PrecompileEnum::PrecompileRegistry,
                )
            }
            _ => None,
        }
    }

    fn is_precompile(&self, address: H160, _gas: u64) -> IsPrecompileResult {
        IsPrecompileResult::Answer {
            is_precompile: Self::used_addresses().contains(&address),
            extra_cost: 0,
        }
    }
}

fn hash(a: u64) -> H160 {
    H160::from_low_u64_be(a)
}

/// Stateless cryptographic precompiles may run in another contract's frame.
/// All other precompiles require a direct call (`code_address == context.address`).
fn accepts_foreign_frame(address: H160) -> bool {
    const PURE_MATH: &[u64] = &[1, 2, 3, 4, 5, 7, 8, 9, 1024, 1025];
    PURE_MATH.iter().any(|&index| address == hash(index))
        || address == hash(Ed25519Verify::<[u8; 32]>::INDEX)
        || address == hash(Sr25519Verify::<[u8; 32]>::INDEX)
}

/*
 *
 * This is used to parse a slice from bytes with PrecompileFailure as Error
 *
 */
fn parse_slice(data: &[u8], from: usize, to: usize) -> Result<&[u8], PrecompileFailure> {
    let maybe_slice = data.get(from..to);
    if let Some(slice) = maybe_slice {
        Ok(slice)
    } else {
        log::error!(
            "fail to get slice from data, {:?}, from {}, to {}",
            &data,
            from,
            to
        );
        Err(PrecompileFailure::Error {
            exit_status: ExitError::InvalidRange,
        })
    }
}

#[cfg(test)]
mod address_and_selector_tests {
    use super::*;
    use crate::mock::{Runtime, execute_precompile, new_test_ext, selector_u32};
    use alloc::collections::BTreeSet;
    use codec::Encode;
    use fp_evm::Context;
    use precompile_utils::testing::MockHandle;

    #[test]
    fn precompile_set_rejects_mismatched_frame() {
        new_test_ext().execute_with(|| {
            let code_address = hash(6);
            let caller = H160::from_low_u64_be(0xBEEF);
            let frame_address = H160::from_low_u64_be(0xDEAD);
            let mut handle = MockHandle::new(
                code_address,
                Context {
                    address: frame_address,
                    caller,
                    apparent_value: U256::zero(),
                },
            );

            assert_eq!(
                Precompiles::<Runtime>::new().execute(&mut handle),
                Some(Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other(
                        "Cannot be called with DELEGATECALL or CALLCODE".into(),
                    ),
                }))
            );
        });
    }

    #[test]
    fn precompile_set_accepts_matching_frame() {
        new_test_ext().execute_with(|| {
            let result = execute_precompile(
                &Precompiles::<Runtime>::new(),
                hash(6),
                H160::from_low_u64_be(0xBEEF),
                alloc::vec::Vec::new(),
                U256::zero(),
            );
            assert_ne!(
                result,
                Some(Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other(
                        "Cannot be called with DELEGATECALL or CALLCODE".into(),
                    ),
                }))
            );
        });
    }

    #[test]
    fn precompile_addresses_are_unique_and_new_addresses_are_locked() {
        let addresses = Precompiles::<Runtime>::used_addresses();
        assert_eq!(addresses.len(), BTreeSet::from_iter(addresses).len());
        assert_eq!(SchedulerPrecompile::<Runtime>::INDEX, 2063);
        assert_eq!(DrandPrecompile::<Runtime>::INDEX, 2064);
        assert_eq!(TimestampPrecompile::<Runtime>::INDEX, 2065);
        assert_eq!(RuntimeConfigurationPrecompile::<Runtime>::INDEX, 2066);
        assert_eq!(PrecompileRegistry::<Runtime>::INDEX, 2067);
    }

    #[test]
    fn precompile_enable_keys_preserve_existing_scale_indices() {
        let variants = [
            (PrecompileEnum::BalanceTransfer, 0),
            (PrecompileEnum::Staking, 1),
            (PrecompileEnum::Subnet, 2),
            (PrecompileEnum::Metagraph, 3),
            (PrecompileEnum::Neuron, 4),
            (PrecompileEnum::UidLookup, 5),
            (PrecompileEnum::Alpha, 6),
            (PrecompileEnum::Crowdloan, 7),
            (PrecompileEnum::Proxy, 8),
            (PrecompileEnum::Leasing, 9),
            (PrecompileEnum::AddressMapping, 10),
            (PrecompileEnum::VotingPower, 11),
            (PrecompileEnum::AccountBalance, 12),
            (PrecompileEnum::Scheduler, 13),
            (PrecompileEnum::Drand, 14),
            (PrecompileEnum::Timestamp, 15),
            (PrecompileEnum::RuntimeConfiguration, 16),
            (PrecompileEnum::PrecompileRegistry, 17),
        ];

        for (variant, expected_index) in variants {
            assert_eq!(variant.encode(), [expected_index]);
        }
    }

    #[test]
    fn new_precompile_selectors_are_locked() {
        for signature in [
            "getIncompleteSince()",
            "getScheduledCallCount(uint64)",
            "getScheduledCall(uint64,uint32)",
            "getRetry(uint64,uint32)",
            "getTaskAddress(bytes32)",
        ] {
            assert!(
                scheduler::SchedulerPrecompileCall::<Runtime>::supports_selector(selector_u32(
                    signature
                )),
                "missing Scheduler selector {signature}"
            );
        }
        for signature in [
            "getBeaconConfig()",
            "getPulse(uint64)",
            "getStoredRoundRange()",
            "getNextUnsignedAt()",
            "hasMigrationRun(bytes)",
        ] {
            assert!(
                drand::DrandPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Drand selector {signature}"
            );
        }
        for signature in ["getTimestamp()", "wasUpdatedThisBlock()"] {
            assert!(
                timestamp::TimestampPrecompileCall::<Runtime>::supports_selector(selector_u32(
                    signature
                )),
                "missing Timestamp selector {signature}"
            );
        }
        let runtime_configuration_signatures = [
            "getEvmChainId()",
            "getTransactionRateLimit()",
            "getSubtensorEconomicConstants()",
            "getSubtensorSubnetConstants()",
            "getSubtensorConsensusConstants()",
            "getSubtensorRegistrationConstants()",
            "getSubtensorDelegationConstants()",
            "getSubtensorRateLimitConstants()",
            "getSubtensorProtocolConstants()",
            "getSubtensorSystemAccounts()",
            "getBalancesConstants()",
            "getProxyConstants()",
            "getSchedulerConstants()",
            "getDrandConstants()",
            "getCrowdloanConstants()",
            "getSwapConstants()",
            "getTimestampConstants()",
            "getAdminConstants()",
        ];
        assert_eq!(
            runtime_configuration_signatures.len(),
            runtime_configuration_signatures
                .iter()
                .map(|signature| selector_u32(signature))
                .collect::<BTreeSet<_>>()
                .len(),
            "runtime-configuration selectors collide"
        );
        for signature in runtime_configuration_signatures {
            assert!(
                runtime_configuration::RuntimeConfigurationPrecompileCall::<Runtime>::supports_selector(
                    selector_u32(signature)
                ),
                "missing runtime-configuration selector {signature}"
            );
        }
        assert!(
            registry::PrecompileRegistryCall::<Runtime>::supports_selector(selector_u32(
                "getPrecompileStatus(address,bytes4)"
            ))
        );
    }

    #[test]
    fn added_domain_selectors_are_locked() {
        for signature in [
            "transferKeepAlive(bytes32,uint256)",
            "transferAll(bytes32,bool)",
        ] {
            assert!(
                balance_transfer::BalanceTransferPrecompileCall::<Runtime>::supports_selector(
                    selector_u32(signature)
                )
            );
        }
        assert!(
            balance::BalancePrecompileCall::<Runtime>::supports_selector(selector_u32(
                "upgradeAccounts(bytes32[])"
            ))
        );
        for signature in [
            "enableVotingPowerTracking(uint16)",
            "disableVotingPowerTracking(uint16)",
        ] {
            assert!(
                voting_power::VotingPowerPrecompileCall::<Runtime>::supports_selector(
                    selector_u32(signature)
                )
            );
        }
        assert!(
            leasing::LeasingPrecompileCall::<Runtime>::supports_selector(selector_u32(
                "startCall(uint16)"
            ))
        );
        assert!(
            crowdloan::CrowdloanPrecompileCall::<Runtime>::supports_selector(selector_u32(
                "setMaxContribution(uint32,bool,uint64)"
            ))
        );
        for signature in [
            "setRecycleOrBurn(uint16,uint8)",
            "setBurnHalfLife(uint16,uint16)",
            "setBurnIncreaseMultiplier(uint16,uint128)",
        ] {
            assert!(alpha::AlphaPrecompileCall::<Runtime>::supports_selector(
                selector_u32(signature)
            ));
        }
        for signature in [
            "announce(bytes32,bytes32)",
            "removeAnnouncement(bytes32,bytes32)",
            "rejectAnnouncement(bytes32,bytes32)",
            "setRealPaysFee(bytes32,bool)",
        ] {
            assert!(proxy::ProxyPrecompileCall::<Runtime>::supports_selector(
                selector_u32(signature)
            ));
        }
        for signature in [
            "setSubnetIdentity(uint16,string,string,string,string,string,string,string,string)",
            "updateSubnetSymbol(uint16,string)",
            "triggerEpoch(uint16)",
            "setBondsPenalty(uint16,uint16)",
            "setMaxAllowedUids(uint16,uint16)",
            "setMaxBurnV2(uint16,uint64)",
            "setMechanismCount(uint16,uint8)",
            "setMechanismEmissionSplit(uint16,bool,uint16[])",
            "setMinBurnV2(uint16,uint64)",
            "setOwnerCutEnabled(uint16,bool)",
            "setOwnerImmuneNeuronLimit(uint16,uint16)",
            "setTempo(uint16,uint16)",
            "trimToMaxAllowedUids(uint16,uint16)",
        ] {
            assert!(
                subnet::SubnetPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Subnet selector {signature}"
            );
        }
        for signature in [
            "decreaseTake(bytes32,uint16)",
            "increaseTake(bytes32,uint16)",
            "setChildkeyTake(bytes32,uint16,uint16)",
            "unstakeAll(bytes32)",
            "unstakeAllAlpha(bytes32)",
            "swapStake(bytes32,uint16,uint16,uint64)",
            "swapStakeLimit(bytes32,uint16,uint16,uint64,uint64,bool)",
            "moveStakeLimit(bytes32,bytes32,uint16,uint16,uint64,uint64,bool)",
            "recycleAlpha(bytes32,uint64,uint16)",
            "setColdkeyAutoStakeHotkey(uint16,bytes32)",
            "claimRoot(uint16[])",
            "claimRootWithHotkey(bytes32)",
            "setRootClaimThreshold(uint16,uint64)",
            "addStakeBurn(bytes32,uint16,uint64,bool,uint64)",
            "setAutoParentDelegationEnabled(bytes32,bool)",
            "transferStakeAndHotkey(bytes32,bytes32,bytes32,uint16,uint16,uint64)",
            "addCollateral(uint16,bytes32,uint64,uint64)",
            "setMinCollateral(uint16,bytes32,uint64)",
            "setMinChildkeyTakePerSubnet(uint16,uint16)",
            "setCollateralLockShare(uint16,uint16)",
            "setCollateralDrainRatio(uint16,uint128)",
        ] {
            assert!(
                staking::StakingPrecompileV2Call::<Runtime>::supports_selector(selector_u32(
                    signature
                )),
                "missing Staking V2 selector {signature}"
            );
        }
        for signature in [
            "setMechanismWeights(uint16,uint8,uint16[],uint16[],uint64)",
            "batchSetWeights(uint16[],uint16[][],uint16[][],uint64[])",
            "commitMechanismWeights(uint16,uint8,bytes32)",
            "batchCommitWeights(uint16[],bytes32[])",
            "revealMechanismWeights(uint16,uint8,uint16[],uint16[],uint16[],uint64)",
            "commitCrv3MechanismWeights(uint16,uint8,bytes,uint64)",
            "batchRevealWeights(uint16,uint16[][],uint16[][],uint16[][],uint64[])",
            "commitTimelockedWeights(uint16,bytes,uint64,uint16)",
            "commitTimelockedMechanismWeights(uint16,uint8,bytes,uint64,uint16)",
            "register(uint16,uint64,uint64,bytes,bytes32,bytes32)",
            "rootRegister(bytes32)",
            "swapHotkey(bytes32,bytes32,bool,uint16)",
            "swapHotkeyV2(bytes32,bytes32,bool,uint16,bool)",
            "setChildren(bytes32,uint16,uint64[],bytes32[])",
            "setIdentity(string,string,string,string,string,string,string)",
            "tryAssociateHotkey(bytes32)",
            "associateEvmKey(uint16,address,uint64,bytes)",
            "announceColdkeySwap(bytes32)",
            "executeAnnouncedColdkeySwap(bytes32)",
            "disputeColdkeySwap()",
            "clearColdkeySwapAnnouncement()",
        ] {
            assert!(
                neuron::NeuronPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Neuron selector {signature}"
            );
        }
    }

    #[test]
    fn state_reader_selectors_are_locked() {
        for signature in [
            "getDelegate(bytes32)",
            "getChildkeyTake(bytes32,uint16)",
            "getPendingChildKeys(bytes32,uint16)",
            "getChildKeys(bytes32,uint16)",
            "getParentKeys(bytes32,uint16)",
            "getPendingChildKeyCooldown()",
            "getTakeLimits()",
            "getMinChildkeyTakePerSubnet(uint16)",
            "getHotkeyOwner(bytes32)",
            "getOwnedHotkeys(bytes32)",
            "getStakingHotkeys(bytes32,uint64,uint16)",
            "getAutoStakeDestination(bytes32,uint16)",
            "getAutoStakeDestinationColdkeys(bytes32,uint16)",
            "getHotkeySuccessor(bytes32,uint16)",
            "getHotkeyRoot(bytes32,uint16)",
            "getColdkeySuccessor(bytes32)",
            "getColdkeyRoot(bytes32)",
            "getColdkeySwapStatus(bytes32)",
            "getColdkeySwapDelays()",
            "getLastHotkeySwapOnSubnet(bytes32,uint16)",
            "getStakeAccounting()",
            "getMinerCollateral(uint16,bytes32,bytes32)",
            "getColdkeyCollateral(uint16,bytes32)",
            "getCollateralConfig(uint16)",
            "getUnclaimedRootTaoByHotkey(bytes32,bytes32)",
            "getUnclaimedRootTaoBySubnet(bytes32,uint16,bytes32[])",
        ] {
            assert!(
                staking::StakingPrecompileV2Call::<Runtime>::supports_selector(selector_u32(
                    signature
                )),
                "missing Staking V2 reader selector {signature}"
            );
        }

        for signature in [
            "getRegisteredSubnetCounter(uint16)",
            "getSubnetDissolutionStatus(uint16)",
            "getSubnetMetadata(uint16)",
            "getSubnetCapacityConfig(uint16)",
            "getMechanismEmissionSplit(uint16)",
            "getBurnConfig(uint16)",
            "getGlobalNetworkLimits()",
            "getGlobalRateLimits()",
            "getGlobalProtocolConfig()",
        ] {
            assert!(
                subnet::SubnetPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Subnet reader selector {signature}"
            );
        }

        for signature in [
            "getEmissionAccounting(uint16,bytes32)",
            "getSubnetEconomicState(uint16)",
            "getSubnetFlowState(uint16)",
            "getEmissionGateConfig()",
            "getSwapState(uint16)",
            "hasSwapMigrationRun(bytes)",
        ] {
            assert!(
                alpha::AlphaPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Alpha reader selector {signature}"
            );
        }

        for signature in [
            "getUid(uint16,bytes32)",
            "isNetworkMember(bytes32,uint16)",
            "getWeights(uint16,uint16)",
            "getBonds(uint16,uint16)",
            "getBlockAtRegistration(uint16,uint16)",
            "getNeuronCertificate(uint16,bytes32)",
            "getPrometheus(uint16,bytes32)",
            "getChainIdentity(bytes32)",
            "getSubnetIdentity(uint16)",
            "getLoadedEmission(uint16)",
            "getTransactionKeyLastBlock(bytes32,uint16,uint16)",
            "getLegacyTransactionRateBlocks(bytes32)",
            "getWeightCommit(uint16,bytes32,uint32)",
            "getWeightCommitCount(uint16,bytes32)",
            "getTimelockedWeightCommit(uint16,uint64,uint32)",
            "getTimelockedWeightCommitCount(uint16,uint64)",
            "getLegacyTimelockedWeightCommit(uint8,uint16,uint64,uint32)",
            "getLegacyTimelockedWeightCommitCount(uint8,uint16,uint64)",
        ] {
            assert!(
                neuron::NeuronPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Neuron reader selector {signature}"
            );
        }

        for signature in [
            "getProxyDeposit(bytes32)",
            "getAnnouncements(bytes32)",
            "getLastCallResult(bytes32)",
            "isRealPaysFee(bytes32,bytes32)",
        ] {
            assert!(
                proxy::ProxyPrecompileCall::<Runtime>::supports_selector(selector_u32(signature)),
                "missing Proxy reader selector {signature}"
            );
        }

        for signature in ["getNextLeaseId()", "getAccumulatedLeaseDividends(uint32)"] {
            assert!(
                leasing::LeasingPrecompileCall::<Runtime>::supports_selector(selector_u32(
                    signature
                )),
                "missing Leasing reader selector {signature}"
            );
        }

        assert!(
            balance::BalancePrecompileCall::<Runtime>::supports_selector(selector_u32(
                "getTotalIssuance()"
            ))
        );
        assert!(
            uid_lookup::UidLookupPrecompileCall::<Runtime>::supports_selector(selector_u32(
                "getAssociatedEvmAddress(uint16,uint16)"
            ))
        );

        for (domain, selectors) in [
            (
                "Staking V2",
                staking::StakingPrecompileV2Call::<Runtime>::selectors(),
            ),
            (
                "Subnet",
                subnet::SubnetPrecompileCall::<Runtime>::selectors(),
            ),
            ("Alpha", alpha::AlphaPrecompileCall::<Runtime>::selectors()),
            (
                "Neuron",
                neuron::NeuronPrecompileCall::<Runtime>::selectors(),
            ),
            ("Proxy", proxy::ProxyPrecompileCall::<Runtime>::selectors()),
            (
                "Leasing",
                leasing::LeasingPrecompileCall::<Runtime>::selectors(),
            ),
            (
                "Balance",
                balance::BalancePrecompileCall::<Runtime>::selectors(),
            ),
            (
                "UID lookup",
                uid_lookup::UidLookupPrecompileCall::<Runtime>::selectors(),
            ),
        ] {
            let unique = selectors
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                unique.len(),
                selectors.len(),
                "{domain} contains a selector collision"
            );
        }
    }
}
