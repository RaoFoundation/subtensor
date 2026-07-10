"""Ledger hardware signing via the Polkadot generic app.

:class:`LedgerSigner` satisfies the SDK's :class:`~bittensor.Signer` protocol
plus the two optional transport capabilities that make hardware clear-signing
work end to end:

- ``metadata_digest(SigningContext)`` — the RFC-0078 merkleized-metadata
  digest, signed into the payload via the ``CheckMetadataHash`` extension.
  The generic app refuses to blind-sign; this digest is how it trusts what it
  shows on screen.
- ``sign_unsigned_extrinsic(UnsignedExtrinsic)`` — receives the payload's
  wire seams, builds the metadata proof for exactly the types this extrinsic
  touches, and sends payload + proof to the device for on-screen review.

The device signs with ed25519 on the Polkadot derivation path
``m/44'/354'/account'/0'/index'`` — the same keys Ledger Live, Nova, Talisman
and SubWallet derive, so one device shows one set of addresses everywhere.

Everything device-shaped (HID discovery, APDU chunking) lives in the Rust
core (``bittensor_core.LedgerDevice``); this module is glue between the SDK's
signing protocols and that device handle.
"""

from __future__ import annotations

import asyncio
from typing import Optional

import bittensor_core as _backend

from ._transport.contract import SigningContext, UnsignedExtrinsic

__all__ = ["LedgerError", "LedgerSigner"]

LedgerError = _backend.LedgerError

# Bittensor chain constants baked into the runtime's metadata hash
# (runtime/build.rs: enable_metadata_hash("TAO", 9); System.SS58Prefix = 42).
_SS58_FORMAT = 42
_DECIMALS = 9
_TOKEN_SYMBOL = "TAO"


def _device_cls():
    device = getattr(_backend, "LedgerDevice", None)
    if device is None:
        raise LedgerError(
            "this bittensor-core build has no Ledger support "
            "(the wheel was built without the 'ledger' feature)"
        )
    return device


class LedgerSigner:
    """A :class:`~bittensor.Signer` backed by a Ledger device.

    Connects on construction (fail fast: device present, unlocked, Polkadot
    app open) and derives the address silently. Every ``sign`` round-trips to
    the device and blocks until the transaction is approved or rejected on
    its screen.

    ``account`` / ``index`` select the derivation path
    ``m/44'/354'/account'/0'/index'``.
    """

    crypto_type = 0  # ed25519: the scheme the generic app signs with

    def __init__(
        self,
        account: int = 0,
        index: int = 0,
        *,
        ss58_format: int = _SS58_FORMAT,
        decimals: int = _DECIMALS,
        token_symbol: str = _TOKEN_SYMBOL,
    ):
        self._account = account
        self._index = index
        self._ss58_format = ss58_format
        self._decimals = decimals
        self._token_symbol = token_symbol
        self._device = _device_cls()()
        public_key, address = self._device.address(account, index, ss58_format, False)
        self._public_key = bytes(public_key)
        self._ss58_address = address
        # The signing context from the most recent metadata_digest call; the
        # transport always resolves the digest before preparing the payload,
        # so this is populated by the time sign_unsigned_extrinsic runs.
        self._context: Optional[SigningContext] = None

    @property
    def ss58_address(self) -> str:
        return self._ss58_address

    @property
    def public_key(self) -> bytes:
        return self._public_key

    @property
    def ss58_format(self) -> int:
        return self._ss58_format

    def app_version(self) -> tuple[int, int, int]:
        """The generic app's version — also an "is the app open?" probe."""
        return self._device.app_version()

    def confirm_address(self) -> str:
        """Re-derive the address with on-device display; returns it after the
        user approves. Use to verify the address on the trusted screen."""
        _, address = self._device.address(self._account, self._index, self._ss58_format, True)
        return address

    # -- transport capabilities -------------------------------------------------

    def metadata_digest(self, context: SigningContext) -> bytes:
        """The RFC-0078 digest for the runtime this payload targets
        (:class:`~bittensor.SigningContext` carries the raw materials)."""
        self._context = context
        return bytes(
            _backend.metadata_digest(
                context.metadata_bytes,
                context.spec_version,
                context.spec_name,
                context.ss58_format,
                self._decimals,
                self._token_symbol,
            )
        )

    async def sign_unsigned_extrinsic(self, unsigned: UnsignedExtrinsic) -> bytes:
        """Clear-sign a prepared extrinsic on the device.

        Builds the metadata proof for exactly the types this extrinsic
        touches and ships payload + proof over HID. The device decodes and
        displays the transaction; the returned 65-byte MultiSignature carries
        its ed25519 version prefix (the transport already handles that).
        """
        context = self._context
        if context is None:
            raise LedgerError(
                "no signing context: the transport must resolve metadata_digest "
                "before signing (this signer cannot sign raw payloads)"
            )
        proof = _backend.generate_extrinsic_proof(
            unsigned.call_data,
            unsigned.included_in_extrinsic,
            unsigned.included_in_signed_data,
            context.metadata_bytes,
            context.spec_version,
            context.spec_name,
            context.ss58_format,
            self._decimals,
            self._token_symbol,
        )
        payload = (
            unsigned.call_data + unsigned.included_in_extrinsic + unsigned.included_in_signed_data
        )
        # Device I/O blocks until the user approves on-screen; keep the event
        # loop responsive.
        signature = await asyncio.to_thread(
            self._device.sign, payload, bytes(proof), self._account, self._index
        )
        return bytes(signature)

    def sign(self, payload: bytes) -> bytes:
        """Raw-payload signing is refused: the generic app only clear-signs.

        The transport never calls this (it prefers ``sign_unsigned_extrinsic``);
        anything else asking for a blind signature gets a clear error instead
        of a device that silently rejects.
        """
        raise LedgerError(
            "Ledger's generic app does not blind-sign raw payloads; "
            "sign through the SDK's extrinsic flow instead"
        )

    def __repr__(self) -> str:
        return (
            f"LedgerSigner({self._ss58_address}, "
            f"path=m/44'/354'/{self._account}'/0'/{self._index}')"
        )
