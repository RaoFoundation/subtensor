"""Import an address from a Polkadot Vault phone by scanning its QR.

Vault's key-details screen shows the account as a ``substrate:<ss58>:<genesis
hash>`` QR (the polkadot-js address-QR convention); some wallets show a bare
ss58 instead. This scans either through the same local page the vault signer
uses — webcam only, nothing is sent to the phone.
"""

from __future__ import annotations

from typing import Callable, Optional

from .server import VaultSessionServer

_SCAN_PROMPT = (
    "Open the key in Polkadot Vault (tap it in the key list) — the screen "
    "shows its address QR. Hold the phone up to this camera."
)


async def scan_vault_address(
    *,
    browser: Optional[str] = None,
    open_browser: bool = True,
    on_status: Optional[Callable[[str], None]] = None,
) -> tuple[str, Optional[str]]:
    """Scan one address QR and return ``(ss58, genesis_hash or None)``.

    The genesis hash is present when the phone showed a ``substrate:`` QR;
    callers can compare it against the expected chain and warn on mismatch.
    """
    server = VaultSessionServer()
    await server.start(open_browser=open_browser, browser=browser)
    if on_status is not None:
        on_status(
            f"scan page open at {server.http_url} — show your Vault key's address QR to the webcam"
        )
    try:
        text = await server.request_scan(prompt=_SCAN_PROMPT)
    finally:
        await server.stop()
    if text.startswith("substrate:"):
        parts = text.split(":")
        address = parts[1]
        genesis = parts[2] if len(parts) > 2 and parts[2] else None
        if genesis is not None and not genesis.startswith("0x"):
            genesis = "0x" + genesis
        return address, genesis
    return text, None
