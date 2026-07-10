"""Conformance and behavior tests for ``bittensor.http_auth`` (btauth/1).

The golden vectors here are duplicated in the signed-requests guide
(docs/guides/signed-requests.mdx) — they are the cross-language conformance
fixtures, so a change that breaks these tests breaks every non-Python
implementation and needs a protocol version bump, not a fixture update.
"""

from __future__ import annotations

import hashlib

import pytest

from bittensor import http_auth
from bittensor.result import BittensorError
from bittensor.sp_core import CRYPTO_ED25519, CRYPTO_SR25519, Keypair

FIXED_NONCE = 1_752_076_800_000_000_000  # 2025-07-09T16:00:00Z in ns
BODY = b'{"prompt": "hello"}'
METHOD = "POST"
PATH = "/generate?stream=false"

GOLDEN_SR25519_PAYLOAD = b"""btauth/1
sr25519
POST
/generate?stream=false
341c57448e531310fbbe83f44cea2a5e838bd9e8a6b82b269f01d0dbbc23c3cc
1752076800000000000
5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY
5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty"""

GOLDEN_ED25519_SIGNATURE = (
    "0x1a6b631988bf1f7ea093ab9d5acf61f05c316d0b0e428b24b487b75a5f6ee0e1"
    "a56a26918fdf871b6cf09437e6588023a4ce423cf622621b2a7feee17612f704"
)


@pytest.fixture(scope="module")
def alice_sr() -> Keypair:
    return Keypair.create_from_uri("//Alice", CRYPTO_SR25519)


@pytest.fixture(scope="module")
def alice_ed() -> Keypair:
    return Keypair.create_from_uri("//Alice", CRYPTO_ED25519)


@pytest.fixture(scope="module")
def bob() -> Keypair:
    return Keypair.create_from_uri("//Bob", CRYPTO_SR25519)


def make_headers(sender: Keypair, bob: Keypair, **overrides) -> dict[str, str]:
    kwargs = dict(
        method=METHOD,
        path=PATH,
        body=BODY,
        receiver_ss58=bob.ss58_address,
        nonce_ns=FIXED_NONCE,
    )
    kwargs.update(overrides)
    return http_auth.sign(sender, **kwargs)


def check(headers, bob: Keypair, *, body: bytes = BODY, **overrides) -> http_auth.Caller:
    kwargs = dict(
        method=METHOD,
        path=PATH,
        self_hotkey_ss58=bob.ss58_address,
        nonce_store=http_auth.InMemoryNonceStore(),
        now_ns=FIXED_NONCE + 1_000_000,
    )
    kwargs.update(overrides)
    return http_auth.verify(headers, body, **kwargs)


# --- golden vectors (cross-language conformance) ---


def test_golden_payload_bytes(alice_sr: Keypair, bob: Keypair) -> None:
    payload = http_auth.build_payload(
        scheme="sr25519",
        method=METHOD,
        path=PATH,
        body=BODY,
        nonce_ns=FIXED_NONCE,
        sender_ss58=alice_sr.ss58_address,
        receiver_ss58=bob.ss58_address,
    )
    assert payload == GOLDEN_SR25519_PAYLOAD
    assert hashlib.sha256(BODY).hexdigest() == GOLDEN_SR25519_PAYLOAD.splitlines()[4].decode()


def test_golden_ed25519_signature(alice_ed: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_ed, bob)
    assert headers["X-Bittensor-Signature"] == GOLDEN_ED25519_SIGNATURE
    assert headers["X-Bittensor-Crypto"] == "ed25519"
    assert alice_ed.ss58_address == "5FA9nQDVg267DEd8m1ZypXLBnvN7SFxYwV7ndqSYGiN9TTpu"


def test_golden_addresses(alice_sr: Keypair, bob: Keypair) -> None:
    assert alice_sr.ss58_address == "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
    assert bob.ss58_address == "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty"


# --- round trips ---


@pytest.mark.parametrize("crypto_type", [CRYPTO_SR25519, CRYPTO_ED25519])
def test_round_trip(crypto_type: int, bob: Keypair) -> None:
    sender = Keypair.create_from_uri("//Alice", crypto_type)
    caller = check(make_headers(sender, bob), bob)
    assert caller == http_auth.Caller(sender.ss58_address, FIXED_NONCE, crypto_type)


def test_round_trip_live_nonce_get_empty_body(alice_sr: Keypair, bob: Keypair) -> None:
    headers = http_auth.sign(alice_sr, method="GET", path="/health", receiver_ss58=bob.ss58_address)
    caller = http_auth.verify(
        headers,
        b"",
        method="GET",
        path="/health",
        self_hotkey_ss58=bob.ss58_address,
        nonce_store=http_auth.InMemoryNonceStore(),
    )
    assert caller.hotkey_ss58 == alice_sr.ss58_address


def test_sr25519_crypto_header_omitted(alice_sr: Keypair, bob: Keypair) -> None:
    assert "X-Bittensor-Crypto" not in make_headers(alice_sr, bob)


def test_headers_survive_lowercasing(alice_sr: Keypair, bob: Keypair) -> None:
    lowered = {k.lower(): v for k, v in make_headers(alice_sr, bob).items()}
    assert check(lowered, bob).hotkey_ss58 == alice_sr.ss58_address


@pytest.mark.parametrize("spelling", ["SR25519", "1"])
def test_non_canonical_scheme_spellings(alice_sr: Keypair, bob: Keypair, spelling: str) -> None:
    headers = dict(make_headers(alice_sr, bob), **{"X-Bittensor-Crypto": spelling})
    assert check(headers, bob).hotkey_ss58 == alice_sr.ss58_address


# --- freshness and replay ---


def test_stale_and_future_nonces(alice_sr: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_sr, bob)
    with pytest.raises(http_auth.StaleRequest):
        check(headers, bob, now_ns=FIXED_NONCE + int(11e9))
    with pytest.raises(http_auth.StaleRequest):
        check(headers, bob, now_ns=FIXED_NONCE - int(3e9))


def test_replay_rejected(alice_sr: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_sr, bob)
    store = http_auth.InMemoryNonceStore()
    check(headers, bob, nonce_store=store)
    with pytest.raises(http_auth.ReplayedRequest):
        check(headers, bob, nonce_store=store)


def test_window_wider_than_store_retention_refused(alice_sr: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_sr, bob)
    with pytest.raises(ValueError, match="retention"):
        check(headers, bob, max_age=120.0)
    caller = check(
        headers, bob, max_age=120.0, nonce_store=http_auth.InMemoryNonceStore(retention=300.0)
    )
    assert caller.hotkey_ss58 == alice_sr.ss58_address


def test_nonce_store_expires_entries() -> None:
    store = http_auth.InMemoryNonceStore(retention=0.0)
    assert store.check_and_store("5F...", 1)
    assert store.check_and_store("5F...", 1)  # expired immediately, seen again


# --- tampering ---


def test_tampered_nonce_rejected(alice_sr: Keypair, bob: Keypair) -> None:
    headers = dict(make_headers(alice_sr, bob), **{"X-Bittensor-Nonce": str(FIXED_NONCE + 1)})
    with pytest.raises(http_auth.BadSignature):
        check(headers, bob, now_ns=FIXED_NONCE + 2)


def test_tampered_body_method_path_rejected(alice_sr: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_sr, bob)
    with pytest.raises(http_auth.BadSignature):
        check(headers, bob, body=b"tampered body")
    with pytest.raises(http_auth.BadSignature):
        check(headers, bob, method="DELETE")
    with pytest.raises(http_auth.BadSignature):
        check(headers, bob, path="/admin")


def test_scheme_confusion_rejected(alice_ed: Keypair, bob: Keypair) -> None:
    # An ed25519 signature presented as sr25519 (crypto header stripped) must
    # fail — verification never falls back to trying the other scheme.
    headers = dict(make_headers(alice_ed, bob))
    del headers["X-Bittensor-Crypto"]
    with pytest.raises(http_auth.BadSignature):
        check(headers, bob)


# --- receiver binding ---


def test_wrong_receiver_rejected(alice_sr: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_sr, bob)
    with pytest.raises(http_auth.WrongReceiver):
        check(headers, bob, self_hotkey_ss58=alice_sr.ss58_address)


def test_unbound_request_requires_opt_out(alice_sr: Keypair, bob: Keypair) -> None:
    headers = make_headers(alice_sr, bob, receiver_ss58=None)
    with pytest.raises(http_auth.WrongReceiver):
        check(headers, bob)
    assert check(headers, bob, require_receiver=False).hotkey_ss58 == alice_sr.ss58_address


# --- malformed input ---


@pytest.mark.parametrize(
    "missing",
    ["X-Bittensor-Version", "X-Bittensor-Hotkey", "X-Bittensor-Nonce", "X-Bittensor-Signature"],
)
def test_missing_header_rejected(alice_sr: Keypair, bob: Keypair, missing: str) -> None:
    headers = {k: v for k, v in make_headers(alice_sr, bob).items() if k != missing}
    with pytest.raises(http_auth.MalformedAuth):
        check(headers, bob)


@pytest.mark.parametrize(
    ("header", "value"),
    [
        ("X-Bittensor-Nonce", "not-a-number"),
        ("X-Bittensor-Crypto", "ecdsa"),
        ("X-Bittensor-Signature", "0xnothex"),
        ("X-Bittensor-Version", "2"),
    ],
)
def test_unparseable_header_rejected(
    alice_sr: Keypair, bob: Keypair, header: str, value: str
) -> None:
    headers = dict(make_headers(alice_sr, bob), **{header: value})
    with pytest.raises(http_auth.MalformedAuth):
        check(headers, bob)


# --- error hierarchy ---


def test_auth_errors_root_in_bittensor_error() -> None:
    for exc in (
        http_auth.MalformedAuth,
        http_auth.WrongReceiver,
        http_auth.StaleRequest,
        http_auth.ReplayedRequest,
        http_auth.BadSignature,
    ):
        assert issubclass(exc, http_auth.AuthError)
    assert issubclass(http_auth.AuthError, BittensorError)
