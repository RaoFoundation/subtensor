use alloc::string::String;
use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure, PrecompileHandle};
use pallet_subtensor::AxonInfo as SubtensorModuleAxonInfo;
use precompile_utils::{EvmResult, solidity::Codec};
use sp_core::{ByteArray, H256};
use subtensor_runtime_common::{NetUid, Token};

use crate::PrecompileExt;
use crate::PrecompileHandleExt;

pub struct MetagraphPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for MetagraphPrecompile<R>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]> + ByteArray,
{
    const INDEX: u64 = 2050;
}

#[precompile_utils::precompile]
impl<R> MetagraphPrecompile<R>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: ByteArray,
{
    #[precompile::public("getUidCount(uint16)")]
    #[precompile::view]
    fn get_uid_count(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::SubnetworkN::<R>::get(NetUid::from(
            netuid,
        )))
    }

    #[precompile::public("getStake(uint16,uint16)")]
    #[precompile::view]
    fn get_stake(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<u64> {
        // Keys + TotalHotkeyAlpha reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = pallet_subtensor::Pallet::<R>::get_hotkey_for_net_and_uid(netuid.into(), uid)
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::InvalidRange,
            })?;

        Ok(pallet_subtensor::Pallet::<R>::get_total_stake_for_hotkey(&hotkey).to_u64())
    }

    /// Deprecated: Rank is no longer computed. Always returns 0.
    #[precompile::public("getRank(uint16,uint16)")]
    #[precompile::view]
    fn get_rank(_: &mut impl PrecompileHandle, _netuid: u16, _uid: u16) -> EvmResult<u16> {
        Ok(0)
    }

    /// Deprecated: Trust is no longer computed. Always returns 0.
    #[precompile::public("getTrust(uint16,uint16)")]
    #[precompile::view]
    fn get_trust(_: &mut impl PrecompileHandle, _netuid: u16, _uid: u16) -> EvmResult<u16> {
        Ok(0)
    }

    #[precompile::public("getConsensus(uint16,uint16)")]
    #[precompile::view]
    fn get_consensus(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_consensus_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getIncentive(uint16,uint16)")]
    #[precompile::view]
    fn get_incentive(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_incentive_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getDividends(uint16,uint16)")]
    #[precompile::view]
    fn get_dividends(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_dividends_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getEmission(uint16,uint16)")]
    #[precompile::view]
    fn get_emission(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_emission_for_uid(netuid.into(), uid).into())
    }

    #[precompile::public("getVtrust(uint16,uint16)")]
    #[precompile::view]
    fn get_vtrust(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_validator_trust_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getValidatorStatus(uint16,uint16)")]
    #[precompile::view]
    fn get_validator_status(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        uid: u16,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_validator_permit_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getLastUpdate(uint16,uint16)")]
    #[precompile::view]
    fn get_last_update(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        uid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_last_update_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getIsActive(uint16,uint16)")]
    #[precompile::view]
    fn get_is_active(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::Pallet::<R>::get_active_for_uid(
            netuid.into(),
            uid,
        ))
    }

    #[precompile::public("getAxon(uint16,uint16)")]
    #[precompile::view]
    fn get_axon(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<AxonInfo> {
        // Keys + Axons reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = pallet_subtensor::Pallet::<R>::get_hotkey_for_net_and_uid(netuid.into(), uid)
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other("hotkey not found".into()),
            })?;

        Ok(pallet_subtensor::Pallet::<R>::get_axon_info(netuid.into(), &hotkey).into())
    }

    #[precompile::public("getHotkey(uint16,uint16)")]
    #[precompile::view]
    fn get_hotkey(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<H256> {
        handle.record_db_reads::<R>(1)?;
        pallet_subtensor::Pallet::<R>::get_hotkey_for_net_and_uid(netuid.into(), uid)
            .map(|acc| H256::from_slice(acc.as_slice()))
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::InvalidRange,
            })
    }

    #[precompile::public("getColdkey(uint16,uint16)")]
    #[precompile::view]
    fn get_coldkey(handle: &mut impl PrecompileHandle, netuid: u16, uid: u16) -> EvmResult<H256> {
        // Keys + Owner reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = pallet_subtensor::Pallet::<R>::get_hotkey_for_net_and_uid(netuid.into(), uid)
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::InvalidRange,
            })?;
        let coldkey = pallet_subtensor::Owner::<R>::get(&hotkey);

        Ok(H256::from_slice(coldkey.as_slice()))
    }
}

#[derive(Codec)]
struct AxonInfo {
    block: u64,
    version: u32,
    ip: u128,
    port: u16,
    ip_type: u8,
    protocol: u8,
}

impl From<SubtensorModuleAxonInfo> for AxonInfo {
    fn from(value: SubtensorModuleAxonInfo) -> Self {
        Self {
            block: value.block,
            version: value.version,
            ip: value.ip,
            port: value.port,
            ip_type: value.ip_type,
            protocol: value.protocol,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        Runtime, abi_word, addr_from_index, new_test_ext, precompiles, selector_u32,
    };
    use precompile_utils::solidity::{encode_return_value, encode_with_selector};
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::H256;
    use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex};

    const TEST_NETUID_U16: u16 = 1;
    const UID: u16 = 0;
    const EMISSION: u64 = 111;
    const VTRUST: u16 = 222;
    const LAST_UPDATE: u64 = 333;
    const AXON_BLOCK: u64 = 444;
    const AXON_VERSION: u32 = 555;
    const AXON_IP: u128 = 666;
    const AXON_PORT: u16 = 777;
    const AXON_IP_TYPE: u8 = 4;
    const AXON_PROTOCOL: u8 = 1;

    fn seed_metagraph_test_state() -> (
        NetUid,
        <Runtime as frame_system::Config>::AccountId,
        <Runtime as frame_system::Config>::AccountId,
        pallet_subtensor::AxonInfo,
    ) {
        let netuid = NetUid::from(TEST_NETUID_U16);
        let hotkey = <Runtime as frame_system::Config>::AccountId::from([0x11; 32]);
        let coldkey = <Runtime as frame_system::Config>::AccountId::from([0x22; 32]);

        let axon = pallet_subtensor::AxonInfo {
            block: AXON_BLOCK,
            version: AXON_VERSION,
            ip: AXON_IP,
            port: AXON_PORT,
            ip_type: AXON_IP_TYPE,
            protocol: AXON_PROTOCOL,
            placeholder1: 0,
            placeholder2: 0,
        };

        pallet_subtensor::SubnetworkN::<Runtime>::insert(netuid, 1);
        pallet_subtensor::Keys::<Runtime>::insert(netuid, UID, hotkey.clone());
        pallet_subtensor::Uids::<Runtime>::insert(netuid, &hotkey, UID);
        pallet_subtensor::Owner::<Runtime>::insert(&hotkey, coldkey.clone());
        pallet_subtensor::Emission::<Runtime>::insert(netuid, vec![AlphaBalance::from(EMISSION)]);
        pallet_subtensor::ValidatorTrust::<Runtime>::insert(netuid, vec![VTRUST]);
        pallet_subtensor::ValidatorPermit::<Runtime>::insert(netuid, vec![true]);
        pallet_subtensor::LastUpdate::<Runtime>::insert(
            NetUidStorageIndex::from(netuid),
            vec![LAST_UPDATE],
        );
        pallet_subtensor::Active::<Runtime>::insert(netuid, vec![true]);
        pallet_subtensor::Axons::<Runtime>::insert(netuid, &hotkey, axon.clone());

        (netuid, hotkey, coldkey, axon)
    }

    #[test]
    fn metagraph_precompile_matches_runtime_values() {
        new_test_ext().execute_with(|| {
            let (netuid, hotkey, coldkey, axon) = seed_metagraph_test_state();
            let precompiles = precompiles::<MetagraphPrecompile<Runtime>>();
            let caller = addr_from_index(1);
            let precompile_addr = addr_from_index(MetagraphPrecompile::<Runtime>::INDEX);

            let uid_count = pallet_subtensor::SubnetworkN::<Runtime>::get(netuid);
            let emission =
                pallet_subtensor::Pallet::<Runtime>::get_emission_for_uid(netuid, UID).to_u64();
            let vtrust =
                pallet_subtensor::Pallet::<Runtime>::get_validator_trust_for_uid(netuid, UID);
            let validator_status =
                pallet_subtensor::Pallet::<Runtime>::get_validator_permit_for_uid(netuid, UID);
            let last_update = pallet_subtensor::Pallet::<Runtime>::get_last_update_for_uid(
                NetUidStorageIndex::from(netuid),
                UID,
            );
            let is_active = pallet_subtensor::Pallet::<Runtime>::get_active_for_uid(netuid, UID);
            let runtime_axon = pallet_subtensor::Pallet::<Runtime>::get_axon_info(netuid, &hotkey);

            assert_eq!(uid_count, 1);
            assert_eq!(emission, EMISSION);
            assert_eq!(vtrust, VTRUST);
            assert!(validator_status);
            assert_eq!(last_update, LAST_UPDATE);
            assert!(is_active);
            assert_eq!(runtime_axon, axon);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(selector_u32("getUidCount(uint16)"), (TEST_NETUID_U16,)),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(uid_count.into()));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getAxon(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(encode_return_value((
                    runtime_axon.block,
                    runtime_axon.version,
                    runtime_axon.ip,
                    runtime_axon.port,
                    runtime_axon.ip_type,
                    runtime_axon.protocol,
                )));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getEmission(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(emission.into()));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getVtrust(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(vtrust.into()));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getValidatorStatus(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word((validator_status as u8).into()));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getLastUpdate(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word(last_update.into()));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getIsActive(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(abi_word((is_active as u8).into()));
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getHotkey(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(H256::from_slice(hotkey.as_ref()).as_bytes().to_vec());
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getColdkey(uint16,uint16)"),
                        (TEST_NETUID_U16, UID),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(H256::from_slice(coldkey.as_ref()).as_bytes().to_vec());
        });
    }
}
