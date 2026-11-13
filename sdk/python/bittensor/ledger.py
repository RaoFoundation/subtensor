"""Ledger hardware signing via the Polkadot generic app.

:class:`LedgerSigner` satisfies the SDK's :class:`~bittensor.Signer` protocol
plus the two optional transport capabilities that make hardware clear-signing
work end to end:

- ``metadata_digest(SigningContext)`` — the RFC-0078 merkleized-metadata
  digest, signed into the payload via the ``CheckMetadataHash`` extension.
  This clear-signing path uses the digest to verify what it shows on screen.
- ``sign_unsigned_extrinsic(UnsignedExtrinsic)`` — receives the payload's
  wire seams, builds the metadata proof for exactly the types this extrinsic
  touches, and sends payload + proof to the device for on-screen review.

The device signs with ed25519 on the Polkadot derivation path
``m/44'/354'/account'/0'/index'`` — the same keys Ledger Live, Nova, Talisman
and SubWallet derive, so one device shows one set of addresses everywhere.

Everything device-shaped (HID discovery, APDU chunking) lives in the Rust
core (``bittensor_core.LedgerDevice``); this module is glue between the SDK's
signing protocols and that device handle.

``LedgerHashedSigner`` is a separate, explicit message-signing adapter for
rotating hashed Ed25519 accounts. The device displays a wrapped digest, not
decoded transaction details; ordinary ``LedgerSigner`` never uses this path.
"""

from __future__ import annotations

import asyncio
from copy import copy
from hashlib import blake2b
from typing import Optional

import bittensor_core as _backend

from ._transport.contract import SigningContext, UnsignedExtrinsic

__all__ = ["LedgerError", "LedgerHashedSigner", "LedgerSigner"]

LedgerError = _backend.LedgerError

# Bittensor chain constants baked into the runtime's metadata hash
# (runtime/build.rs: enable_metadata_hash("TAO", 9); System.SS58Prefix = 42).
_SS58_FORMAT = 42
_DECIMALS = 9
_TOKEN_SYMBOL = "TAO"
_HARDENED = 2**31
_HASHED_ED25519 = 6
_HASHED_DESCRIPTOR_PREFIX = b"\x01\x03"
_HASHED_KEY_DOMAIN = b"bittensor/hashed/v1/key"
_HASHED_TRANSACTION_DOMAIN = b"bittensor/hashed/v1/transaction"
_WRAPPED_SIGNER_VERSION = 0x81


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
        """Raw-payload signing is refused: this signer only clear-signs.

        The transport never calls this (it prefers ``sign_unsigned_extrinsic``);
        anything else asking for a blind signature gets a clear error instead
        of a device that silently rejects.
        """
        raise LedgerError(
            "LedgerSigner does not blind-sign raw payloads; "
            "sign through the SDK's extrinsic flow instead"
        )

    def __repr__(self) -> str:
        return (
            f"LedgerSigner({self._ss58_address}, "
            f"path=m/44'/354'/{self._account}'/0'/{self._index}')"
        )


def _hardened_component(value: int, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value < _HARDENED:
        raise ValueError(f"Ledger {name} must be an integer from 0 to 2^31 - 1")
    return value


def _hashed_key_commitment(public_key: bytes) -> bytes:
    return blake2b(
        _HASHED_KEY_DOMAIN + _HASHED_DESCRIPTOR_PREFIX + public_key, digest_size=32
    ).digest()


class LedgerHashedSigner:
    """An opt-in hashed Ed25519 signer using Ledger wrapped-message signatures.

    Reserve a dedicated ``account`` for this signer: generation ``g`` uses
    ``m/44'/354'/account'/0'/g'`` and commits to the next index.
    Do not reuse the account's index sequence for ordinary wallets or another
    hashed signer. The permanent account ID comes from the initial key's
    version-1 descriptor; it differs from the device's ordinary SS58 address.

    The chain determines the current generation, through the SDK's normal
    hashed signing flow. There is no local rotation counter or private-key
    file. Recreate this object with the same device and account
    to recover it. The Polkadot app must support wrapped-message signing.

    This mode displays a 32-byte transaction digest on the device rather than
    clear-signing transaction details. Review the transaction in the caller
    before approving; use ``LedgerSigner`` for ordinary clear-signing.
    """

    crypto_type = _HASHED_ED25519
    hashed_signature_version = _WRAPPED_SIGNER_VERSION

    def __init__(self, account: int, *, ss58_format: int = _SS58_FORMAT):
        self._account = _hardened_component(account, "account")
        self._generation = 0
        self._ss58_format = ss58_format
        self._device = _device_cls()()
        self._public_keys: dict[int, bytes] = {}
        initial = self._derive_public(0)
        self._descriptor = _HASHED_DESCRIPTOR_PREFIX + _hashed_key_commitment(initial)
        self._public = _backend.Keypair.from_hashed_descriptor(self._descriptor, ss58_format)

    def _path_index(self, generation: int) -> int:
        return _hardened_component(generation, "generation")

    def _derive_public(self, generation: int) -> bytes:
        index = self._path_index(generation)
        if generation not in self._public_keys:
            public_key, _ = self._device.address(self._account, index, self._ss58_format, False)
            public_key = bytes(public_key)
            if len(public_key) != 32:
                raise LedgerError("device returned an invalid Ed25519 public key")
            self._public_keys[generation] = public_key
        return self._public_keys[generation]

    @property
    def public_key(self) -> bytes:
        """The permanent account ID, not the rotating device public key."""
        return bytes(self._public.public_key)

    @property
    def ss58_address(self) -> str:
        return self._public.ss58_address

    @property
    def ss58_format(self) -> int:
        return self._ss58_format

    @property
    def hashed_descriptor(self) -> bytes:
        return self._descriptor

    @property
    def hashed_generation(self) -> int:
        return self._generation

    @property
    def hashed_signing_public_key(self) -> bytes:
        return self._derive_public(self._generation)

    @property
    def hashed_public_key(self) -> bytes:
        return self.hashed_signing_public_key

    @property
    def hashed_current_commitment(self) -> bytes:
        return self.hashed_commitment(self._generation)

    @property
    def hashed_next_commitment(self) -> bytes:
        return self.hashed_commitment(self._generation + 1)

    def hashed_commitment(self, generation: int) -> bytes:
        return _hashed_key_commitment(self._derive_public(generation))

    def hashed_receiving_address(self) -> str:
        return self._public.hashed_receiving_address()

    def public_only(self):
        return self._public.public_only()

    def at_generation(self, generation: int) -> LedgerHashedSigner:
        """Select chain-reported authority without changing the account identity."""
        self._path_index(generation)
        self._path_index(generation + 1)
        selected = copy(self)
        selected._generation = generation
        return selected

    def sign(self, payload: bytes) -> bytes:
        raise LedgerError("hashed Ledger accounts must use the synchronized hashed signing flow")

    async def sign_hashed(self, payload: bytes) -> bytes:
        """Sign the complete unshortened payload with explicit wrapped mode.

        The SDK checks the live account commitment and runtime support before
        calling this method. This method independently checks the device's
        signature against the exact wrapped digest and expected rotating key.
        """
        payload = bytes(payload)
        current = await asyncio.to_thread(self._derive_public, self._generation)
        next_commitment = await asyncio.to_thread(self.hashed_commitment, self._generation + 1)
        generation = self._generation.to_bytes(8, "little")
        signer_address = (
            self.public_key
            + bytes([self.hashed_signature_version, _HASHED_DESCRIPTOR_PREFIX[1]])
            + generation
            + current
            + next_commitment
        )
        digest = blake2b(
            _HASHED_TRANSACTION_DOMAIN + signer_address + payload, digest_size=32
        ).digest()
        message = b"<Bytes>" + digest + b"</Bytes>"
        sign_message = getattr(self._device, "sign_message", None)
        if sign_message is None:
            raise LedgerError("this bittensor-core build has no Ledger message-signing support")
        signature = bytes(
            await asyncio.to_thread(
                sign_message, message, self._account, self._path_index(self._generation)
            )
        )
        if len(signature) != 65 or signature[0] != 0:
            raise LedgerError("expected a 65-byte Ed25519 MultiSignature from the device")
        address = _backend.ss58_encode(current, self._ss58_format)
        if not _backend.verify_strict(message, signature[1:], address, 0):
            raise LedgerError("device signature does not match the requested hashed authorization")
        return (
            bytes([self.hashed_signature_version])
            + generation
            + current
            + next_commitment
            + signature[1:]
        )

    def __repr__(self) -> str:
        return (
            f"LedgerHashedSigner({self.ss58_address}, "
            f"account={self._account}, generation={self._generation})"
        )
