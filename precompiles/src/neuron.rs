use core::marker::PhantomData;

use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, PrecompileHandle};
use precompile_utils::{EvmResult, prelude::UnboundedBytes};
use sp_core::H256;
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable};
use sp_std::vec::Vec;

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct NeuronPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for NeuronPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
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
    const INDEX: u64 = 2052;
}

#[precompile_utils::precompile]
impl<R> NeuronPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
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
    #[precompile::public("setWeights(uint16,uint16[],uint16[],uint64)")]
    #[precompile::payable]
    pub fn set_weights(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        dests: Vec<u16>,
        weights: Vec<u16>,
        version_key: u64,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::set_weights {
            netuid: netuid.into(),
            dests,
            weights,
            version_key,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("commitWeights(uint16,bytes32)")]
    #[precompile::payable]
    pub fn commit_weights(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        commit_hash: H256,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::commit_weights {
            netuid: netuid.into(),
            commit_hash,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("revealWeights(uint16,uint16[],uint16[],uint16[],uint64)")]
    #[precompile::payable]
    pub fn reveal_weights(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        uids: Vec<u16>,
        values: Vec<u16>,
        salt: Vec<u16>,
        version_key: u64,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::reveal_weights {
            netuid: netuid.into(),
            uids,
            values,
            salt,
            version_key,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("burnedRegister(uint16,bytes32)")]
    #[precompile::payable]
    fn burned_register(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
    ) -> EvmResult<()> {
        let coldkey = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(hotkey.0);
        let call = pallet_subtensor::Call::<R>::burned_register {
            netuid: netuid.into(),
            hotkey,
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(coldkey))
    }

    #[precompile::public("registerLimit(uint16,bytes32,uint64)")]
    #[precompile::payable]
    fn register_limit(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
        limit_price: u64,
    ) -> EvmResult<()> {
        let coldkey = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(hotkey.0);
        let call = pallet_subtensor::Call::<R>::register_limit {
            netuid: netuid.into(),
            hotkey,
            limit_price,
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(coldkey))
    }

    #[precompile::public("serveAxon(uint16,uint32,uint128,uint16,uint8,uint8,uint8,uint8)")]
    #[precompile::payable]
    #[allow(clippy::too_many_arguments)]
    fn serve_axon(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
        protocol: u8,
        placeholder1: u8,
        placeholder2: u8,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::serve_axon {
            netuid: netuid.into(),
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public(
        "serveAxonTls(uint16,uint32,uint128,uint16,uint8,uint8,uint8,uint8,bytes)"
    )]
    #[precompile::payable]
    #[allow(clippy::too_many_arguments)]
    fn serve_axon_tls(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
        protocol: u8,
        placeholder1: u8,
        placeholder2: u8,
        certificate: UnboundedBytes,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::serve_axon_tls {
            netuid: netuid.into(),
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
            certificate: certificate.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    #[precompile::public("servePrometheus(uint16,uint32,uint128,uint16,uint8)")]
    #[precompile::payable]
    #[allow(clippy::too_many_arguments)]
    fn serve_prometheus(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::serve_prometheus {
            netuid: netuid.into(),
            version,
            ip,
            port,
            ip_type,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Runtime, System, addr_from_index, execute_precompile, mapped_account,
        new_test_ext, precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::{H160, H256, U256};
    use sp_runtime::traits::Hash;
    use subtensor_runtime_common::{AlphaBalance, NetUid, NetUidStorageIndex, TaoBalance, Token};

    const TEST_NETUID_U16: u16 = 1;
    const REGISTRATION_BURN: u64 = 1_000;
    const RESERVE: u64 = 1_000_000_000;
    const COLDKEY_BALANCE: u64 = 50_000;
    const TEMPO: u16 = 100;
    const REVEAL_PERIOD: u64 = 1;
    const VERSION_KEY: u64 = 0;
    const REGISTERED_UID: u16 = 0;
    const REVEAL_UIDS: [u16; 1] = [REGISTERED_UID];
    const REVEAL_VALUES: [u16; 1] = [5];
    const REVEAL_SALT: [u16; 1] = [9];
    const SERVE_VERSION: u32 = 0;
    const SERVE_IP: u128 = 1;
    const SERVE_PORT: u16 = 2;
    const SERVE_IP_TYPE: u8 = 4;
    const SERVE_PROTOCOL: u8 = 0;
    const SERVE_PLACEHOLDER1: u8 = 8;
    const SERVE_PLACEHOLDER2: u8 = 9;

    fn add_balance_to_coldkey_account(coldkey: &sp_core::crypto::AccountId32, tao: TaoBalance) {
        let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(tao);
        let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(coldkey, credit, tao).unwrap();
    }

    fn setup_registered_caller(caller: H160) -> (NetUid, AccountId) {
        let netuid = NetUid::from(TEST_NETUID_U16);
        let caller_account = mapped_account(caller);
        let caller_hotkey = H256::from_slice(caller_account.as_ref());

        pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, TEMPO);
        pallet_subtensor::Pallet::<Runtime>::set_network_registration_allowed(netuid, true);
        pallet_subtensor::Pallet::<Runtime>::set_burn(netuid, REGISTRATION_BURN.into());
        pallet_subtensor::Pallet::<Runtime>::set_max_allowed_uids(netuid, 4096);
        pallet_subtensor::Pallet::<Runtime>::set_weights_set_rate_limit(netuid, 0);
        pallet_subtensor::Pallet::<Runtime>::set_tempo_unchecked(netuid, TEMPO);
        pallet_subtensor::Pallet::<Runtime>::set_commit_reveal_weights_enabled(netuid, true);
        pallet_subtensor::Pallet::<Runtime>::set_reveal_period(netuid, REVEAL_PERIOD)
            .expect("reveal period setup should succeed");
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(RESERVE));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(RESERVE));
        add_balance_to_coldkey_account(&caller_account, COLDKEY_BALANCE.into());

        precompiles::<NeuronPrecompile<Runtime>>()
            .prepare_test(
                caller,
                addr_from_index(NeuronPrecompile::<Runtime>::INDEX),
                encode_with_selector(
                    selector_u32("burnedRegister(uint16,bytes32)"),
                    (TEST_NETUID_U16, caller_hotkey),
                ),
            )
            .execute_returns(());

        let registered_uid = pallet_subtensor::Pallet::<Runtime>::get_uid_for_net_and_hotkey(
            netuid,
            &caller_account,
        )
        .expect("caller should be registered on subnet");
        assert_eq!(registered_uid, REGISTERED_UID);

        (netuid, caller_account)
    }

    fn reveal_commit_hash(caller_account: &AccountId, netuid: NetUid) -> H256 {
        <Runtime as frame_system::Config>::Hashing::hash_of(&(
            caller_account.clone(),
            NetUidStorageIndex::from(netuid),
            REVEAL_UIDS.as_slice(),
            REVEAL_VALUES.as_slice(),
            REVEAL_SALT.as_slice(),
            VERSION_KEY,
        ))
    }

    #[test]
    fn neuron_precompile_burned_register_adds_a_new_uid_and_key() {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(TEST_NETUID_U16);
            let caller = addr_from_index(0x1234);
            let caller_account = mapped_account(caller);
            let hotkey_account = AccountId::from([0x42; 32]);
            let hotkey = H256::from_slice(hotkey_account.as_ref());

            pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, TEMPO);
            pallet_subtensor::Pallet::<Runtime>::set_network_registration_allowed(netuid, true);
            pallet_subtensor::Pallet::<Runtime>::set_burn(netuid, REGISTRATION_BURN.into());
            pallet_subtensor::Pallet::<Runtime>::set_max_allowed_uids(netuid, 4096);
            pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(RESERVE));
            pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(RESERVE));
            add_balance_to_coldkey_account(&caller_account, COLDKEY_BALANCE.into());

            let uid_before = pallet_subtensor::SubnetworkN::<Runtime>::get(netuid);
            let balance_before =
                pallet_subtensor::Pallet::<Runtime>::get_coldkey_balance(&caller_account).to_u64();

            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(NeuronPrecompile::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("burnedRegister(uint16,bytes32)"),
                        (TEST_NETUID_U16, hotkey),
                    ),
                )
                .execute_returns(());

            let uid_after = pallet_subtensor::SubnetworkN::<Runtime>::get(netuid);
            let registered_hotkey = pallet_subtensor::Keys::<Runtime>::get(netuid, uid_before);
            let owner = pallet_subtensor::Owner::<Runtime>::get(&hotkey_account);
            let balance_after =
                pallet_subtensor::Pallet::<Runtime>::get_coldkey_balance(&caller_account).to_u64();

            assert_eq!(uid_after, uid_before + 1);
            assert_eq!(registered_hotkey, hotkey_account);
            assert_eq!(owner, caller_account);
            assert!(balance_after < balance_before);
        });
    }

    #[test]
    fn neuron_precompile_commit_weights_respects_stake_threshold_and_stores_commit() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x2234);
            let (netuid, caller_account) = setup_registered_caller(caller);
            let commit_hash = reveal_commit_hash(&caller_account, netuid);
            let precompile_addr = addr_from_index(NeuronPrecompile::<Runtime>::INDEX);

            pallet_subtensor::Pallet::<Runtime>::set_stake_threshold(1);
            let rejected = execute_precompile(
                &precompiles::<NeuronPrecompile<Runtime>>(),
                precompile_addr,
                caller,
                encode_with_selector(
                    selector_u32("commitWeights(uint16,bytes32)"),
                    (TEST_NETUID_U16, commit_hash),
                ),
                U256::zero(),
            )
            .expect("commit weights should route to neuron precompile");
            assert!(rejected.is_err());

            pallet_subtensor::Pallet::<Runtime>::set_stake_threshold(0);
            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("commitWeights(uint16,bytes32)"),
                        (TEST_NETUID_U16, commit_hash),
                    ),
                )
                .execute_returns(());

            let commits = pallet_subtensor::WeightCommits::<Runtime>::get(
                NetUidStorageIndex::from(netuid),
                &caller_account,
            )
            .expect("weight commits should be stored after successful commit");
            assert_eq!(commits.len(), 1);
        });
    }

    #[test]
    fn neuron_precompile_reveal_weights_respects_stake_threshold_and_sets_weights() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x3234);
            let (netuid, caller_account) = setup_registered_caller(caller);
            let commit_hash = reveal_commit_hash(&caller_account, netuid);
            let precompile_addr = addr_from_index(NeuronPrecompile::<Runtime>::INDEX);

            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("commitWeights(uint16,bytes32)"),
                        (TEST_NETUID_U16, commit_hash),
                    ),
                )
                .execute_returns(());

            let commits = pallet_subtensor::WeightCommits::<Runtime>::get(
                NetUidStorageIndex::from(netuid),
                &caller_account,
            )
            .expect("weight commit should exist before reveal");
            // CR-v2 tuple layout: (hash, commit_epoch, commit_block, _unused).
            let (_, commit_epoch, _, _) = commits
                .front()
                .copied()
                .expect("weight commit queue should contain the committed hash");

            // Put the subnet into the exact epoch in which the commit is revealable:
            // `current_epoch == commit_epoch + reveal_period`. Pin `LastEpochBlock` and
            // `PendingEpochAt` so `should_run_epoch` is false and the look-ahead does
            // not advance past the reveal epoch.
            let reveal_epoch = commit_epoch.saturating_add(REVEAL_PERIOD);
            pallet_subtensor::SubnetEpochIndex::<Runtime>::insert(netuid, reveal_epoch);
            let cur_block = pallet_subtensor::Pallet::<Runtime>::get_current_block_as_u64();
            pallet_subtensor::LastEpochBlock::<Runtime>::insert(netuid, cur_block);
            pallet_subtensor::PendingEpochAt::<Runtime>::insert(netuid, 0u64);

            pallet_subtensor::Pallet::<Runtime>::set_stake_threshold(1);
            let rejected = execute_precompile(
                &precompiles::<NeuronPrecompile<Runtime>>(),
                precompile_addr,
                caller,
                encode_with_selector(
                    selector_u32("revealWeights(uint16,uint16[],uint16[],uint16[],uint64)"),
                    (
                        TEST_NETUID_U16,
                        REVEAL_UIDS.to_vec(),
                        REVEAL_VALUES.to_vec(),
                        REVEAL_SALT.to_vec(),
                        VERSION_KEY,
                    ),
                ),
                U256::zero(),
            )
            .expect("reveal weights should route to neuron precompile");
            assert!(rejected.is_err());

            pallet_subtensor::Pallet::<Runtime>::set_stake_threshold(0);
            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("revealWeights(uint16,uint16[],uint16[],uint16[],uint64)"),
                        (
                            TEST_NETUID_U16,
                            REVEAL_UIDS.to_vec(),
                            REVEAL_VALUES.to_vec(),
                            REVEAL_SALT.to_vec(),
                            VERSION_KEY,
                        ),
                    ),
                )
                .execute_returns(());

            assert!(
                pallet_subtensor::WeightCommits::<Runtime>::get(
                    NetUidStorageIndex::from(netuid),
                    &caller_account
                )
                .is_none()
            );

            let neuron_uid = pallet_subtensor::Pallet::<Runtime>::get_uid_for_net_and_hotkey(
                netuid,
                &caller_account,
            )
            .expect("caller should remain registered after reveal");
            let weights = pallet_subtensor::Weights::<Runtime>::get(
                NetUidStorageIndex::from(netuid),
                neuron_uid,
            );

            assert_eq!(weights.len(), 1);
            assert_eq!(weights[0].0, neuron_uid);
            assert!(weights[0].1 > 0);
        });
    }

    #[test]
    fn neuron_precompile_set_weights_sets_weights_when_commit_reveal_is_disabled() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x4234);
            let (netuid, caller_account) = setup_registered_caller(caller);
            let precompile_addr = addr_from_index(NeuronPrecompile::<Runtime>::INDEX);

            pallet_subtensor::Pallet::<Runtime>::set_commit_reveal_weights_enabled(netuid, false);

            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setWeights(uint16,uint16[],uint16[],uint64)"),
                        (
                            TEST_NETUID_U16,
                            vec![REGISTERED_UID],
                            vec![2_u16],
                            VERSION_KEY,
                        ),
                    ),
                )
                .execute_returns(());

            let neuron_uid = pallet_subtensor::Pallet::<Runtime>::get_uid_for_net_and_hotkey(
                netuid,
                &caller_account,
            )
            .expect("caller should remain registered after setting weights");
            let weights = pallet_subtensor::Weights::<Runtime>::get(
                NetUidStorageIndex::from(netuid),
                neuron_uid,
            );

            assert_eq!(weights.len(), 1);
            assert_eq!(weights[0].0, neuron_uid);
            assert!(weights[0].1 > 0);
        });
    }

    #[test]
    fn neuron_precompile_serve_axon_sets_axon_info() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5234);
            let (netuid, caller_account) = setup_registered_caller(caller);

            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(NeuronPrecompile::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "serveAxon(uint16,uint32,uint128,uint16,uint8,uint8,uint8,uint8)",
                        ),
                        (
                            TEST_NETUID_U16,
                            SERVE_VERSION,
                            SERVE_IP,
                            SERVE_PORT,
                            SERVE_IP_TYPE,
                            SERVE_PROTOCOL,
                            SERVE_PLACEHOLDER1,
                            SERVE_PLACEHOLDER2,
                        ),
                    ),
                )
                .execute_returns(());

            let axon = pallet_subtensor::Axons::<Runtime>::get(netuid, &caller_account)
                .expect("axon info should be stored");
            assert!(axon.block > 0);
            assert_eq!(axon.version, SERVE_VERSION);
            assert_eq!(axon.ip, SERVE_IP);
            assert_eq!(axon.port, SERVE_PORT);
            assert_eq!(axon.ip_type, SERVE_IP_TYPE);
            assert_eq!(axon.protocol, SERVE_PROTOCOL);
            assert_eq!(axon.placeholder1, SERVE_PLACEHOLDER1);
            assert_eq!(axon.placeholder2, SERVE_PLACEHOLDER2);
        });
    }

    #[test]
    fn neuron_precompile_dispatch_runs_subtensor_dispatch_extensions() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x5A34);
            let (netuid, caller_account) = setup_registered_caller(caller);
            let new_coldkey_hash =
                <Runtime as frame_system::Config>::Hashing::hash_of(&AccountId::new([0x99; 32]));

            pallet_subtensor::ColdkeySwapAnnouncements::<Runtime>::insert(
                &caller_account,
                (System::block_number(), new_coldkey_hash),
            );

            let rejected = execute_precompile(
                &precompiles::<NeuronPrecompile<Runtime>>(),
                addr_from_index(NeuronPrecompile::<Runtime>::INDEX),
                caller,
                encode_with_selector(
                    selector_u32("serveAxon(uint16,uint32,uint128,uint16,uint8,uint8,uint8,uint8)"),
                    (
                        TEST_NETUID_U16,
                        SERVE_VERSION,
                        SERVE_IP,
                        SERVE_PORT,
                        SERVE_IP_TYPE,
                        SERVE_PROTOCOL,
                        SERVE_PLACEHOLDER1,
                        SERVE_PLACEHOLDER2,
                    ),
                ),
                U256::zero(),
            )
            .expect("serve axon should route to neuron precompile");

            assert!(rejected.is_err());
            assert!(
                pallet_subtensor::Axons::<Runtime>::get(netuid, caller_account).is_none(),
                "dispatch extension rejection must happen before the call writes endpoint metadata"
            );
        });
    }

    #[test]
    fn neuron_precompile_serve_axon_tls_sets_axon_info_and_certificate() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x6234);
            let (netuid, caller_account) = setup_registered_caller(caller);
            let certificate: Vec<u8> = (1u8..=65).collect();

            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(NeuronPrecompile::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "serveAxonTls(uint16,uint32,uint128,uint16,uint8,uint8,uint8,uint8,bytes)",
                        ),
                        (
                            TEST_NETUID_U16,
                            SERVE_VERSION,
                            SERVE_IP,
                            SERVE_PORT,
                            SERVE_IP_TYPE,
                            SERVE_PROTOCOL,
                            SERVE_PLACEHOLDER1,
                            SERVE_PLACEHOLDER2,
                            UnboundedBytes::from(certificate.clone()),
                        ),
                    ),
                )
                .execute_returns(());

            let axon = pallet_subtensor::Axons::<Runtime>::get(netuid, &caller_account)
                .expect("axon info should be stored");
            assert!(axon.block > 0);
            assert_eq!(axon.version, SERVE_VERSION);
            assert_eq!(axon.ip, SERVE_IP);
            assert_eq!(axon.port, SERVE_PORT);
            assert_eq!(axon.ip_type, SERVE_IP_TYPE);
            assert_eq!(axon.protocol, SERVE_PROTOCOL);
            assert_eq!(axon.placeholder1, SERVE_PLACEHOLDER1);
            assert_eq!(axon.placeholder2, SERVE_PLACEHOLDER2);

            let stored_certificate =
                pallet_subtensor::NeuronCertificates::<Runtime>::get(netuid, caller_account)
                    .expect("certificate should be stored");
            assert_eq!(
                stored_certificate.public_key.into_inner(),
                certificate[1..].to_vec()
            );
        });
    }

    #[test]
    fn neuron_precompile_serve_prometheus_sets_prometheus_info() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7234);
            let (netuid, caller_account) = setup_registered_caller(caller);

            precompiles::<NeuronPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(NeuronPrecompile::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("servePrometheus(uint16,uint32,uint128,uint16,uint8)"),
                        (
                            TEST_NETUID_U16,
                            SERVE_VERSION,
                            SERVE_IP,
                            SERVE_PORT,
                            SERVE_IP_TYPE,
                        ),
                    ),
                )
                .execute_returns(());

            let prometheus = pallet_subtensor::Prometheus::<Runtime>::get(netuid, caller_account)
                .expect("prometheus info should be stored");
            assert!(prometheus.block > 0);
            assert_eq!(prometheus.version, SERVE_VERSION);
            assert_eq!(prometheus.ip, SERVE_IP);
            assert_eq!(prometheus.port, SERVE_PORT);
            assert_eq!(prometheus.ip_type, SERVE_IP_TYPE);
        });
    }
}
