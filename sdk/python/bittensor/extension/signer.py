"""Extension-backed signers implementing the SDK :class:`~bittensor.signing.Signer` protocol."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Optional

from ..sp_core import CRYPTO_ED25519, CRYPTO_SR25519
from ..wallets import CRYPTO_TYPE_NAMES
from .client import BridgeClient, BridgeError, ExtensionAccount
from .picker import pick_extension_account

_ACCOUNT_TYPE_TO_CRYPTO = {
    "ed25519": CRYPTO_ED25519,
    "sr25519": CRYPTO_SR25519,
}

_UNSUPPORTED_EXTENSION_ACCOUNT_TYPES = frozenset({"ecdsa", "ethereum"})


def account_crypto_type(account_type: str) -> int:
    normalized = account_type.strip().lower()
    if normalized in _UNSUPPORTED_EXTENSION_ACCOUNT_TYPES:
        raise ValueError(
            f"extension account type {account_type!r} is not supported for Substrate "
            "signing; use an sr25519 or ed25519 account in your wallet extension"
        )
    if normalized not in _ACCOUNT_TYPE_TO_CRYPTO:
        supported = ", ".join(sorted(CRYPTO_TYPE_NAMES.values()))
        raise ValueError(
            f"unsupported extension account type {account_type!r}; expected {supported}"
        )
    return _ACCOUNT_TYPE_TO_CRYPTO[normalized]


@dataclass(frozen=True)
class ExtensionAccountSelection:
    account: ExtensionAccount
    public_key: bytes
    crypto_type: int
    ss58_format: int


class ExtensionSigner:
    """Sign extrinsics via a browser extension bridge.

    Private keys never leave the extension. ``sign_extrinsic_payload`` is invoked
    by the transport when building a signed extrinsic; ``sign`` covers raw-byte
    flows such as ``btcli wallet sign``.
    """

    uses_extension_signing = True

    def __init__(
        self,
        account: ExtensionAccount,
        bridge: BridgeClient,
        *,
        public_key: bytes,
        crypto_type: int,
        ss58_format: int = 42,
    ):
        self._account = account
        self._bridge = bridge
        self._public_key = public_key
        self._crypto_type = crypto_type
        self._ss58_format = ss58_format

    @property
    def ss58_address(self) -> str:
        return self._account.address

    @property
    def public_key(self) -> bytes:
        return self._public_key

    @property
    def crypto_type(self) -> int:
        return self._crypto_type

    @property
    def ss58_format(self) -> int:
        return self._ss58_format

    @property
    def extension_source(self) -> str:
        return self._account.source

    @property
    def account_name(self) -> str:
        return self._account.name

    async def close(self) -> None:
        await self._bridge.close()

    async def report_transaction_result(self, success: bool) -> None:
        """Update the bridge page after the chain submission completes."""
        await self._bridge.report_transaction_result(success)

    async def sign_extrinsic_payload(self, payload: dict[str, Any]) -> dict[str, Any]:
        body = dict(payload)
        body["address"] = self.ss58_address
        try:
            return await self._bridge.sign_extrinsic_payload(body)
        except BridgeError as error:
            message = str(error).lower()
            if "cancel" in message or "reject" in message or "denied" in message:
                raise BridgeError("extension signing cancelled") from error
            raise

    async def sign(self, payload: bytes) -> bytes:
        result = await self._bridge.sign_bytes(
            self.ss58_address,
            "0x" + payload.hex(),
        )
        signature = result.get("signature")
        if not isinstance(signature, str):
            raise BridgeError("extension did not return a signature")
        return bytes.fromhex(signature.removeprefix("0x"))

    def __repr__(self) -> str:
        return (
            f"ExtensionSigner({self._account.name!r}, "
            f"source={self._account.source!r}, address={self.ss58_address})"
        )


async def select_extension_account(
    bridge_url: str,
    *,
    address: Optional[str] = None,
    source: Optional[str] = None,
    name: Optional[str] = None,
    interactive: bool = True,
    default_address: Optional[str] = None,
    on_picked: Optional[Callable[[ExtensionAccount], None]] = None,
) -> ExtensionAccountSelection:
    """List extension accounts, pick one, and return public key material."""
    from ..sp_core import Keypair

    async with BridgeClient(bridge_url) as client:
        accounts = await client.list_accounts()
    if not accounts:
        raise BridgeError(
            "no extension accounts available; authorize your browser extension and try again"
        )

    selected = pick_extension_account(
        accounts,
        address=address,
        source=source,
        name=name,
        interactive=interactive,
        default_address=default_address,
        on_picked=on_picked,
    )
    crypto_type = account_crypto_type(selected.type)
    public = Keypair(ss58_address=selected.address, crypto_type=crypto_type)
    if public.crypto_type != crypto_type:
        crypto_type = public.crypto_type

    return ExtensionAccountSelection(
        account=selected,
        public_key=public.public_key,
        crypto_type=crypto_type,
        ss58_format=public.ss58_format,
    )


async def open_extension_signer(
    bridge_url: str,
    selection: ExtensionAccountSelection,
) -> ExtensionSigner:
    """Open a fresh bridge connection for one signing session."""
    client = BridgeClient(bridge_url)
    await client.connect()
    return ExtensionSigner(
        selection.account,
        client,
        public_key=selection.public_key,
        crypto_type=selection.crypto_type,
        ss58_format=selection.ss58_format,
    )


async def connect_extension_signer(
    bridge_url: str,
    *,
    address: Optional[str] = None,
    source: Optional[str] = None,
    name: Optional[str] = None,
    interactive: bool = True,
    default_address: Optional[str] = None,
    on_picked: Optional[Callable[[ExtensionAccount], None]] = None,
) -> ExtensionSigner:
    """Connect to the bridge and return a signer for one extension account."""
    selection = await select_extension_account(
        bridge_url,
        address=address,
        source=source,
        name=name,
        interactive=interactive,
        default_address=default_address,
        on_picked=on_picked,
    )
    return await open_extension_signer(bridge_url, selection)
