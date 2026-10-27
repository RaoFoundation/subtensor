"""Polkadot Vault (QR / air-gapped) signing for the Bittensor SDK.

With ``--signer vault``, btcli serves a local page that displays the
transaction as a proof-carrying UOS QR (the animated QR embeds the RFC-0078
metadata proof, so the phone needs no metadata updates); scan it with the
Polkadot Vault app, approve, and hold the phone's signature QR up to the
webcam. Keys never leave the phone. Vault needs the Bittensor network added
once via a chain-specs QR (hosted in the docs guide,
``settings.VAULT_GUIDE_URL``).
"""

from .scan import scan_vault_address
from .server import VaultPageError, VaultSessionServer
from .signer import VaultError, VaultSigner

__all__ = [
    "VaultError",
    "VaultPageError",
    "VaultSessionServer",
    "VaultSigner",
    "scan_vault_address",
]
