"""One-shot smoke check for bittensor.http_auth (btauth/1).

Round-trips both schemes, exercises every rejection path, and prints the
golden vectors (payload string, ed25519 signature) for the docs page.
Run: source .venv/bin/activate && python scripts/btauth_smoke.py
"""

import time

from bittensor import http_auth
from bittensor.sp_core import CRYPTO_ED25519, CRYPTO_SR25519, Keypair

FIXED_NONCE = 1_752_076_800_000_000_000  # 2025-07-09T16:00:00Z in ns
BODY = b'{"prompt": "hello"}'

sender_sr = Keypair.create_from_uri("//Alice", CRYPTO_SR25519)
sender_ed = Keypair.create_from_uri("//Alice", CRYPTO_ED25519)
receiver = Keypair.create_from_uri("//Bob", CRYPTO_SR25519)


def expect(exc_type, fn):
    try:
        fn()
    except exc_type:
        return
    raise AssertionError(f"expected {exc_type.__name__}")


def make_headers(sender, **overrides):
    kwargs = dict(
        method="POST",
        path="/generate?stream=false",
        body=BODY,
        receiver_ss58=receiver.ss58_address,
        nonce_ns=FIXED_NONCE,
    )
    kwargs.update(overrides)
    return http_auth.sign(sender, **kwargs)


def check(headers, *, now_ns=FIXED_NONCE + 1_000_000, **overrides):
    kwargs = dict(
        method="POST",
        path="/generate?stream=false",
        self_hotkey_ss58=receiver.ss58_address,
        nonce_store=http_auth.InMemoryNonceStore(),
        now_ns=now_ns,
    )
    kwargs.update(overrides)
    return http_auth.verify(headers, BODY, **kwargs)


# --- round trips, both schemes, live and fixed nonces ---
for sender, crypto in ((sender_sr, CRYPTO_SR25519), (sender_ed, CRYPTO_ED25519)):
    caller = check(make_headers(sender))
    assert caller == http_auth.Caller(sender.ss58_address, FIXED_NONCE, crypto), caller
    live = http_auth.sign(
        sender, method="GET", path="/health", receiver_ss58=receiver.ss58_address
    )
    got = http_auth.verify(
        live,
        b"",
        method="GET",
        path="/health",
        self_hotkey_ss58=receiver.ss58_address,
        nonce_store=http_auth.InMemoryNonceStore(),
    )
    assert got.hotkey_ss58 == sender.ss58_address
print("round-trip: ok (sr25519 + ed25519, fixed + live nonce, GET empty body)")

# --- rejection paths ---
h = make_headers(sender_sr)
expect(http_auth.StaleRequest, lambda: check(h, now_ns=FIXED_NONCE + int(11e9)))
expect(http_auth.StaleRequest, lambda: check(h, now_ns=FIXED_NONCE - int(3e9)))

tampered = dict(h, **{"X-Bittensor-Nonce": str(FIXED_NONCE + 1)})
expect(http_auth.BadSignature, lambda: check(tampered, now_ns=FIXED_NONCE + 2))
expect(
    http_auth.BadSignature,
    lambda: http_auth.verify(
        h,
        b"tampered body",
        method="POST",
        path="/generate?stream=false",
        self_hotkey_ss58=receiver.ss58_address,
        nonce_store=http_auth.InMemoryNonceStore(),
        now_ns=FIXED_NONCE + 1,
    ),
)
expect(http_auth.BadSignature, lambda: check(h, method="DELETE"))
expect(http_auth.BadSignature, lambda: check(h, path="/admin"))

# scheme confusion: ed25519 signature presented as sr25519 (header stripped)
confused = dict(make_headers(sender_ed))
del confused["X-Bittensor-Crypto"]
expect(http_auth.BadSignature, lambda: check(confused))

expect(
    http_auth.WrongReceiver,
    lambda: check(h, self_hotkey_ss58=sender_sr.ss58_address),
)
unbound = make_headers(sender_sr, receiver_ss58=None)
expect(http_auth.WrongReceiver, lambda: check(unbound))
assert check(unbound, require_receiver=False).hotkey_ss58 == sender_sr.ss58_address

for missing in ("X-Bittensor-Version", "X-Bittensor-Hotkey", "X-Bittensor-Signature"):
    broken = {k: v for k, v in h.items() if k != missing}
    expect(http_auth.MalformedAuth, lambda b=broken: check(b))
expect(http_auth.MalformedAuth, lambda: check(dict(h, **{"X-Bittensor-Nonce": "abc"})))
expect(http_auth.MalformedAuth, lambda: check(dict(h, **{"X-Bittensor-Crypto": "ecdsa"})))

# case-insensitive header lookup (proxies often lowercase)
lowercased = {k.lower(): v for k, v in h.items()}
assert check(lowercased).hotkey_ss58 == sender_sr.ss58_address

# replay: same store twice
store = http_auth.InMemoryNonceStore()
check(h, nonce_store=store)
expect(http_auth.ReplayedRequest, lambda: check(h, nonce_store=store))
print("rejections: ok (stale, skew, tamper, scheme-confusion, receiver, malformed, replay)")

# --- performance sanity ---
start = time.perf_counter()
for _ in range(200):
    check(h)
per_call = (time.perf_counter() - start) / 200
print(f"verify latency: {per_call * 1e6:.0f} us/call")

# --- golden vectors for the docs ---
payload = http_auth.build_payload(
    scheme="sr25519",
    method="POST",
    path="/generate?stream=false",
    body=BODY,
    nonce_ns=FIXED_NONCE,
    sender_ss58=sender_sr.ss58_address,
    receiver_ss58=receiver.ss58_address,
)
print("\n--- golden vector data ---")
print("sr25519 //Alice:", sender_sr.ss58_address)
print("ed25519 //Alice:", sender_ed.ss58_address)
print("sr25519 //Bob (receiver):", receiver.ss58_address)
print("payload:")
print(payload.decode())
print("sha256(body):", __import__("hashlib").sha256(BODY).hexdigest())
ed_headers = make_headers(sender_ed)
print("ed25519 signature (deterministic):", ed_headers["X-Bittensor-Signature"])
ed_headers_2 = make_headers(sender_ed)
assert ed_headers_2["X-Bittensor-Signature"] == ed_headers["X-Bittensor-Signature"]
print("\nall checks passed")
