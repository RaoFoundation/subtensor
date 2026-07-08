"""Bittensor SDK: a lean, unopinionated client for the Bittensor chain.

Quick start:

    import asyncio
    import bittensor as sub

    async def main():
        async with sub.Client("finney") as client:
            bal = await client.balances.get("5F...coldkey")
            print(bal)

    asyncio.run(main())

Synchronous:

    client = sub.SyncClient("finney")
    print(client.balances.get("5F...coldkey"))
    client.close()

Logging:

The SDK emits diagnostics through standard library loggers under the
``bittensor.*`` namespace (e.g. ``bittensor.transport`` for connection
lifecycle and RPC traffic). It never configures handlers, levels, or the
root logger, so it stays silent unless your application opts in:

    import logging

    logging.basicConfig(level=logging.INFO)                   # everything
    logging.getLogger("bittensor").setLevel(logging.DEBUG)    # just the SDK
"""

import logging as _logging

from . import evm, intents, reads, timelock, wallets
from ._generated import calls, constants, storage
from ._generated import runtime_apis as runtime_api
from ._substrate import RpcSubstrate, Substrate
from ._transport.contract import SigningContext, UnsignedExtrinsic
from .balance import Balance, UnitMismatchError, alpha, rao, tao
from .client import BlockHeader, BlockInfo, Client, EpochEvent
from .extension import (
    BridgeClient,
    BridgeError,
    BridgeServer,
    ExtensionAccount,
    ExtensionSigner,
    connect_extension_signer,
    ensure_bridge,
    run_bridge,
    stop_bridge_daemon,
)
from .intents import REGISTRY as _INTENT_REGISTRY
from .intents import Intent, Plan, Policy
from .metagraph import Metagraph, MetagraphNeuron, NeuronCommitment
from .multisig import Multisig
from .reads import (
    Commitment,
    DelegatedStake,
    DelegateInfo,
    Neuron,
    StakePosition,
    StakeValuation,
    SubnetInfo,
    SwapQuote,
)
from .result import (
    BittensorError,
    ChainError,
    ConnectionNotReady,
    ErrorCode,
    ExtrinsicResult,
    PolicyError,
)
from .signing import (
    KeyedWallet,
    Signer,
    WalletLike,
    WalletSigner,
    public_view,
    resolve_signer,
)
from .snapshot import Snapshot
from .sync import SyncClient
from .timelock import Timelocked, TimelockError, TimelockNotReady
from .wallet import Wallet
from .wallets import (
    CRYPTO_ED25519,
    CRYPTO_SR25519,
    DEFAULT_CRYPTO_TYPE,
    format_crypto_type,
    parse_crypto_type,
)

# Library convention (logging HOWTO): attach a NullHandler to the package
# root logger so the SDK is silent by default (no lastResort stderr fallback)
# while leaving the consumer's logging configuration untouched.
_logging.getLogger(__name__).addHandler(_logging.NullHandler())

# Re-export every registered intent class at the top level, derived from the
# registry so this can never drift from the actual set of intents (the codegen
# coverage gate is the source of truth for which intents exist).
_INTENT_EXPORTS = {cls.__name__: cls for cls in _INTENT_REGISTRY.values()}
globals().update(_INTENT_EXPORTS)

__all__ = [
    "Client",
    "SyncClient",
    # The chain-access contract and its production (websocket RPC) backend.
    # Client(substrate=...) accepts any Substrate implementation.
    "Substrate",
    "RpcSubstrate",
    "Snapshot",
    "BlockHeader",
    "BlockInfo",
    "EpochEvent",
    "Neuron",
    "Metagraph",
    "MetagraphNeuron",
    "NeuronCommitment",
    "SubnetInfo",
    "Balance",
    "UnitMismatchError",
    "tao",
    "alpha",
    "rao",
    "StakePosition",
    "StakeValuation",
    "DelegateInfo",
    "DelegatedStake",
    "Commitment",
    "SwapQuote",
    # The read registry (client.read / client.reads is the dispatch surface)
    "reads",
    "ExtrinsicResult",
    "BittensorError",
    "ChainError",
    "ConnectionNotReady",
    "ErrorCode",
    "PolicyError",
    "Wallet",
    "wallets",
    # EVM support: h160<->ss58 address math, EVM key storage, precompiles
    "evm",
    "timelock",
    "Timelocked",
    "TimelockError",
    "TimelockNotReady",
    # Generated chain vocabulary (descriptors for query/runtime/constant, and
    # raw call builders for the submit_call escape hatch)
    "storage",
    "runtime_api",
    "constants",
    "calls",
    # Intent layer
    "intents",
    "Intent",
    "Plan",
    "Policy",
    "Multisig",
    "Signer",
    "KeyedWallet",
    "WalletLike",
    "WalletSigner",
    "public_view",
    "resolve_signer",
    # Out-of-process signing (QR / air-gapped / hardware): prepare_call ->
    # UnsignedExtrinsic, external signature -> submit_signature. SigningContext
    # feeds a signer's optional metadata_digest hook (CheckMetadataHash).
    "UnsignedExtrinsic",
    "SigningContext",
    "BridgeClient",
    "BridgeError",
    "BridgeServer",
    "ExtensionAccount",
    "ExtensionSigner",
    "connect_extension_signer",
    "ensure_bridge",
    "run_bridge",
    "stop_bridge_daemon",
    "CRYPTO_ED25519",
    "CRYPTO_SR25519",
    "DEFAULT_CRYPTO_TYPE",
    "parse_crypto_type",
    "format_crypto_type",
    *sorted(_INTENT_EXPORTS),
]

__version__ = "11.0.0.dev0"
