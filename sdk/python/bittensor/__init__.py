"""Bittensor SDK: a lean, unopinionated client for the Bittensor chain.

Quick start:

    import bittensor as bt

    sub = bt.Subtensor()                 # defaults to finney; connects lazily
    print(sub.balances.get("5F...coldkey"))

No ``close()`` needed — the connection is cleaned up automatically. The same
class is the async client when awaited:

    import asyncio
    import bittensor as bt

    async def main():
        async with bt.Subtensor() as client:      # or: client = await bt.Subtensor()
            bal = await client.balances.get("5F...coldkey")
            print(bal)

    asyncio.run(main())

Logging:

The SDK emits diagnostics through standard library loggers under the
``bittensor.*`` namespace (e.g. ``bittensor.transport`` for connection
lifecycle and RPC traffic). It never configures handlers, levels, or the
root logger, so it stays silent unless your application opts in:

    import logging

    logging.basicConfig(level=logging.INFO)                   # everything
    logging.getLogger("bittensor").setLevel(logging.DEBUG)    # just the SDK
"""

import importlib.metadata as _importlib_metadata
import logging as _logging

from . import evm, http_auth, intents, reads, timelock, wallets
from ._generated import calls, constants, storage
from ._generated import runtime_apis as runtime_api
from ._substrate import RpcSubstrate, Substrate
from ._subtensor import Subtensor, close_shared_clients, set_weights
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
from .ledger import LedgerError, LedgerSigner
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
    RpcConnectionError,
    RpcPolicyError,
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
from .vault import VaultError, VaultSigner
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

# Lowercase v10-style aliases: ``bt.subtensor()`` and ``bt.wallet()`` are the
# same classes as ``bt.Subtensor`` / ``bt.Wallet``. The ``wallet`` binding
# shadows the submodule of the same name in the package namespace on purpose;
# ``import bittensor.wallet`` still resolves to the module via ``sys.modules``.
subtensor = Subtensor
wallet = Wallet

__all__ = [
    "Subtensor",
    "subtensor",  # lowercase alias for Subtensor
    "set_weights",
    "close_shared_clients",
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
    "RpcConnectionError",
    "RpcPolicyError",
    "Wallet",
    "wallet",  # lowercase alias for Wallet
    "wallets",
    # EVM support: h160<->ss58 address math, EVM key storage, precompiles
    "evm",
    "timelock",
    "Timelocked",
    "TimelockError",
    "TimelockNotReady",
    # Hotkey-signed HTTP requests (btauth/1): the identity layer for
    # miner/validator traffic. Sign and verify only — bring your own HTTP.
    "http_auth",
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
    # Ledger hardware signing (Polkadot generic app, RFC-0078 clear-signing)
    "LedgerSigner",
    "LedgerError",
    # Polkadot Vault air-gapped signing (UOS QR round-trip)
    "VaultSigner",
    "VaultError",
    "CRYPTO_ED25519",
    "CRYPTO_SR25519",
    "DEFAULT_CRYPTO_TYPE",
    "parse_crypto_type",
    "format_crypto_type",
    *sorted(_INTENT_EXPORTS),
]

# The single source of truth for the version is pyproject.toml (which the
# release workflows stamp with the rc/dev suffix at build time); read it from
# the installed distribution so wheels report what they were published as.
try:
    __version__ = _importlib_metadata.version("bittensor")
except _importlib_metadata.PackageNotFoundError:  # uninstalled source tree
    __version__ = "0.0.0.dev0+unknown"

# Removed v10 API names raise with a pointer to the replacement instead of a
# bare AttributeError, so an un-migrated script fails with instructions
# rather than a mystery. The full mapping lives in the docs: /docs/migration.
_NO_NEURON_STACK = (
    "v11 has no miner/validator networking stack (axon/dendrite/synapse); "
    "publish your endpoint with the ServeAxon intent, keep your own HTTP "
    "layer, and authenticate requests with bittensor.http_auth"
)

_REMOVED_V10_HINTS = {
    "AsyncSubtensor": (
        "use `async with bittensor.Subtensor(network) as client:` "
        "or `await bittensor.Subtensor(network)`"
    ),
    "async_subtensor": "use `async with bittensor.Subtensor(network) as client:`",
    "get_async_subtensor": "use `await bittensor.Subtensor(network)`",
    "MockSubtensor": "removed — test against a local node instead",
    "axon": _NO_NEURON_STACK,
    "Axon": _NO_NEURON_STACK,
    "dendrite": _NO_NEURON_STACK,
    "Dendrite": _NO_NEURON_STACK,
    "Synapse": _NO_NEURON_STACK,
    "StreamingSynapse": _NO_NEURON_STACK,
    "SubnetsAPI": _NO_NEURON_STACK,
    "Tensor": _NO_NEURON_STACK,
    "logging": (
        "use the standard `logging` module; the SDK logs under the 'bittensor' logger namespace"
    ),
    "trace": "use `logging.getLogger('bittensor').setLevel(logging.DEBUG)`",
    "debug": "use `logging.getLogger('bittensor').setLevel(logging.DEBUG)`",
    # bt.config exists in v11 but is the CLI-config module, not the old
    # argparse Config class — only the class name can get a hint here.
    "Config": "removed — the SDK no longer parses CLI args; pass arguments directly",
    "Keypair": (
        "keypairs come from bittensor.Wallet; the low-level type is bittensor.sp_core.Keypair"
    ),
    "Keyfile": "use bittensor.keyfiles.Keyfile",
}


def __getattr__(name: str):
    hint = _REMOVED_V10_HINTS.get(name)
    if hint is not None:
        raise AttributeError(
            f"bittensor.{name} was removed in v11 — {hint}. "
            "Migration guide: https://www.bittensor.com/docs/migration"
        )
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
