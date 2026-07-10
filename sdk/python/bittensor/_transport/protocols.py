from typing import Awaitable, Optional, Protocol, runtime_checkable

from .contract import SigningContext, UnsignedExtrinsic

__all__: list[str] = [
    "ExtensionPayloadSigner",
    "Keypair",
    "MetadataVerifyingSigner",
    "UnsignedExtrinsicSigner",
]


# For reference only
# class KeypairType:
#     """
#     Type of cryptography, used in `Keypair` instance to encrypt and sign data
#
#     * ED25519 = 0
#     * SR25519 = 1
#     * ECDSA = 2
#
#     """
#     ED25519 = 0
#     SR25519 = 1
#     ECDSA = 2


@runtime_checkable
class Keypair(Protocol):
    """The signer shape the transport signs with.

    ``sign`` may return the signature or a coroutine (remote and hardware
    signers). Signers may additionally match :class:`ExtensionPayloadSigner`
    and/or :class:`MetadataVerifyingSigner`; the transport checks for those
    capabilities at the signing choke points.
    """

    @property
    def crypto_type(self) -> int: ...

    @property
    def public_key(self) -> Optional[bytes]: ...

    @property
    def ss58_address(self) -> str: ...

    @property
    def ss58_format(self) -> int: ...

    def sign(self, data: bytes | str) -> bytes | Awaitable[bytes]: ...


@runtime_checkable
class ExtensionPayloadSigner(Protocol):
    """A signer that takes the Polkadot-JS ``SignerPayloadJSON`` instead of raw
    payload bytes (browser extensions and anything speaking their protocol).

    Returns (or resolves to) the extension's result dict, whose ``signature``
    key holds the 0x-hex MultiSignature (65 bytes: version prefix + signature).
    """

    def sign_extrinsic_payload(self, payload_json: dict) -> dict | Awaitable[dict]: ...


@runtime_checkable
class MetadataVerifyingSigner(Protocol):
    """A signer that verifies the runtime before signing (e.g. Ledger's generic
    app, which refuses to blind-sign).

    ``metadata_digest`` computes the RFC-0078 merkleized-metadata digest from
    the raw materials in :class:`SigningContext`; the transport signs it into
    the payload via the ``CheckMetadataHash`` extension and flips the matching
    mode byte in the assembled extrinsic.
    """

    def metadata_digest(self, context: SigningContext) -> bytes | Awaitable[bytes]: ...


@runtime_checkable
class UnsignedExtrinsicSigner(Protocol):
    """A signer that takes the whole prepared :class:`UnsignedExtrinsic`
    instead of raw payload bytes.

    Hardware devices that prove the runtime on-device (Ledger's generic app)
    need the payload's wire seams (``call_data`` / ``included_in_extrinsic`` /
    ``included_in_signed_data``) to build the RFC-0078 extrinsic proof they
    display and verify before signing — the flattened (and possibly
    blake2b-hashed) ``payload`` alone is not enough.
    """

    def sign_unsigned_extrinsic(self, unsigned: UnsignedExtrinsic) -> bytes | Awaitable[bytes]: ...
