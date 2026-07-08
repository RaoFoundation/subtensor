"""Browser extension signing for the Bittensor SDK.

A local bridge page talks to wallet extensions (Polkadot.js first; the protocol
is extensible to Talisman, SubWallet, etc.). Python clients connect over
WebSocket and never see private keys.

With ``--signer extension``, the bridge starts automatically in the background
and you pick an account at the command line — no separate bridge step.
"""

from .bridge import DEFAULT_BRIDGE_HOST, DEFAULT_BRIDGE_PORT, BridgeServer, run_bridge
from .client import BridgeClient, BridgeError
from .session import ensure_bridge, start_bridge_daemon, stop_bridge_daemon
from .signer import (
    ExtensionAccount,
    ExtensionAccountSelection,
    ExtensionSigner,
    account_crypto_type,
    connect_extension_signer,
    open_extension_signer,
    select_extension_account,
)

__all__ = [
    "DEFAULT_BRIDGE_HOST",
    "DEFAULT_BRIDGE_PORT",
    "BridgeClient",
    "BridgeError",
    "BridgeServer",
    "ExtensionAccount",
    "ExtensionAccountSelection",
    "ExtensionSigner",
    "account_crypto_type",
    "connect_extension_signer",
    "ensure_bridge",
    "open_extension_signer",
    "run_bridge",
    "select_extension_account",
    "start_bridge_daemon",
    "stop_bridge_daemon",
]
