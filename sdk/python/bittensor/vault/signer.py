"""Air-gapped signing with Polkadot Vault (ex Parity Signer) via QR codes.

:class:`VaultSigner` satisfies the SDK's :class:`~bittensor.Signer` protocol
plus two transport capabilities:

- ``metadata_digest(SigningContext)`` — the RFC-0078 merkleized-metadata
  digest, signed into the payload via the ``CheckMetadataHash`` extension.
- ``sign_unsigned_extrinsic(UnsignedExtrinsic)`` — serves a local page
  showing the transaction as a proof-carrying UOS QR (type ``06``): the
  animated QR embeds the metadata proof for exactly the types this extrinsic
  touches, so the Vault phone decodes and displays the transaction with no
  pre-loaded metadata — nothing to re-scan after runtime upgrades. The user
  approves on the phone and the page scans Vault's signature QR back through
  the webcam.

The private key never leaves the phone; this process only ever sees the
account's public address and the returned signature (which it verifies
against the signing payload before handing it to the transport).

The phone still needs the Bittensor network added once (the chain-specs QR
in the docs guide, ``settings.VAULT_GUIDE_URL``); requires Polkadot Vault
recent enough to support proof-carrying transactions.
"""

from __future__ import annotations

import asyncio
from typing import Any, Callable, Optional

import bittensor_core as _backend

from .._transport.contract import SigningContext
from ..result import BittensorError
from ..settings import BLOCKTIME, SS58_FORMAT, VAULT_GUIDE_URL
from ..sp_core import CRYPTO_SR25519, ss58_decode, verify
from .qr import svg_data_uri
from .server import VaultPageError, VaultSessionServer
from .uos import transaction_frames, transaction_frames_with_proof

__all__ = ["VaultError", "VaultSigner"]

# Chain constants baked into the runtime's metadata hash (runtime/build.rs:
# enable_metadata_hash("TAO", 9)) — must match or CheckMetadataHash rejects.
_DECIMALS = 9
_TOKEN_SYMBOL = "TAO"


class VaultError(BittensorError):
    """A Vault signing round-trip failed (page error, bad scan, timeout)."""


# Fallback signature wait when the extrinsic is immortal (no era deadline).
_IMMORTAL_SIGN_TIMEOUT = 900.0


class VaultSigner:
    """A :class:`~bittensor.Signer` backed by a Polkadot Vault phone.

    Construction is offline and instant — it only decodes the ss58 address.
    The browser page (and its embedded webcam scanner) appears on the first
    ``sign_unsigned_extrinsic`` and stays up to report the submission result.

    ``crypto_type`` defaults to sr25519, the scheme Vault derives keys with.
    """

    def __init__(
        self,
        ss58_address: str,
        *,
        crypto_type: int = CRYPTO_SR25519,
        ss58_format: int = SS58_FORMAT,
        browser: Optional[str] = None,
        open_browser: bool = True,
        on_status: Optional[Callable[[str], None]] = None,
    ):
        self._address = ss58_address
        try:
            self._public_key = ss58_decode(ss58_address)
        except Exception as error:
            raise VaultError(f"invalid vault signer address {ss58_address!r}: {error}") from error
        self._crypto_type = crypto_type
        self._ss58_format = ss58_format
        self._browser = browser
        self._open_browser = open_browser
        self._on_status = on_status
        self._server: Optional[VaultSessionServer] = None
        # The signing context from the most recent metadata_digest call; the
        # transport always resolves the digest before preparing the payload,
        # so this is populated by the time sign_unsigned_extrinsic runs.
        self._context: Optional[SigningContext] = None
        # Display-only transaction context for the page (set by the CLI).
        self.summary: Optional[str] = None
        # MEV-shielded flows sign two extrinsics back to back (the shielded
        # transaction, then its encrypted carrier); the CLI flips this so the
        # page can label the stages and the user knows a second scan follows.
        self.two_stage: bool = False
        self._sign_index = 0

    @property
    def ss58_address(self) -> str:
        return self._address

    @property
    def public_key(self) -> bytes:
        return self._public_key

    @property
    def crypto_type(self) -> int:
        return self._crypto_type

    @property
    def ss58_format(self) -> int:
        return self._ss58_format

    def sign(self, payload: bytes) -> bytes:
        raise VaultError(
            "the vault signer only signs extrinsics (via QR round-trip); "
            "raw-byte signing is not supported"
        )

    def metadata_digest(self, context: SigningContext) -> bytes:
        """The RFC-0078 digest for the runtime this payload targets.

        Signed into the payload via ``CheckMetadataHash``; Vault verifies the
        embedded metadata proof against the same root, so what the phone
        displays is provably this runtime's decoding.
        """
        self._context = context
        return bytes(
            _backend.metadata_digest(
                context.metadata_bytes,
                context.spec_version,
                context.spec_name,
                context.ss58_format,
                _DECIMALS,
                _TOKEN_SYMBOL,
            )
        )

    async def sign_unsigned_extrinsic(self, unsigned: Any) -> bytes:
        """Show the proof-carrying transaction QR, wait for Vault's signature.

        Returns the raw signature bytes; a 65-byte result keeps its
        MultiSignature version prefix (the transport handles both shapes).
        The signature is verified against the signing payload before it is
        accepted, so a stale or mismatched scan fails here instead of
        on-chain with ``BadProof``.
        """
        context = self._context
        hashed = False
        if context is not None:
            proof = bytes(
                _backend.generate_extrinsic_proof(
                    unsigned.call_data,
                    unsigned.included_in_extrinsic,
                    unsigned.included_in_signed_data,
                    context.metadata_bytes,
                    context.spec_version,
                    context.spec_name,
                    context.ss58_format,
                    _DECIMALS,
                    _TOKEN_SYMBOL,
                )
            )
            frames = transaction_frames_with_proof(unsigned, proof)
        else:
            # No signing context (a caller outside the transport flow):
            # fall back to portal-metadata framing.
            frames, hashed = transaction_frames(unsigned)
        server = await self._ensure_server()
        self._sign_index += 1
        stage = None
        if self.two_stage:
            stage = (
                "scan 1 of 2 — the shielded transaction"
                if self._sign_index == 1
                else "scan 2 of 2 — the encrypted carrier"
            )
        deadline = _era_seconds(unsigned.era)
        self._status(
            f"scan the QR at {server.http_url} with Polkadot Vault, approve, "
            "then hold the phone's signature QR up to your webcam"
            + (f" ({stage})" if stage else "")
        )
        if hashed:
            self._status(
                "note: this call is too large to display — Vault will show only "
                "its hash (blind signing)"
            )
        summary = {
            "address": self._address,
            "genesisHash": unsigned.genesis_hash,
            "specVersion": unsigned.spec_version,
            "nonce": unsigned.nonce,
            "hashed": hashed,
            "deadlineSeconds": deadline,
            "text": self.summary,
            "stage": stage,
            "guideUrl": VAULT_GUIDE_URL,
        }
        try:
            scanned = await server.request_signature(
                frames=[svg_data_uri(frame) for frame in frames],
                frames_hex=[frame.hex() for frame in frames],
                summary=summary,
                sign_timeout=deadline or _IMMORTAL_SIGN_TIMEOUT,
            )
        except VaultPageError as error:
            raise VaultError(str(error)) from error
        return self._validated_signature(scanned, unsigned)

    async def warm_up(self) -> None:
        """Open the page and start its camera before a time-critical flow.

        MEV-shielded signing runs against an 8-block (~96 s) era: warming up
        first means the countdown starts only once you are ready to scan.
        """
        server = await self._ensure_server()
        self._status(
            f"get ready at {server.http_url} — unlock Polkadot Vault and open "
            "its scanner before continuing"
        )
        try:
            await server.warm_up()
        except VaultPageError as error:
            raise VaultError(str(error)) from error

    def _validated_signature(self, scanned: str, unsigned: Any) -> bytes:
        text = scanned.strip().removeprefix("0x")
        try:
            signature = bytes.fromhex(text)
        except ValueError:
            raise VaultError(
                f"the scanned QR is not a signature (got {len(text)} chars of non-hex data); "
                "scan the QR Vault shows *after* you approve the transaction"
            ) from None
        if len(signature) not in (64, 65):
            raise VaultError(
                f"the scanned QR holds {len(signature)} bytes, not a 64/65-byte signature"
            )
        raw = signature[1:] if len(signature) == 65 else signature
        crypto_type = signature[0] if len(signature) == 65 else self._crypto_type
        try:
            valid = verify(unsigned.payload, raw, self._address, crypto_type)
        except Exception:
            valid = False
        if not valid:
            raise VaultError(
                "the scanned signature does not verify against this transaction "
                f"for {self._address} — was a stale QR scanned, or did a different "
                "key sign?"
            )
        return bytes(signature)

    async def report_transaction_result(self, success: bool) -> None:
        """Flip the page to its success/failure state after submission."""
        if self._server is not None:
            await self._server.report_result(success)

    async def close(self) -> None:
        if self._server is not None:
            # Give the page a beat to paint the result before the socket drops.
            await asyncio.sleep(0.25)
            await self._server.stop()
            self._server = None

    async def _ensure_server(self) -> VaultSessionServer:
        if self._server is None:
            server = VaultSessionServer()
            await server.start(open_browser=self._open_browser, browser=self._browser)
            self._server = server
        return self._server

    def _status(self, message: str) -> None:
        if self._on_status is not None:
            self._on_status(message)

    def __repr__(self) -> str:
        return f"VaultSigner({self._address!r})"


def _era_seconds(era: Any) -> Optional[float]:
    """How long a mortal era stays valid, or None for immortal extrinsics."""
    if isinstance(era, dict) and era.get("period"):
        return float(era["period"]) * BLOCKTIME
    return None
