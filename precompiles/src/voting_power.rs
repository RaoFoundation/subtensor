use core::marker::PhantomData;

use fp_evm::PrecompileHandle;
use frame_support::{
    dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo},
    traits::IsSubType,
};
use frame_system::RawOrigin;
use pallet_evm::AddressMapping;
use precompile_utils::EvmResult;
use sp_core::{ByteArray, H256, U256};
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable};
use subtensor_runtime_common::NetUid;

use crate::PrecompileExt;
use crate::PrecompileHandleExt;

/// VotingPower precompile for smart contract access to validator voting power.
///
/// This precompile allows smart contracts to query voting power for validators,
/// enabling on-chain governance decisions like slashing and spending.
pub struct VotingPowerPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for VotingPowerPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_subtensor::Config
        + pallet_evm::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2061;
}

#[precompile_utils::precompile]
impl<R> VotingPowerPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_subtensor::Config
        + pallet_evm::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + ByteArray,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    /// Get voting power for a hotkey on a subnet.
    ///
    /// Returns the EMA of stake for the hotkey, which represents its voting power.
    /// Returns 0 if:
    /// - The hotkey has no voting power entry
    /// - Voting power tracking is not enabled for the subnet
    /// - The hotkey is not registered on the subnet
    ///
    /// # Arguments
    /// * `netuid` - The subnet identifier (u16)
    /// * `hotkey` - The hotkey account ID (bytes32)
    ///
    /// # Returns
    /// * `u256` - The voting power value (in RAO, same precision as stake)
    #[precompile::public("getVotingPower(uint16,bytes32)")]
    #[precompile::view]
    fn get_voting_power(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
    ) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        let hotkey = R::AccountId::from(hotkey.0);
        let voting_power = pallet_subtensor::VotingPower::<R>::get(NetUid::from(netuid), &hotkey);
        Ok(U256::from(voting_power))
    }

    /// Check if voting power tracking is enabled for a subnet.
    ///
    /// # Arguments
    /// * `netuid` - The subnet identifier (u16)
    ///
    /// # Returns
    /// * `bool` - True if voting power tracking is enabled
    #[precompile::public("isVotingPowerTrackingEnabled(uint16)")]
    #[precompile::view]
    fn is_voting_power_tracking_enabled(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::VotingPowerTrackingEnabled::<R>::get(
            NetUid::from(netuid),
        ))
    }

    /// Get the block at which voting power tracking will be disabled.
    ///
    /// Returns 0 if not scheduled for disabling.
    /// When non-zero, tracking continues until this block, then stops.
    ///
    /// # Arguments
    /// * `netuid` - The subnet identifier (u16)
    ///
    /// # Returns
    /// * `u64` - The block number at which tracking will be disabled (0 if not scheduled)
    #[precompile::public("getVotingPowerDisableAtBlock(uint16)")]
    #[precompile::view]
    fn get_voting_power_disable_at_block(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::VotingPowerDisableAtBlock::<R>::get(
            NetUid::from(netuid),
        ))
    }

    /// Get the EMA alpha value for voting power calculation on a subnet.
    ///
    /// Alpha is stored with 18 decimal precision (1.0 = 10^18).
    /// Higher alpha = faster response to stake changes.
    ///
    /// # Arguments
    /// * `netuid` - The subnet identifier (u16)
    ///
    /// # Returns
    /// * `u64` - The alpha value (with 18 decimal precision)
    #[precompile::public("getVotingPowerEmaAlpha(uint16)")]
    #[precompile::view]
    fn get_voting_power_ema_alpha(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::VotingPowerEmaAlpha::<R>::get(
            NetUid::from(netuid),
        ))
    }

    /// Get total voting power for a subnet.
    ///
    /// Returns the sum of all voting power for all validators on the subnet.
    /// Useful for calculating voting thresholds (e.g., 51% quorum).
    ///
    /// # Arguments
    /// * `netuid` - The subnet identifier (u16)
    ///
    /// # Returns
    /// * `u256` - The total voting power across all validators
    #[precompile::public("getTotalVotingPower(uint16)")]
    #[precompile::view]
    fn get_total_voting_power(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(U256::from(pallet_subtensor::TotalVotingPower::<R>::get(
            NetUid::from(netuid),
        )))
    }

    #[precompile::public("enableVotingPowerTracking(uint16)")]
    fn enable_voting_power_tracking(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<()> {
        let caller = handle.caller_account_id::<R>();
        let call = pallet_subtensor::Call::<R>::enable_voting_power_tracking {
            netuid: NetUid::from(netuid),
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }

    #[precompile::public("disableVotingPowerTracking(uint16)")]
    fn disable_voting_power_tracking(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<()> {
        let caller = handle.caller_account_id::<R>();
        let call = pallet_subtensor::Call::<R>::disable_voting_power_tracking {
            netuid: NetUid::from(netuid),
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Runtime, addr_from_index, assert_static_call, new_test_ext, precompiles,
        selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use sp_core::{H160, H256, U256};

    const TEST_NETUID_U16: u16 = 1;
    const TEST_TEMPO: u16 = 100;
    const DEFAULT_ALPHA: u64 = 3_570_000_000_000_000;

    fn setup_subnet() -> NetUid {
        let netuid = NetUid::from(TEST_NETUID_U16);
        pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, TEST_TEMPO);
        netuid
    }

    fn hotkey(byte: u8) -> AccountId {
        AccountId::from([byte; 32])
    }

    fn assert_voting_power_call(
        caller: H160,
        signature: &str,
        args: impl precompile_utils::solidity::Codec,
        expected: U256,
    ) {
        assert_static_call(
            &precompiles::<VotingPowerPrecompile<Runtime>>(),
            caller,
            addr_from_index(VotingPowerPrecompile::<Runtime>::INDEX),
            encode_with_selector(selector_u32(signature), args),
            expected,
        );
    }

    #[test]
    fn voting_power_precompile_returns_default_zero_values() {
        new_test_ext().execute_with(|| {
            let netuid = setup_subnet();
            let caller = addr_from_index(0x6001);
            let existing_hotkey = hotkey(0x11);
            let unknown_hotkey = hotkey(0x22);

            assert!(!pallet_subtensor::VotingPowerTrackingEnabled::<Runtime>::get(netuid));
            assert_voting_power_call(
                caller,
                "isVotingPowerTrackingEnabled(uint16)",
                (TEST_NETUID_U16,),
                U256::zero(),
            );
            assert_voting_power_call(
                caller,
                "getVotingPowerDisableAtBlock(uint16)",
                (TEST_NETUID_U16,),
                U256::zero(),
            );
            assert_voting_power_call(
                caller,
                "getVotingPowerEmaAlpha(uint16)",
                (TEST_NETUID_U16,),
                U256::from(DEFAULT_ALPHA),
            );
            assert_voting_power_call(
                caller,
                "getVotingPower(uint16,bytes32)",
                (
                    TEST_NETUID_U16,
                    H256::from_slice(existing_hotkey.as_slice()),
                ),
                U256::zero(),
            );
            assert_voting_power_call(
                caller,
                "getVotingPower(uint16,bytes32)",
                (TEST_NETUID_U16, H256::from_slice(unknown_hotkey.as_slice())),
                U256::zero(),
            );
            assert_voting_power_call(
                caller,
                "getTotalVotingPower(uint16)",
                (TEST_NETUID_U16,),
                U256::zero(),
            );
        });
    }

    #[test]
    fn voting_power_precompile_reads_enabled_tracking_and_stored_power() {
        new_test_ext().execute_with(|| {
            let netuid = setup_subnet();
            let caller = addr_from_index(0x6002);
            let first_hotkey = hotkey(0x33);
            let second_hotkey = hotkey(0x44);

            pallet_subtensor::VotingPowerTrackingEnabled::<Runtime>::insert(netuid, true);
            pallet_subtensor::VotingPower::<Runtime>::insert(netuid, &first_hotkey, 123_u64);
            pallet_subtensor::VotingPower::<Runtime>::insert(netuid, &second_hotkey, 456_u64);
            pallet_subtensor::TotalVotingPower::<Runtime>::insert(netuid, 579_u64);

            assert_voting_power_call(
                caller,
                "isVotingPowerTrackingEnabled(uint16)",
                (TEST_NETUID_U16,),
                U256::one(),
            );
            assert_voting_power_call(
                caller,
                "getVotingPowerDisableAtBlock(uint16)",
                (TEST_NETUID_U16,),
                U256::zero(),
            );
            assert_voting_power_call(
                caller,
                "getVotingPower(uint16,bytes32)",
                (TEST_NETUID_U16, H256::from_slice(first_hotkey.as_slice())),
                U256::from(123_u64),
            );
            assert_voting_power_call(
                caller,
                "getTotalVotingPower(uint16)",
                (TEST_NETUID_U16,),
                U256::from(579_u64),
            );
        });
    }
}
