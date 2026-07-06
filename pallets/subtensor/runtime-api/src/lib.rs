#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use codec::Compact;
use pallet_subtensor::rpc_info::{
    delegate_info::DelegateInfo,
    dynamic_info::DynamicInfo,
    metagraph::{Metagraph, SelectiveMetagraph},
    neuron_info::{NeuronInfo, NeuronInfoLite},
    show_subnet::SubnetState,
    stake_info::{StakeAvailability, StakeInfo},
    subnet_info::{
        SubnetHyperparams, SubnetHyperparamsV2, SubnetHyperparamsV3, SubnetInfo, SubnetInfov2,
    },
};
use pallet_subtensor::staking::lock::LockState;
use sp_runtime::AccountId32;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{
    AlphaBalance, MechId, NetUid, ProxyFilterInfo, ProxyTypeInfo, TaoBalance,
};

// Here we declare the runtime API. It is implemented it the `impl` block in
// src/neuron_info.rs, src/subnet_info.rs, and src/delegate_info.rs
sp_api::decl_runtime_apis! {
    pub trait DelegateInfoRuntimeApi {
        fn get_delegates() -> Vec<DelegateInfo<AccountId32>>;
        fn get_delegate( delegate_account: AccountId32 ) -> Option<DelegateInfo<AccountId32>>;
        fn get_delegated( delegatee_account: AccountId32 ) -> Vec<(DelegateInfo<AccountId32>, (Compact<NetUid>, Compact<AlphaBalance>))>;
    }

    pub trait NeuronInfoRuntimeApi {
        fn get_neurons(netuid: NetUid) -> Vec<NeuronInfo<AccountId32>>;
        fn get_neuron(netuid: NetUid, uid: u16) -> Option<NeuronInfo<AccountId32>>;
        fn get_neurons_lite(netuid: NetUid) -> Vec<NeuronInfoLite<AccountId32>>;
        fn get_neuron_lite(netuid: NetUid, uid: u16) -> Option<NeuronInfoLite<AccountId32>>;
    }

    pub trait SubnetInfoRuntimeApi {
        fn get_subnet_info(netuid: NetUid) -> Option<SubnetInfo<AccountId32>>;
        fn get_subnets_info() -> Vec<Option<SubnetInfo<AccountId32>>>;
        fn get_subnet_info_v2(netuid: NetUid) -> Option<SubnetInfov2<AccountId32>>;
        fn get_subnets_info_v2() -> Vec<Option<SubnetInfov2<AccountId32>>>;
        #[deprecated(note = "Use `get_subnet_hyperparams_v3` instead.")]
        fn get_subnet_hyperparams(netuid: NetUid) -> Option<SubnetHyperparams>;
        #[deprecated(note = "Use `get_subnet_hyperparams_v3` instead.")]
        fn get_subnet_hyperparams_v2(netuid: NetUid) -> Option<SubnetHyperparamsV2>;
        #[api_version(2)]
        fn get_subnet_hyperparams_v3(netuid: NetUid) -> Option<SubnetHyperparamsV3>;
        fn get_all_dynamic_info() -> Vec<Option<DynamicInfo<AccountId32>>>;
        fn get_all_metagraphs() -> Vec<Option<Metagraph<AccountId32>>>;
        fn get_metagraph(netuid: NetUid) -> Option<Metagraph<AccountId32>>;
        fn get_all_mechagraphs() -> Vec<Option<Metagraph<AccountId32>>>;
        fn get_mechagraph(netuid: NetUid, mecid: MechId) -> Option<Metagraph<AccountId32>>;
        fn get_dynamic_info(netuid: NetUid) -> Option<DynamicInfo<AccountId32>>;
        fn get_subnet_state(netuid: NetUid) -> Option<SubnetState<AccountId32>>;
        fn get_selective_metagraph(netuid: NetUid, metagraph_indexes: Vec<u16>) -> Option<SelectiveMetagraph<AccountId32>>;
        fn get_coldkey_auto_stake_hotkey(coldkey: AccountId32, netuid: NetUid) -> Option<AccountId32>;
        fn get_selective_mechagraph(netuid: NetUid, subid: MechId, metagraph_indexes: Vec<u16>) -> Option<SelectiveMetagraph<AccountId32>>;
        fn get_subnet_to_prune() -> Option<NetUid>;
        fn get_subnet_account_id(netuid: NetUid) -> Option<AccountId32>;
        fn get_next_epoch_start_block(netuid: NetUid) -> Option<u64>;
        fn get_block_emission() -> TaoBalance;
    }

    pub trait StakeInfoRuntimeApi {
        fn get_stake_info_for_coldkey( coldkey_account: AccountId32 ) -> Vec<StakeInfo<AccountId32>>;
        fn get_stake_info_for_coldkeys( coldkey_accounts: Vec<AccountId32> ) -> Vec<(AccountId32, Vec<StakeInfo<AccountId32>>)>;
        fn get_stake_info_for_hotkey_coldkey_netuid( hotkey_account: AccountId32, coldkey_account: AccountId32, netuid: NetUid ) -> Option<StakeInfo<AccountId32>>;
        fn get_stake_availability_for_coldkeys( coldkey_accounts: Vec<AccountId32>, netuids: Option<Vec<NetUid>> ) -> BTreeMap<AccountId32, BTreeMap<NetUid, StakeAvailability>>;
        fn get_stake_fee( origin: Option<(AccountId32, NetUid)>, origin_coldkey_account: AccountId32, destination: Option<(AccountId32, NetUid)>, destination_coldkey_account: AccountId32, amount: u64 ) -> u64;
        fn get_coldkey_lock(coldkey: AccountId32, netuid: NetUid) -> Option<LockState>;
        fn get_hotkey_conviction(hotkey: AccountId32, netuid: NetUid) -> U64F64;
        fn get_most_convicted_hotkey_on_subnet(netuid: NetUid) -> Option<AccountId32>;
    }

    pub trait SubnetRegistrationRuntimeApi {
        fn get_network_registration_cost() -> TaoBalance;
    }

    pub trait ProxyFilterRuntimeApi {
        fn get_proxy_types() -> Vec<ProxyTypeInfo>;
        fn get_proxy_filters(proxy_types: Option<Vec<u8>>) -> Vec<ProxyFilterInfo>;
    }
}
