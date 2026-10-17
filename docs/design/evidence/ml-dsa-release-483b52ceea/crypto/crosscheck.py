"""Independent portable/AVX2 C ML-DSA against exact production Rust + SDK.
All secrets are public deterministic test inputs. No network or chain access.
"""

import ctypes as c
import hashlib as h
import hmac
import json
import struct
import time
from pathlib import Path
from bittensor.sp_core import (
    Keypair,
    verify_mldsa_authorization,
    verify_mldsa_capability,
)

P = Path(__file__).resolve().parent
lib = c.CDLL(str(P / "build/lib/liboqs.so"))
local = c.CDLL(str(P / "libprimitive_bridge.so"))
bp = c.c_void_p
sz = c.c_size_t
C_INT = c.c_int
for name, args in [
    ("local_keygen", [bp, bp]),
    ("local_sign", [bp, bp, sz, bp, sz, bp, bp]),
    ("local_verify", [bp, bp, sz, bp, sz, bp]),
]:
    f = getattr(local, name)
    f.argtypes = args
    f.restype = C_INT
backends = {}
for backend in ("C", "X86_64"):
    prefix = "PQCP_MLDSA_NATIVE_MLDSA65_" + backend + "_"
    funcs = {}
    for name, args in [
        ("keypair_internal", [bp, bp, bp]),
        ("signature_internal", [bp, c.POINTER(sz), bp, sz, bp, sz, bp, bp, C_INT]),
        ("verify", [bp, sz, bp, sz, bp, sz, bp]),
    ]:
        f = getattr(lib, prefix + name)
        f.argtypes = args
        f.restype = C_INT
        funcs[name] = f
    backends[backend] = funcs


def buf(n):
    return c.create_string_buffer(n)


def keygen(seed, backend):
    pk, sk = buf(1952), buf(4032)
    assert backends[backend]["keypair_internal"](pk, sk, seed) == 0
    return pk.raw, sk.raw


def sign(sk, msg, ctx, rnd, backend):
    sig, n = buf(3309), sz()
    pre = bytes([0, len(ctx)]) + ctx
    assert (
        backends[backend]["signature_internal"](
            sig, c.byref(n), msg, len(msg), pre, len(pre), rnd, sk, 0
        )
        == 0
    )
    assert n.value == 3309
    return sig.raw


def verify(pk, msg, ctx, sig, backend):
    return (
        backends[backend]["verify"](sig, len(sig), msg, len(msg), ctx, len(ctx), pk)
        == 0
    )


def local_sign(seed, msg, ctx, rnd):
    sig = buf(3309)
    assert local.local_sign(seed, msg, len(msg), ctx, len(ctx), rnd, sig) == 0
    return sig.raw


def local_verify(pk, msg, ctx, sig):
    assert len(pk) == 1952 and len(sig) == 3309
    return local.local_verify(pk, msg, len(msg), ctx, len(ctx), sig) == 0


def commit(pk):
    return h.shake_256(b"bittensor/ms/v2/key" + bytes([2, 4]) + pk).digest(64)


def derive(master, profile, role, index):
    prk = hmac.digest(b"bittensor/ms/v2/derive", master, "sha512")
    return hmac.digest(
        prk, bytes([2, profile, role]) + struct.pack("<Q", index) + b"\x01", "sha512"
    )[:32]


counts = {}


def count(k, n=1):
    counts[k] = counts.get(k, 0) + n


started = time.monotonic()
transcript_hash = h.sha256()
# Cross both independent C backends with the exact production Rust graph.
for i in range(128):
    seed = h.sha256(
        b"public-independent-mldsa-test-seed" + struct.pack("<Q", i)
    ).digest()
    msg = h.shake_256(seed).digest((0, 1, 255, 256, 257, 8192, 16384, 65536)[i % 8])
    ctx = h.shake_256(seed + b"context").digest((0, 1, 18, 255)[(i // 8) % 4])
    rnd = bytes(32) if i % 2 == 0 else h.sha256(seed + b"randomness").digest()
    pkbuf = buf(1952)
    assert local.local_keygen(seed, pkbuf) == 0
    pk = pkbuf.raw
    sig = local_sign(seed, msg, ctx, rnd)
    assert local_verify(pk, msg, ctx, sig)
    for backend in backends:
        opk, osk = keygen(seed, backend)
        assert opk == pk
        count("primitive_public_key_equal")
        osig = sign(osk, msg, ctx, rnd, backend)
        assert osig == sig
        count("primitive_signature_equal")
        assert verify(pk, msg, ctx, sig, backend)
        assert local_verify(opk, msg, ctx, osig)
        count("primitive_bidirectional_verification", 2)
        assert not verify(pk, msg + b"X", ctx, sig, backend)
        count("wrong_message_rejection")
    assert not local_verify(pk, msg + b"X", ctx, sig)
    count("wrong_message_rejection")
    wrongctx = (bytes([ctx[0] ^ 1]) + ctx[1:]) if ctx else b"X"
    assert wrongctx != ctx and not local_verify(pk, msg, wrongctx, sig)
    count("wrong_context_rejection")
    for backend in backends:
        assert not verify(pk, msg, wrongctx, sig, backend)
        count("wrong_context_rejection")
    transcript_hash.update(pk + sig)
    if i == 0:
        first = (pk, msg, ctx, sig)
# Every byte of one signature and public key, plus malformed hint encodings.
pk, msg, ctx, sig = first
for index in range(3309):
    changed = bytearray(sig)
    changed[index] ^= 1
    changed = bytes(changed)
    decisions = [local_verify(pk, msg, ctx, changed)] + [
        verify(pk, msg, ctx, changed, b) for b in backends
    ]
    assert decisions == [False] * 3, (index, decisions)
    count("signature_single_byte_mutation_cases")
for index in range(1952):
    changed = bytearray(pk)
    changed[index] ^= 1
    changed = bytes(changed)
    decisions = [local_verify(changed, msg, ctx, sig)] + [
        verify(changed, msg, ctx, sig, b) for b in backends
    ]
    assert decisions == [False] * 3, (index, decisions)
    count("public_key_single_byte_mutation_cases")
# Ensure cumulative hint boundaries, sorting and unused-zero padding are canonical.
for endpoint in range(6):
    for value in (56, 255):
        changed = bytearray(sig)
        changed[3303 + endpoint] = value
        changed = bytes(changed)
        assert not local_verify(pk, msg, ctx, changed)
        for b in backends:
            assert not verify(pk, msg, ctx, changed, b)
        count("malformed_hint_cases")
start = 0
for endpoint in sig[3303:3309]:
    if endpoint - start >= 2:
        for replacement in (sig[3248 + start], max(0, sig[3248 + start] - 1)):
            changed = bytearray(sig)
            changed[3248 + start + 1] = replacement
            changed = bytes(changed)
            assert not local_verify(pk, msg, ctx, changed)
            for b in backends:
                assert not verify(pk, msg, ctx, changed, b)
            count("malformed_hint_cases")
    start = endpoint
used = sig[3308]
for index in range(used, 55):
    changed = bytearray(sig)
    changed[3248 + index] = 1
    changed = bytes(changed)
    assert not local_verify(pk, msg, ctx, changed)
    for b in backends:
        assert not verify(pk, msg, ctx, changed, b)
    count("malformed_hint_cases")
# Protocol's exact context and payload ceilings; FIPS permits context <=255.
longctx = bytes(256)
assert (
    local.local_sign(bytes(32), b"", 0, longctx, len(longctx), bytes(32), buf(3309))
    != 0
)
assert not local_verify(pk, msg, longctx, sig)
count("protocol_boundary_cases", 2)
large = h.shake_256(b"public-max-payload").digest(16 * 1024 * 1024)
largepk, _ = keygen(bytes(32), "C")
largesig = local_sign(bytes(32), large, b"X" * 255, bytes(32))
assert local_verify(largepk, large, b"X" * 255, largesig)
for b in backends:
    assert verify(largepk, large, b"X" * 255, largesig, b)
assert (
    local.local_sign(
        bytes(32), large + b"X", len(large) + 1, b"", 0, bytes(32), buf(3309)
    )
    != 0
)
assert not local_verify(largepk, large + b"X", b"X" * 255, largesig)
count("protocol_boundary_cases", 3)
del large
# SDK HKDF/profile/identity/streamed pure transcript, generation and capabilities.
for code, profile in ((7, 4), (8, 5)):
    for i in range(8):
        master = h.sha256(b"public-wallet-master" + bytes([i])).digest()
        wallet = Keypair.create_from_seed(master, code)
        initialpk, _ = keygen(derive(master, profile, 0, 0), "C")
        descriptor = bytes([2, profile]) + commit(initialpk)
        account = h.blake2b(
            b"bittensor/ms/v2/account" + descriptor, digest_size=32
        ).digest()
        assert (
            bytes(wallet.mldsa_descriptor) == descriptor
            and bytes(wallet.public_key) == account
        )
        count("wallet_descriptor_and_identity_cases")
        for generation in (0, 1, 63, 64, 255, 256, 2**32, 2**64 - 2):
            active = wallet.at_generation(generation)
            seed = derive(master, profile, 0, generation if profile == 4 else 0)
            pk, sk = keygen(seed, "C")
            assert bytes(active.mldsa_public_key) == pk
            assert bytes(active.mldsa_current_commitment) == commit(pk)
            count("wallet_hkdf_generation_cases")
            payload = h.shake_256(master + struct.pack("<Q", generation)).digest(
                (0, 256, 257, 8192)[i % 4]
            )
            proof = bytes(active.sign_mldsa(payload))
            header = proof[:2058]
            ssig = proof[2058:]
            transcript = b"bittensor/ms/v2/transaction" + header + payload
            context = b"bittensor/ms/v2/tx"
            assert len(proof) == 5367
            for b in backends:
                assert verify(pk, transcript, context, ssig, b)
            independent_sig = sign(sk, transcript, context, bytes(32), "C")
            independent_proof = header + independent_sig
            assert verify_mldsa_authorization(
                independent_proof, payload, descriptor, generation, commit(pk)
            )
            assert not verify_mldsa_authorization(
                independent_proof, payload + b"X", descriptor, generation, commit(pk)
            )
            assert not verify_mldsa_authorization(
                independent_proof, payload, descriptor, generation + 1, commit(pk)
            )
            count("wallet_bidirectional_authorization_cases")
            if generation == 0:
                for pos in (0, 32, 33, 34, 42, 1994, 2057):
                    mutated = bytearray(independent_proof)
                    mutated[pos] ^= 1
                    assert not verify_mldsa_authorization(
                        bytes(mutated), payload, descriptor, generation, commit(pk)
                    )
                    count("authorization_header_tamper_cases")
            for role in (1, 2):
                capseed = derive(master, profile, role, generation)
                cpk, csk = keygen(capseed, "C")
                assert (
                    bytes(wallet.mldsa_capability_public_key(role, generation)) == cpk
                )
                assert bytes(
                    wallet.mldsa_capability_commitment(role, generation)
                ) == commit(cpk)
                count("wallet_capability_derivation_cases")
                genesis = bytes([9]) * 32
                if role == 2:
                    cp = bytes(
                        wallet.sign_mldsa_capability(role, generation, genesis, payload)
                    )
                    ch = cp[:2027]
                    cm = b"bittensor/ms/v2/capability" + ch + payload
                    cc = b"bittensor/ms/v2/message"
                    for b in backends:
                        assert verify(cpk, cm, cc, cp[2027:], b)
                    csig = sign(csk, cm, cc, bytes(32), "C")
                    proof = ch + csig
                    assert verify_mldsa_capability(
                        proof, payload, account, genesis, role, generation, commit(cpk)
                    )
                    assert not verify_mldsa_capability(
                        proof,
                        payload,
                        account,
                        bytes(32),
                        role,
                        generation,
                        commit(cpk),
                    )
                    count("capability_bidirectional_cases")
                else:
                    nonce = 81
                    sig = bytes(
                        wallet.sign_mldsa_order(generation, nonce, genesis, payload)
                    )
                    cc = b"bittensor/ms/v2/order"
                    cm = (
                        cc
                        + genesis
                        + account
                        + struct.pack("<QQ", generation, nonce)
                        + payload
                    )
                    for b in backends:
                        assert verify(cpk, cm, cc, sig, b)
                    assert not local_verify(cpk, cm, b"bittensor/ms/v2/message", sig)
                    count("order_transcript_cases")
results = {
    "status": "PASS",
    "seconds": round(time.monotonic() - started, 3),
    "case_counts": counts,
    "primitive_transcript_sha256": transcript_hash.hexdigest(),
    "liboqs_commit": "5a1a854b0dc9f2141bdc771c555ee60c37950183",
    "mldsa_native_upstream": "9b0ee84f4cf399043eca59eca4e5f8531ca1d61b",
    "backends": list(backends),
    "scope": (
        "Independent implementation interoperability, malformed inputs, "
        "protocol transcript and HKDF identity checks; "
        "not formal proof or side-channel certification."
    ),
}
(P / "crosscheck-results.json").write_text(json.dumps(results, indent=2) + "\n")
print(json.dumps(results, indent=2))
