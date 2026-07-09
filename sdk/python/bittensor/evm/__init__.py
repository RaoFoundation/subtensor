"""Bittensor EVM support: the seam between h160 and ss58 worlds.

Everything the Subtensor EVM needs that plain Ethereum tooling can't know:

- address math (``h160_to_ss58`` mirror mapping, ``ss58_to_pubkey``),
- encrypted EVM key storage next to the wallet's hotkeys (keystore V3),
- network facts (chain IDs 964/945, RPC URLs) keyed by the usual network names,
- the precompile catalog (addresses + vendored ABIs) with call encode/decode,
- a minimal JSON-RPC client and transaction signer, and the 18-vs-9 decimal
  conversions (1 TAO = 1e18 wei on the EVM side, 1e9 rao natively).

Signing and ABI encoding use the ``eth-account`` package, which is a core
dependency of the SDK.
"""

from .addresses import (
    h160_to_ss58,
    is_h160,
    normalize_h160,
    pubkey_to_ss58,
    ss58_to_h160_truncated,
    ss58_to_pubkey,
)
from .contracts import ContractArtifact, encode_deploy, load_artifact
from .keys import (
    ETH_DERIVATION_PATH,
    EvmKeyInfo,
    create_evm_key,
    export_evm_key,
    get_evm_key_info,
    import_evm_key,
    list_evm_keys,
    unlock_evm_key,
)
from .networks import EVM_NETWORKS, EvmNetwork, evm_network
from .precompiles import (
    PRECOMPILES,
    STANDARD_PRECOMPILES,
    Precompile,
    caller_role,
    decode_result,
    describe_arguments,
    encode_call,
    function_deprecation,
    get_precompile,
)
from .rpc import WEI_PER_TAO, EvmRpc, EvmRpcError, balance_to_wei, wei_to_balance
from .transactions import (
    EvmTxPreview,
    association_proof,
    prepare_transaction,
    send_transaction,
)

__all__ = [
    "ETH_DERIVATION_PATH",
    "EVM_NETWORKS",
    "PRECOMPILES",
    "STANDARD_PRECOMPILES",
    "WEI_PER_TAO",
    "ContractArtifact",
    "EvmKeyInfo",
    "EvmNetwork",
    "EvmRpc",
    "EvmRpcError",
    "EvmTxPreview",
    "Precompile",
    "association_proof",
    "balance_to_wei",
    "caller_role",
    "create_evm_key",
    "decode_result",
    "describe_arguments",
    "encode_call",
    "encode_deploy",
    "evm_network",
    "export_evm_key",
    "function_deprecation",
    "get_evm_key_info",
    "get_precompile",
    "h160_to_ss58",
    "import_evm_key",
    "is_h160",
    "list_evm_keys",
    "load_artifact",
    "normalize_h160",
    "prepare_transaction",
    "pubkey_to_ss58",
    "send_transaction",
    "ss58_to_h160_truncated",
    "ss58_to_pubkey",
    "unlock_evm_key",
    "wei_to_balance",
]
