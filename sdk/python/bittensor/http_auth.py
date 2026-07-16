"""Hotkey-signed HTTP requests: the ``btauth/1`` protocol.

The identity layer that replaced the v10 Axon/Dendrite stack. Any HTTP request
can prove "sent by hotkey X, to hotkey Y, with exactly this body, recently" via
a handful of headers — no server, no client, no schema objects. The HTTP layer
itself (FastAPI, httpx, aiohttp, another language entirely) is the
application's choice; this module only signs and verifies.

Client side::

    import bittensor as bt

    wallet = bt.Wallet(name="my_coldkey", hotkey="my_hotkey")
    body = b'{"prompt": "hello"}'
    headers = bt.http_auth.sign(
        wallet, method="POST", path="/generate", body=body,
        receiver_ss58=miner_hotkey,
    )
    # send `body` with `headers` using any HTTP client

Server side::

    caller = bt.http_auth.verify(
        request_headers, raw_body,
        method="POST", path="/generate",
        self_hotkey_ss58=my_hotkey,
    )
    caller.hotkey_ss58   # the authenticated sender

``verify`` raises an :class:`AuthError` subclass on failure (map it to
HTTP 401/403 in your framework). Replay protection is a freshness window on
the nanosecond-timestamp nonce plus a seen-nonce cache
(:class:`InMemoryNonceStore` by default; supply a :class:`NonceStore` for
multi-process servers).

The wire format is deliberately language-neutral — sha256 over the raw body
bytes, a newline-joined signed payload, sr25519/ed25519 signatures — so
miners and validators in other languages interoperate from the spec alone.
See the signed-requests guide (/docs/guides/signed-requests) for the
normative spec, framework recipes, and golden test vectors.
"""

from __future__ import annotations

import hashlib
import threading
import time
from collections import deque
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Optional, Protocol

from .result import BittensorError
from .signing import WalletLike, resolve_signer
from .sp_core import verify as _sp_core_verify
from .wallets import CRYPTO_TYPE_NAMES, parse_crypto_type

PROTOCOL = "btauth/1"
VERSION = "1"

HEADER_VERSION = "X-Bittensor-Version"
HEADER_HOTKEY = "X-Bittensor-Hotkey"
HEADER_RECEIVER = "X-Bittensor-Receiver"
HEADER_NONCE = "X-Bittensor-Nonce"
HEADER_CRYPTO = "X-Bittensor-Crypto"
HEADER_SIGNATURE = "X-Bittensor-Signature"

DEFAULT_MAX_AGE = 10.0
DEFAULT_ALLOWED_SKEW = 2.0

# Payload line for a request not bound to a receiver (verify() rejects these
# unless require_receiver=False).
_NO_RECEIVER = "-"

_DEFAULT_SCHEME = "sr25519"


class AuthError(BittensorError):
    """A signed request failed authentication.

    Subclasses say why; messages never echo the signature or payload, so they
    are safe to return to the (unauthenticated) caller.
    """


class MalformedAuth(AuthError):
    """A required header is missing or unparseable."""


class WrongReceiver(AuthError):
    """The request was signed for a different (or no) receiving hotkey."""


class StaleRequest(AuthError):
    """The nonce is outside the freshness window (too old, or too far ahead)."""


class ReplayedRequest(AuthError):
    """This (hotkey, nonce) pair was already accepted inside the window."""


class BadSignature(AuthError):
    """The signature does not verify against the claimed hotkey."""


@dataclass(frozen=True)
class Caller:
    """The authenticated sender of a verified request."""

    hotkey_ss58: str
    nonce_ns: int
    crypto_type: int


class NonceStore(Protocol):
    """Replay-protection state: remembers accepted (hotkey, nonce) pairs.

    ``check_and_store`` returns True (and records the pair) if it was never
    seen, False if it was. Entries only need to outlive the freshness window —
    anything older is already rejected as stale before the store is consulted.
    """

    def check_and_store(self, hotkey_ss58: str, nonce_ns: int) -> bool: ...


class InMemoryNonceStore:
    """Default per-process :class:`NonceStore`: a dict with time-based expiry.

    ``retention`` must be at least ``max_age + allowed_skew`` of the
    verifier's window; the default comfortably covers the defaults. For
    multi-process or multi-host servers back this with shared storage
    (e.g. Redis SET NX EX) instead.
    """

    def __init__(self, retention: float = 60.0):
        self.retention = retention
        self._seen: set[tuple[str, int]] = set()
        self._expiry: deque[tuple[float, tuple[str, int]]] = deque()
        self._lock = threading.Lock()

    def check_and_store(self, hotkey_ss58: str, nonce_ns: int) -> bool:
        key = (hotkey_ss58, nonce_ns)
        now = time.monotonic()
        with self._lock:
            while self._expiry and self._expiry[0][0] <= now:
                self._seen.discard(self._expiry.popleft()[1])
            if key in self._seen:
                return False
            self._seen.add(key)
            self._expiry.append((now + self.retention, key))
            return True


_default_nonce_store = InMemoryNonceStore()


def build_payload(
    *,
    scheme: str,
    method: str,
    path: str,
    body: bytes,
    nonce_ns: int,
    sender_ss58: str,
    receiver_ss58: Optional[str],
) -> bytes:
    """The exact bytes that are signed — the normative ``btauth/1`` payload.

    Newline-joined UTF-8: protocol version, signature scheme, uppercase
    method, request target (path plus ``?query`` as sent), lowercase-hex
    sha256 of the raw body, decimal nanosecond nonce, sender ss58, and the
    receiver ss58 (``-`` when unbound). Exposed so tests and other-language
    implementations can pin it byte-for-byte.
    """
    lines = (
        PROTOCOL,
        scheme,
        method.upper(),
        path,
        hashlib.sha256(body).hexdigest(),
        str(nonce_ns),
        sender_ss58,
        receiver_ss58 or _NO_RECEIVER,
    )
    return "\n".join(lines).encode()


def sign(
    wallet: WalletLike,
    *,
    method: str,
    path: str,
    body: bytes = b"",
    receiver_ss58: Optional[str] = None,
    nonce_ns: Optional[int] = None,
) -> dict[str, str]:
    """Sign a request with the wallet's hotkey; returns the auth headers.

    ``wallet`` is anything :func:`bittensor.resolve_signer` accepts (a
    ``Wallet``, a raw keypair, a custom ``Signer``); the hotkey role is used.
    ``path`` must be the request target exactly as the client will send it,
    including the query string. ``nonce_ns`` is injectable for tests; each
    retry of a request must sign a fresh nonce.

    The signature scheme follows the signer's ``crypto_type``; the
    ``X-Bittensor-Crypto`` header is emitted only for non-default schemes
    (absent means sr25519).
    """
    signer = resolve_signer(wallet, role="hotkey")
    scheme = CRYPTO_TYPE_NAMES.get(signer.crypto_type)
    if scheme is None:
        raise ValueError(f"cannot sign btauth requests with crypto type {signer.crypto_type}")
    nonce = time.time_ns() if nonce_ns is None else int(nonce_ns)
    payload = build_payload(
        scheme=scheme,
        method=method,
        path=path,
        body=body,
        nonce_ns=nonce,
        sender_ss58=signer.ss58_address,
        receiver_ss58=receiver_ss58,
    )
    signature = signer.sign(payload)
    if not isinstance(signature, (bytes, bytearray)):
        raise TypeError(
            "http_auth.sign requires a synchronous signer; "
            f"sign() returned {type(signature).__name__}"
        )
    headers = {
        HEADER_VERSION: VERSION,
        HEADER_HOTKEY: signer.ss58_address,
        HEADER_NONCE: str(nonce),
        HEADER_SIGNATURE: "0x" + bytes(signature).hex(),
    }
    if scheme != _DEFAULT_SCHEME:
        headers[HEADER_CRYPTO] = scheme
    if receiver_ss58 is not None:
        headers[HEADER_RECEIVER] = receiver_ss58
    return headers


def verify(
    headers: Mapping[str, str],
    body: bytes,
    *,
    method: str,
    path: str,
    self_hotkey_ss58: str,
    max_age: float = DEFAULT_MAX_AGE,
    allowed_skew: float = DEFAULT_ALLOWED_SKEW,
    require_receiver: bool = True,
    nonce_store: Optional[NonceStore] = None,
    now_ns: Optional[int] = None,
) -> Caller:
    """Authenticate a request; returns the :class:`Caller` or raises :class:`AuthError`.

    ``body`` must be the raw received bytes, hashed before any parsing or
    re-serialization; ``method`` and ``path`` come from the received request
    (path including the query string). The body hash is always recomputed —
    nothing integrity-relevant is taken from the headers unverified.

    Checks, in order: header shape (:class:`MalformedAuth`), receiver binding
    (:class:`WrongReceiver`), freshness — ``max_age`` seconds back,
    ``allowed_skew`` seconds of forward clock drift (:class:`StaleRequest`),
    the signature under the declared scheme only, never try-both
    (:class:`BadSignature`), and replay (:class:`ReplayedRequest`). The replay
    store is written only after the signature verifies, so unauthenticated
    traffic cannot poison it against a legitimate sender.
    """
    lowered = {key.lower(): value for key, value in headers.items()}

    version = lowered.get(HEADER_VERSION.lower())
    if version != VERSION:
        raise MalformedAuth(f"missing or unsupported {HEADER_VERSION} (expected {VERSION!r})")
    sender = lowered.get(HEADER_HOTKEY.lower())
    if not sender:
        raise MalformedAuth(f"missing {HEADER_HOTKEY}")
    raw_nonce = lowered.get(HEADER_NONCE.lower())
    if raw_nonce is None:
        raise MalformedAuth(f"missing {HEADER_NONCE}")
    try:
        nonce = int(raw_nonce)
    except ValueError:
        raise MalformedAuth(f"{HEADER_NONCE} is not a decimal integer") from None
    try:
        crypto_type = parse_crypto_type(lowered.get(HEADER_CRYPTO.lower(), _DEFAULT_SCHEME))
    except ValueError:
        raise MalformedAuth(f"unsupported {HEADER_CRYPTO}") from None
    # Rebuild the canonical scheme name rather than echoing the header value,
    # so the signed bytes are never client-influenced beyond the scheme choice.
    scheme = CRYPTO_TYPE_NAMES[crypto_type]
    raw_signature = lowered.get(HEADER_SIGNATURE.lower())
    if raw_signature is None:
        raise MalformedAuth(f"missing {HEADER_SIGNATURE}")
    try:
        signature = bytes.fromhex(raw_signature.removeprefix("0x"))
    except ValueError:
        raise MalformedAuth(f"{HEADER_SIGNATURE} is not hex") from None

    receiver = lowered.get(HEADER_RECEIVER.lower())
    if receiver is None:
        if require_receiver:
            raise WrongReceiver(
                f"missing {HEADER_RECEIVER}: this server requires receiver-bound requests"
            )
    elif receiver != self_hotkey_ss58:
        raise WrongReceiver("request was signed for a different receiving hotkey")

    now = time.time_ns() if now_ns is None else now_ns
    if now - nonce > max_age * 1e9:
        raise StaleRequest(f"nonce is older than the {max_age:g}s freshness window")
    if nonce - now > allowed_skew * 1e9:
        raise StaleRequest(f"nonce is more than {allowed_skew:g}s in the future")

    payload = build_payload(
        scheme=scheme,
        method=method,
        path=path,
        body=body,
        nonce_ns=nonce,
        sender_ss58=sender,
        receiver_ss58=receiver,
    )
    try:
        valid = _sp_core_verify(payload, signature, sender, crypto_type)
    except ValueError:
        # Undecodable ss58 address or wrong-length signature bytes.
        raise MalformedAuth("invalid hotkey address or signature encoding") from None
    if not valid:
        raise BadSignature("signature does not verify against the claimed hotkey")

    store = nonce_store if nonce_store is not None else _default_nonce_store
    retention = getattr(store, "retention", None)
    if retention is not None and max_age + allowed_skew > retention:
        raise ValueError(
            f"nonce store retains entries for {retention:g}s but the freshness window is "
            f"{max_age + allowed_skew:g}s — replays inside the gap would be accepted; "
            "use a store with retention >= max_age + allowed_skew"
        )
    if not store.check_and_store(sender, nonce):
        raise ReplayedRequest("this nonce was already accepted from this hotkey")

    return Caller(hotkey_ss58=sender, nonce_ns=nonce, crypto_type=crypto_type)


__all__ = [
    "DEFAULT_ALLOWED_SKEW",
    "DEFAULT_MAX_AGE",
    "HEADER_CRYPTO",
    "HEADER_HOTKEY",
    "HEADER_NONCE",
    "HEADER_RECEIVER",
    "HEADER_SIGNATURE",
    "HEADER_VERSION",
    "PROTOCOL",
    "VERSION",
    "AuthError",
    "BadSignature",
    "Caller",
    "InMemoryNonceStore",
    "MalformedAuth",
    "NonceStore",
    "ReplayedRequest",
    "StaleRequest",
    "WrongReceiver",
    "build_payload",
    "sign",
    "verify",
]
