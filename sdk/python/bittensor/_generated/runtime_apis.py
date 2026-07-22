"""Generated from runtime metadata by codegen. DO NOT EDIT BY HAND.

Regenerate with: python -m codegen <ws-endpoint>
Spec version: 438

Runtime API method descriptors: unpack into substrate.runtime_call.
"""
from typing import NamedTuple


class Method(NamedTuple):
    """A (container, name) pair; unpack into query/constant calls."""

    container: str
    name: str


class AccountNonceApi:
    account_nonce = Method('AccountNonceApi', 'account_nonce')

class AuraApi:
    slot_duration = Method('AuraApi', 'slot_duration')
    authorities = Method('AuraApi', 'authorities')

class BabeApi:
    configuration = Method('BabeApi', 'configuration')
    current_epoch_start = Method('BabeApi', 'current_epoch_start')
    current_epoch = Method('BabeApi', 'current_epoch')
    next_epoch = Method('BabeApi', 'next_epoch')
    generate_key_ownership_proof = Method('BabeApi', 'generate_key_ownership_proof')
    submit_report_equivocation_unsigned_extrinsic = Method('BabeApi', 'submit_report_equivocation_unsigned_extrinsic')

class BlockBuilder:
    apply_extrinsic = Method('BlockBuilder', 'apply_extrinsic')
    finalize_block = Method('BlockBuilder', 'finalize_block')
    inherent_extrinsics = Method('BlockBuilder', 'inherent_extrinsics')
    check_inherents = Method('BlockBuilder', 'check_inherents')

class ContractsApi:
    call = Method('ContractsApi', 'call')
    instantiate = Method('ContractsApi', 'instantiate')
    upload_code = Method('ContractsApi', 'upload_code')
    get_storage = Method('ContractsApi', 'get_storage')

class ConvertTransactionRuntimeApi:
    convert_transaction = Method('ConvertTransactionRuntimeApi', 'convert_transaction')

class Core:
    version = Method('Core', 'version')
    execute_block = Method('Core', 'execute_block')
    initialize_block = Method('Core', 'initialize_block')

class DelegateInfoRuntimeApi:
    get_delegates = Method('DelegateInfoRuntimeApi', 'get_delegates')
    get_delegate = Method('DelegateInfoRuntimeApi', 'get_delegate')
    get_delegated = Method('DelegateInfoRuntimeApi', 'get_delegated')

class EthereumRuntimeRPCApi:
    chain_id = Method('EthereumRuntimeRPCApi', 'chain_id')
    account_basic = Method('EthereumRuntimeRPCApi', 'account_basic')
    gas_price = Method('EthereumRuntimeRPCApi', 'gas_price')
    account_code_at = Method('EthereumRuntimeRPCApi', 'account_code_at')
    author = Method('EthereumRuntimeRPCApi', 'author')
    storage_at = Method('EthereumRuntimeRPCApi', 'storage_at')
    call = Method('EthereumRuntimeRPCApi', 'call')
    create = Method('EthereumRuntimeRPCApi', 'create')
    current_block = Method('EthereumRuntimeRPCApi', 'current_block')
    current_receipts = Method('EthereumRuntimeRPCApi', 'current_receipts')
    current_transaction_statuses = Method('EthereumRuntimeRPCApi', 'current_transaction_statuses')
    current_all = Method('EthereumRuntimeRPCApi', 'current_all')
    extrinsic_filter = Method('EthereumRuntimeRPCApi', 'extrinsic_filter')
    elasticity = Method('EthereumRuntimeRPCApi', 'elasticity')
    gas_limit_multiplier_support = Method('EthereumRuntimeRPCApi', 'gas_limit_multiplier_support')
    pending_block = Method('EthereumRuntimeRPCApi', 'pending_block')
    initialize_pending_block = Method('EthereumRuntimeRPCApi', 'initialize_pending_block')

class GenesisBuilder:
    build_state = Method('GenesisBuilder', 'build_state')
    get_preset = Method('GenesisBuilder', 'get_preset')
    preset_names = Method('GenesisBuilder', 'preset_names')

class GrandpaApi:
    grandpa_authorities = Method('GrandpaApi', 'grandpa_authorities')
    submit_report_equivocation_unsigned_extrinsic = Method('GrandpaApi', 'submit_report_equivocation_unsigned_extrinsic')
    generate_key_ownership_proof = Method('GrandpaApi', 'generate_key_ownership_proof')
    current_set_id = Method('GrandpaApi', 'current_set_id')

class Metadata:
    metadata = Method('Metadata', 'metadata')
    metadata_at_version = Method('Metadata', 'metadata_at_version')
    metadata_versions = Method('Metadata', 'metadata_versions')

class NeuronInfoRuntimeApi:
    get_neurons = Method('NeuronInfoRuntimeApi', 'get_neurons')
    get_neuron = Method('NeuronInfoRuntimeApi', 'get_neuron')
    get_neurons_lite = Method('NeuronInfoRuntimeApi', 'get_neurons_lite')
    get_neuron_lite = Method('NeuronInfoRuntimeApi', 'get_neuron_lite')

class OffchainWorkerApi:
    offchain_worker = Method('OffchainWorkerApi', 'offchain_worker')

class ProxyFilterRuntimeApi:
    get_proxy_types = Method('ProxyFilterRuntimeApi', 'get_proxy_types')
    get_proxy_filters = Method('ProxyFilterRuntimeApi', 'get_proxy_filters')

class SessionKeys:
    generate_session_keys = Method('SessionKeys', 'generate_session_keys')
    decode_session_keys = Method('SessionKeys', 'decode_session_keys')

class ShieldApi:
    try_decode_shielded_tx = Method('ShieldApi', 'try_decode_shielded_tx')
    is_shielded_using_current_key = Method('ShieldApi', 'is_shielded_using_current_key')
    try_unshield_tx = Method('ShieldApi', 'try_unshield_tx')

class StakeInfoRuntimeApi:
    get_stake_info_for_coldkey = Method('StakeInfoRuntimeApi', 'get_stake_info_for_coldkey')
    get_stake_info_for_coldkeys = Method('StakeInfoRuntimeApi', 'get_stake_info_for_coldkeys')
    get_stake_info_for_hotkey_coldkey_netuid = Method('StakeInfoRuntimeApi', 'get_stake_info_for_hotkey_coldkey_netuid')
    get_stake_availability_for_coldkeys = Method('StakeInfoRuntimeApi', 'get_stake_availability_for_coldkeys')
    get_stake_fee = Method('StakeInfoRuntimeApi', 'get_stake_fee')
    get_coldkey_lock = Method('StakeInfoRuntimeApi', 'get_coldkey_lock')
    get_hotkey_conviction = Method('StakeInfoRuntimeApi', 'get_hotkey_conviction')
    get_most_convicted_hotkey_on_subnet = Method('StakeInfoRuntimeApi', 'get_most_convicted_hotkey_on_subnet')

class SubnetInfoRuntimeApi:
    get_subnet_info = Method('SubnetInfoRuntimeApi', 'get_subnet_info')
    get_subnets_info = Method('SubnetInfoRuntimeApi', 'get_subnets_info')
    get_subnet_info_v2 = Method('SubnetInfoRuntimeApi', 'get_subnet_info_v2')
    get_subnets_info_v2 = Method('SubnetInfoRuntimeApi', 'get_subnets_info_v2')
    get_subnet_hyperparams = Method('SubnetInfoRuntimeApi', 'get_subnet_hyperparams')
    get_subnet_hyperparams_v2 = Method('SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v2')
    get_all_dynamic_info = Method('SubnetInfoRuntimeApi', 'get_all_dynamic_info')
    get_all_metagraphs = Method('SubnetInfoRuntimeApi', 'get_all_metagraphs')
    get_metagraph = Method('SubnetInfoRuntimeApi', 'get_metagraph')
    get_all_mechagraphs = Method('SubnetInfoRuntimeApi', 'get_all_mechagraphs')
    get_mechagraph = Method('SubnetInfoRuntimeApi', 'get_mechagraph')
    get_dynamic_info = Method('SubnetInfoRuntimeApi', 'get_dynamic_info')
    get_subnet_state = Method('SubnetInfoRuntimeApi', 'get_subnet_state')
    get_selective_metagraph = Method('SubnetInfoRuntimeApi', 'get_selective_metagraph')
    get_coldkey_auto_stake_hotkey = Method('SubnetInfoRuntimeApi', 'get_coldkey_auto_stake_hotkey')
    get_selective_mechagraph = Method('SubnetInfoRuntimeApi', 'get_selective_mechagraph')
    get_subnet_to_prune = Method('SubnetInfoRuntimeApi', 'get_subnet_to_prune')
    get_subnet_account_id = Method('SubnetInfoRuntimeApi', 'get_subnet_account_id')
    get_next_epoch_start_block = Method('SubnetInfoRuntimeApi', 'get_next_epoch_start_block')
    get_block_emission = Method('SubnetInfoRuntimeApi', 'get_block_emission')
    get_subnet_hyperparams_v3 = Method('SubnetInfoRuntimeApi', 'get_subnet_hyperparams_v3')

class SubnetRegistrationRuntimeApi:
    get_network_registration_cost = Method('SubnetRegistrationRuntimeApi', 'get_network_registration_cost')

class SwapRuntimeApi:
    current_alpha_price = Method('SwapRuntimeApi', 'current_alpha_price')
    current_alpha_price_all = Method('SwapRuntimeApi', 'current_alpha_price_all')
    sim_swap_tao_for_alpha = Method('SwapRuntimeApi', 'sim_swap_tao_for_alpha')
    sim_swap_alpha_for_tao = Method('SwapRuntimeApi', 'sim_swap_alpha_for_tao')

class TaggedTransactionQueue:
    validate_transaction = Method('TaggedTransactionQueue', 'validate_transaction')

class TransactionPaymentApi:
    query_info = Method('TransactionPaymentApi', 'query_info')
    query_fee_details = Method('TransactionPaymentApi', 'query_fee_details')
    query_weight_to_fee = Method('TransactionPaymentApi', 'query_weight_to_fee')
    query_length_to_fee = Method('TransactionPaymentApi', 'query_length_to_fee')

class TransactionPaymentCallApi:
    query_call_info = Method('TransactionPaymentCallApi', 'query_call_info')
    query_call_fee_details = Method('TransactionPaymentCallApi', 'query_call_fee_details')
    query_weight_to_fee = Method('TransactionPaymentCallApi', 'query_weight_to_fee')
    query_length_to_fee = Method('TransactionPaymentCallApi', 'query_length_to_fee')

