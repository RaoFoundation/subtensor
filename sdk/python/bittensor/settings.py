"""Network presets and chain constants.

This is the single place that knows about networks and chain-wide magic numbers.
Everything else takes an explicit endpoint or a network name resolved here.
"""

import os
from typing import Optional

SS58_FORMAT = 42

TAO_SYMBOL = "\u03c4"  # τ
ALPHA_SYMBOL = "\u03b1"  # α

RAO_PER_TAO = 10**9

# Max value of a u16 weight, used for on-chain weight normalization.
U16_MAX = 65535

BLOCKTIME = 12.0

DEFAULT_ERA_PERIOD = 128

# MEV-shielded extrinsics must use a short-lived era: the inner extrinsic has to
# stay valid only until the block author reveals and executes it. The chain's
# CheckMortality extension rejects shielded submissions with an era period
# above 8 blocks (MAX_SHIELD_ERA_PERIOD in the runtime), so this must not
# exceed 8.
MEV_SHIELD_ERA_PERIOD = 8

GLOBAL_MAX_SUBNET_COUNT = 4096

NETWORKS = {
    "finney": "wss://entrypoint-finney.opentensor.ai:443",
    "test": "wss://test.finney.opentensor.ai:443",
    "archive": "wss://archive.chain.opentensor.ai:443",
    "devnet": "wss://dev.chain.opentensor.ai:443",
    "local": os.getenv("BT_CHAIN_ENDPOINT") or "ws://127.0.0.1:9944",
}

DEFAULT_NETWORK = "finney"

# Default fallback pools, keyed by network label. When the primary endpoint is
# unreachable (or exhausts its reconnect retries mid-session) the connection
# rotates through these transparently. Networks without public alternatives
# (test, local) have no fallbacks.
FALLBACK_ENDPOINTS = {
    "finney": [
        "wss://lite.chain.opentensor.ai:443",
        "wss://lite.sub.latent.to:443",
    ],
    "archive": [
        "wss://archive.sub.latent.to:443",
    ],
}

# Default archive pools, keyed by network label. Used when a read hits state
# the primary (lite) node has already pruned: the read is retried against an
# archive node instead of failing with "state discarded".
ARCHIVE_ENDPOINTS = {
    "finney": [
        "wss://archive.chain.opentensor.ai:443",
        "wss://archive.sub.latent.to:443",
    ],
    "archive": [
        "wss://archive.sub.latent.to:443",
    ],
}


def default_fallback_endpoints(network: str) -> list[str]:
    """Default fallback endpoints for a network label (or a known endpoint URL)."""
    label = _REVERSE_NETWORKS.get(network, network)
    primary = NETWORKS.get(label)
    return [url for url in FALLBACK_ENDPOINTS.get(label, []) if url != primary]


def default_archive_endpoints(network: str) -> list[str]:
    """Default archive endpoints for a network label (or a known endpoint URL)."""
    label = _REVERSE_NETWORKS.get(network, network)
    return list(ARCHIVE_ENDPOINTS.get(label, []))


def resolve_endpoint(network: str) -> tuple[str, str]:
    """Resolve a network name or raw ws(s):// URL into (network_label, endpoint_url).

    A bare ``ws://`` or ``wss://`` string is treated as a direct endpoint.
    """
    if network.startswith("ws://") or network.startswith("wss://"):
        label = _REVERSE_NETWORKS.get(network, network)
        return label, network
    try:
        return network, NETWORKS[network]
    except KeyError:
        raise ValueError(
            f"Unknown network {network!r}. Known networks: {sorted(NETWORKS)}, "
            "or pass a ws:// / wss:// endpoint directly."
        ) from None


_REVERSE_NETWORKS = {url: name for name, url in NETWORKS.items()}

# The published SDK/CLI documentation site (docs pages referenced from CLI
# output link here).
DOCS_URL = "https://www.bittensor.com/docs"


def error_docs_url(code_value: str) -> str:
    """Docs explainer page for a semantic error code (e.g. insufficient_balance)."""
    return f"{DOCS_URL}/errors/{code_value.replace('_', '-')}"


def chain_error_docs_url(name: str) -> str:
    """Docs explainer page for an exact chain error name (e.g. SlippageTooHigh).

    The docs keep the on-chain CamelCase name in the URL, so no mangling."""
    return f"{DOCS_URL}/errors/chain/{name}"


# Public block-explorer pages for an extrinsic, keyed by network label. ``{id}``
# is the on-chain extrinsic identifier "block_number-index" (index zero-padded
# to 4 digits), the format both taostats and taomarketcap use. taomarketcap is
# preferred because taostats renders 404s for recent extrinsics. Networks that
# no public explorer indexes (test, local) get no link.
EXPLORER_EXTRINSIC_URLS = {
    "finney": "https://taomarketcap.com/extrinsics/{id}",
    "archive": "https://taomarketcap.com/extrinsics/{id}",
}


def explorer_extrinsic_url(network: str, extrinsic_id: str) -> Optional[str]:
    """Explorer page for an extrinsic, or None when the network is not indexed.

    ``network`` may be a network label or an endpoint URL (mapped back to its
    label when it is a known preset).
    """
    label = _REVERSE_NETWORKS.get(network, network)
    template = EXPLORER_EXTRINSIC_URLS.get(label)
    return template.format(id=extrinsic_id) if template else None


# Public explorer pages for accounts, keyed by network label then key kind.
# Hotkeys land on the validator page (stake, APY, take); coldkeys on the
# account page (balance, transfers). Same indexing caveat as extrinsics:
# test/local get no link.
EXPLORER_ACCOUNT_URLS = {
    "finney": {
        "hotkey": "https://taomarketcap.com/validators/{ss58}",
        "coldkey": "https://taomarketcap.com/coldkey/{ss58}",
    },
}
EXPLORER_ACCOUNT_URLS["archive"] = EXPLORER_ACCOUNT_URLS["finney"]


def explorer_account_url(network: str, kind: str, ss58: str) -> Optional[str]:
    """Explorer page for an account, or None when the network is not indexed.

    ``kind`` is ``"hotkey"`` (validator page) or ``"coldkey"`` (account page).
    """
    label = _REVERSE_NETWORKS.get(network, network)
    template = EXPLORER_ACCOUNT_URLS.get(label, {}).get(kind)
    return template.format(ss58=ss58) if template else None


# Public explorer pages for a block, keyed by network label. Same indexing
# caveat as extrinsics: test/local get no link.
EXPLORER_BLOCK_URLS = {
    "finney": "https://www.tao.app/block/{number}",
}
EXPLORER_BLOCK_URLS["archive"] = EXPLORER_BLOCK_URLS["finney"]


def explorer_block_url(network: str, number: int) -> Optional[str]:
    """Explorer page for a block, or None when the network is not indexed."""
    label = _REVERSE_NETWORKS.get(network, network)
    template = EXPLORER_BLOCK_URLS.get(label)
    return template.format(number=number) if template else None


# Public explorer pages for a subnet, keyed by network label. Same indexing
# caveat as extrinsics: test/local get no link.
EXPLORER_SUBNET_URLS = {
    "finney": "https://taomarketcap.com/subnets/{netuid}",
}
EXPLORER_SUBNET_URLS["archive"] = EXPLORER_SUBNET_URLS["finney"]


def explorer_subnet_url(network: str, netuid: int) -> Optional[str]:
    """Explorer page for a subnet, or None when the network is not indexed."""
    label = _REVERSE_NETWORKS.get(network, network)
    template = EXPLORER_SUBNET_URLS.get(label)
    return template.format(netuid=netuid) if template else None
