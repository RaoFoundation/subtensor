"""The signing seam: anything that can sign an extrinsic.

The SDK never requires a raw private key — it requires a :class:`Signer`: an
address, a public key, a crypto type, and a ``sign(payload)`` method. The
transport already awaits ``sign`` when it returns a coroutine, so signers that
round-trip to a device or a service (Ledger, OS keychain, remote signing box)
fit the same protocol without the SDK changing.

Beyond the required protocol, the transport matches two optional capabilities
structurally (no base class needed — just define the method):

- ``sign_extrinsic_payload(payload_json) -> {"signature": "0x..."}`` — for
  extension-style signers that take the Polkadot-JS ``SignerPayloadJSON``
  instead of raw payload bytes (:class:`bittensor.ExtensionSigner`).
- ``metadata_digest(SigningContext) -> bytes`` — for signers that verify the
  runtime before signing (Ledger's generic app refuses to blind-sign). The
  returned RFC-0078 merkleized-metadata digest is signed into the payload via
  the ``CheckMetadataHash`` extension, and the assembled extrinsic carries the
  matching mode byte.

Either hook may be sync or async. A signature of 65 bytes carries its
MultiSignature version prefix in the first byte (as hardware devices return).

For signers that can't be called in-process at all (QR / air-gapped vaults),
signing is split in two on ``Client``: ``prepare_call`` builds an
``UnsignedExtrinsic`` (exact payload bytes + Polkadot-JS payload JSON) from
just an address, and ``submit_signature`` reunites it with the signature.

Native keyfiles are the first backend (:class:`WalletSigner`).
Every SDK API that accepts a ``wallet`` accepts a :data:`WalletLike`: a
``Wallet``, anything wallet-shaped (:class:`KeyedWallet` — e.g. an object
carrying dev keypairs), or a :class:`Signer` directly. ``resolve_signer`` at
the signing choke points normalizes all of them.
"""

from __future__ import annotations

from typing import Any, Optional, Protocol, Union, runtime_checkable

from .wallet import Wallet
from .wallets import signing_keypair

_ROLES = ("coldkey", "hotkey")


@runtime_checkable
class Signer(Protocol):
    """Anything that can sign an extrinsic payload.

    ``ss58_address`` is the chain account address (ss58 encoding applies to both
    ed25519 and sr25519 keys). ``crypto_type`` selects the signature scheme:
    ``0`` = ed25519, ``1`` = sr25519.

    ``sign`` may be a plain function or a coroutine function — the transport
    awaits the result if needed, so hardware and remote signers can block on
    user approval without holding the event loop hostage.
    """

    @property
    def ss58_address(self) -> str: ...

    @property
    def public_key(self) -> bytes: ...

    @property
    def crypto_type(self) -> int: ...

    def sign(self, payload: bytes) -> bytes: ...


@runtime_checkable
class KeyedWallet(Protocol):
    """Anything wallet-shaped: it carries a coldkey, a coldkey public view, and
    a hotkey (each keypair-like, i.e. satisfying :class:`Signer`).

    ``Wallet`` matches this shape, and so does any plain
    object holding dev keypairs (e.g. a test double built from ``//Alice``).
    """

    @property
    def coldkey(self) -> Any: ...

    @property
    def coldkeypub(self) -> Any: ...

    @property
    def hotkey(self) -> Any: ...


# What every ``wallet`` parameter in the SDK accepts.
WalletLike = Union[Wallet, KeyedWallet, "Signer"]


class WalletSigner:
    """A :class:`Signer` backed by a native Bittensor keyfile.

    Public attributes (address, public key) come from the plaintext pub files,
    so building the signer, addressing, and fee estimation never unlock
    anything. The private key is unlocked lazily on the first ``sign`` via the
    password resolution chain (see ``wallets.resolve_wallet_password``) and
    kept for the signer's lifetime — one unlock per process, not per
    signature.
    """

    def __init__(
        self,
        wallet: Wallet,
        role: str = "coldkey",
        *,
        password: Optional[str] = None,
        password_file: Optional[str] = None,
        macos_prompt: bool = False,
        keychain: bool = False,
    ):
        if role not in _ROLES:
            raise ValueError(f"role must be one of {_ROLES}, got {role!r}")
        self._wallet = wallet
        self._role = role
        self._password = password
        self._password_file = password_file
        self._macos_prompt = macos_prompt
        self._keychain = keychain
        self._keypair: Any = None

    def _public(self) -> Any:
        """A public-only keypair — never triggers an unlock."""
        if self._keypair is not None:
            return self._keypair
        if self._role == "coldkey":
            return self._wallet.coldkeypub
        try:
            return self._wallet.hotkeypub
        except FileNotFoundError:
            # Legacy wallets may lack hotkeypub.txt; fall back to the hotkey
            # file (usually plaintext, but an encrypted one would unlock here).
            return self._wallet.hotkey

    def _unlock(self) -> Any:
        if self._keypair is None:
            self._keypair = signing_keypair(
                self._wallet,
                self._role,
                password=self._password,
                password_file=self._password_file,
                macos_prompt=self._macos_prompt,
                keychain=self._keychain,
            )
        return self._keypair

    @property
    def ss58_address(self) -> str:
        return self._public().ss58_address

    @property
    def public_key(self) -> bytes:
        return self._public().public_key

    @property
    def crypto_type(self) -> int:
        return self._public().crypto_type

    @property
    def ss58_format(self) -> int:
        return self._public().ss58_format

    def sign(self, payload: bytes) -> bytes:
        # The first unlock can block on getpass/Keychain/dialog subprocesses;
        # when called from async transport code that blocks the event loop
        # until the user responds. Unlock eagerly (or supply a password) to
        # avoid the pause.
        return self._unlock().sign(payload)

    def __repr__(self) -> str:
        state = "unlocked" if self._keypair is not None else "locked"
        return f"WalletSigner({self._wallet.name!r}, role={self._role!r}, {state})"


def resolve_signer(
    wallet: WalletLike,
    role: str = "coldkey",
    *,
    password: Optional[str] = None,
    password_file: Optional[str] = None,
    macos_prompt: bool = False,
    keychain: bool = False,
) -> Signer:
    """The signer for ``wallet``: a ``Wallet`` is wrapped, a ``Signer`` passes
    through, and a :class:`KeyedWallet` yields its keypair for ``role``.

    This is the single seam every signing path goes through. Passing a raw
    ``Keypair`` also works — it already satisfies the protocol.
    """
    if isinstance(wallet, Wallet):
        return WalletSigner(
            wallet,
            role,
            password=password,
            password_file=password_file,
            macos_prompt=macos_prompt,
            keychain=keychain,
        )
    if isinstance(wallet, Signer):
        return wallet
    if getattr(wallet, "uses_extension_signing", False):
        return wallet
    if isinstance(wallet, KeyedWallet):
        return wallet.coldkey if role == "coldkey" else wallet.hotkey
    raise TypeError(
        f"cannot sign with {type(wallet).__name__}: expected a Wallet, "
        "a wallet-shaped object (coldkey/coldkeypub/hotkey), or an object implementing "
        "bittensor.Signer"
    )


def public_view(wallet: WalletLike, role: str = "coldkey") -> Any:
    """An address/public-key view for fee and weight estimation.

    Never unlocks a private key: anything wallet-shaped yields its public
    coldkey view (or its hotkey, which is stored unencrypted); a ``Signer``
    already exposes its public parts without unlocking.
    """
    if isinstance(wallet, (Wallet, KeyedWallet)) and not isinstance(wallet, Signer):
        if role == "coldkey":
            return wallet.coldkeypub
        try:
            # Prefer the public-only hotkey view; not every wallet-shaped
            # object (or legacy wallet on disk) has one.
            return wallet.hotkeypub
        except (AttributeError, FileNotFoundError):
            return wallet.hotkey
    return wallet
