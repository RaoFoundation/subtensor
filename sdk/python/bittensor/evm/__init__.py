"""Bittensor EVM support: the seam between h160 and ss58 worlds.

Everything the Subtensor EVM needs that plain Ethereum tooling can't know:

- address math (``h160_to_ss58`` mirror mapping, ``ss58_to_pubkey``),
- encrypted EVM key storage next to the wallet's hotkeys (keystore V3),
- network facts (chain IDs 964/945, RPC URLs) keyed by the usual network names,
- the precompile catalog (addresses + vendored ABIs) with call encode/decode,
- a minimal JSON-RPC client and transaction signer, and the 18-vs-9 decimal
  conversions (1 TAO = 1e18 wei on the EVM side, 1e9 rao natively).

Key storage and address math are dependency-free; signing and ABI encoding
need the ``eth-account`` package (``pip install 'bittensor[evm]'``).
"""

from .addresses import (
    h160_to_ss58,
    is_h160,
    normalize_h160,
    pubkey_to_ss58,
    ss58_to_h160_truncated,
    ss58_to_pubkey,
)
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
    decode_result,
    encode_call,
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
    "EvmKeyInfo",
    "EvmNetwork",
    "EvmRpc",
    "EvmRpcError",
    "EvmTxPreview",
    "Precompile",
    "association_proof",
    "balance_to_wei",
    "create_evm_key",
    "decode_result",
    "encode_call",
    "evm_network",
    "export_evm_key",
    "get_evm_key_info",
    "get_precompile",
    "h160_to_ss58",
    "import_evm_key",
    "is_h160",
    "list_evm_keys",
    "normalize_h160",
    "prepare_transaction",
    "pubkey_to_ss58",
    "send_transaction",
    "ss58_to_h160_truncated",
    "ss58_to_pubkey",
    "unlock_evm_key",
    "wei_to_balance",
]
