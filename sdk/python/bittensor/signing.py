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
``Wallet``, a wallet name string (``"cold"`` or ``"cold/hot"``), anything
wallet-shaped (:class:`KeyedWallet` — e.g. an object carrying dev keypairs),
or a :class:`Signer` directly. ``as_wallet`` and ``resolve_signer`` at the
choke points normalize all of them.
"""

from __future__ import annotations

from typing import Any, Optional, Protocol, Union, runtime_checkable

from .ledger import LedgerSigner  # noqa: F401  (re-export: hardware signing backend)
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


# What every ``wallet`` parameter in the SDK accepts. A plain string is a
# wallet name ("cold" or "cold/hot"), resolved by ``as_wallet`` at the entry
# points before anything key-shaped is needed.
WalletLike = Union[Wallet, KeyedWallet, "Signer", str]


def as_wallet(wallet: WalletLike) -> Union[Wallet, KeyedWallet, "Signer"]:
    """Normalize the wallet shorthands: a string becomes a :class:`Wallet`
    (``"cold"`` or ``"cold/hot"``); everything else passes through."""
    if isinstance(wallet, str):
        return Wallet(wallet)
    return wallet


def as_ss58(value: Any, param: str = "") -> Any:
    """Normalize an address argument: a string passes through; a ``Wallet`` (or
    anything wallet-shaped) yields the key the parameter name asks for
    (``*hotkey*`` -> hotkey, otherwise coldkey); a keypair/:class:`Signer`
    yields its address. Unknown shapes pass through for downstream validation.

    Never unlocks or reads a private keyfile: wallet shapes go through
    :func:`public_view`, so the address comes from the public coldkeypub /
    hotkeypub files (a hotkey lookup on a wallet with no hotkey at all still
    raises the keyfile's own not-found error, which is the right message).
    """
    if value is None or isinstance(value, str):
        return value
    if isinstance(value, (Wallet, KeyedWallet)) and not isinstance(value, Signer):
        role = "hotkey" if "hotkey" in param else "coldkey"
        return public_view(value, role).ss58_address
    if hasattr(value, "ss58_address"):
        return value.ss58_address
    return value


def is_address_param(name: str) -> bool:
    """Whether a parameter or field name carries an ss58 address (``*_ss58``)
    or a list of them (``*_ss58s``) — the single predicate every coercion seam
    (intent fields, read parameters) matches against."""
    return name.endswith("_ss58") or name.endswith("_ss58s")


def coerce_address(value: Any, param: str) -> Any:
    """:func:`as_ss58` over a scalar or, for plural ``*_ss58s`` parameters, a
    list — returning the input unchanged when nothing needs coercing."""
    if isinstance(value, (list, tuple)):
        coerced = [as_ss58(item, param) for item in value]
        return coerced if any(a is not b for a, b in zip(coerced, value)) else value
    return as_ss58(value, param)


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
    ``Keypair`` also works — it already satisfies the protocol — and so does a
    wallet name string.
    """
    wallet = as_wallet(wallet)
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
    wallet = as_wallet(wallet)
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
