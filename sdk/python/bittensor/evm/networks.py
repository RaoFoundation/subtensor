"""EVM connectivity facts per network, keyed by the same names ``--network`` uses.

Chain IDs are on-chain state (``EVMChainId`` pallet) but fixed in practice:
964 on mainnet (UTF-8 for the TAO symbol) and 945 on testnet (UTF-8 for the
alpha symbol). A fresh localnet boots with the generic substrate chain ID 42
from its chainspec genesis; ``btcli evm setup-localnet`` replaces it (945 by
default) via the ``AdminUtils.sudo_set_evm_chain_id`` sudo extrinsic.
"""

from __future__ import annotations

import os
from dataclasses import dataclass


@dataclass(frozen=True)
class EvmNetwork:
    name: str
    chain_id: "int | None"  # None: localnet — genesis 42 until sudo_set_evm_chain_id
    rpc_url: str  # HTTP JSON-RPC (eth_*) endpoint
    currency_symbol: str = "TAO"
    # 1 TAO is 1e18 in EVM transaction values (Ethereum's 18 decimals) but
    # 1e9 rao natively; every EVM<->native amount conversion crosses this.
    evm_decimals: int = 18


EVM_NETWORKS: dict[str, EvmNetwork] = {
    "finney": EvmNetwork(
        name="finney",
        chain_id=964,
        rpc_url="https://lite.chain.opentensor.ai",
    ),
    "test": EvmNetwork(
        name="test",
        chain_id=945,
        rpc_url="https://test.chain.opentensor.ai",
    ),
    "local": EvmNetwork(
        name="local",
        chain_id=None,
        rpc_url=os.getenv("BT_EVM_ENDPOINT") or "http://127.0.0.1:9944",
    ),
}

# Substrate ws endpoints (bittensor.settings.NETWORKS values) mapped back to
# their EVM networks, so a raw `-n wss://...` still resolves when it is one
# of the known presets.
_WS_TO_LABEL = {
    "wss://entrypoint-finney.opentensor.ai:443": "finney",
    "wss://lite.chain.opentensor.ai:443": "finney",
    "wss://test.finney.opentensor.ai:443": "test",
    "ws://127.0.0.1:9944": "local",
}


def evm_network(network: str) -> EvmNetwork:
    """Resolve a network label, ws:// endpoint, or http(s):// EVM RPC URL.

    An explicit http(s) URL is taken as a direct EVM RPC endpoint with an
    unknown chain ID; known names/endpoints resolve to their preset.
    """
    if network.startswith("http://") or network.startswith("https://"):
        return EvmNetwork(name=network, chain_id=None, rpc_url=network)
    label = _WS_TO_LABEL.get(network, network)
    if label.startswith("ws://") or label.startswith("wss://"):
        # An unknown substrate endpoint: subtensor nodes serve eth JSON-RPC
        # over HTTP on the same host/port the ws endpoint uses.
        http = label.replace("wss://", "https://", 1).replace("ws://", "http://", 1)
        return EvmNetwork(name=network, chain_id=None, rpc_url=http)
    try:
        return EVM_NETWORKS[label]
    except KeyError:
        raise ValueError(
            f"Unknown network {network!r} for EVM access. Known: {sorted(EVM_NETWORKS)}, "
            "or pass an http(s):// EVM RPC URL."
        ) from None
