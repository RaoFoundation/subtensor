---
title: Falcon wallet protocol
description: Falcon-512 and Falcon-1024 account profiles, recovery, compatibility, and release requirements.
---

# Falcon wallets

Falcon wallets use the original Falcon padded signature format. They are not a
claim of conformance to a future final FN-DSA standard. The four immutable account
profiles extend the existing signed-v4 authorization envelope; existing SR, ED
and ML-DSA identities and encodings stay unchanged.

| Wallet | CLI | SDK crypto ID | Wire profile | Registration reserve |
| --- | --- | --- | --- | --- |
| Fixed Falcon-512 | `btcli wallet create --crypto-type falcon512` | 9 | 7 | None |
| Rotating Falcon-512 | `btcli wallet create --crypto-type falcon512 --type hashed` | 10 | 6 | 0.2 TAO |
| Fixed Falcon-1024 | `btcli wallet create --crypto-type falcon1024` | 11 | 9 | None |
| Rotating Falcon-1024 | `btcli wallet create --crypto-type falcon1024 --type hashed` | 12 | 8 | 0.2 TAO |

Normal transaction fees and operation-specific deposits still apply. Changing
scheme or account mode creates a different identity; it is not an upgrade of an
existing address. Restore with the original mnemonic, scheme and account mode.

## Addresses and authority

Each descriptor is `version (2) || profile || initial commitment (64 bytes)`.
Commitments use SHAKE256 with 64 output bytes and bind the profile and exact
public key. The permanent AccountId is BLAKE2b-256 of the Falcon account domain
and descriptor. Fixed profiles search a bounded, deterministic sequence of
transaction keys until the account cannot also encode an Ed25519 or Sr25519
public key. This preserves the existing MS protection against a classical
signature bypass of a post-quantum account.

Fixed wallets display their permanent SS58 address. Their proof supplies the
public key, proves its descriptor hashes to that address, and authenticates the
transaction. No registration or registration reserve is needed. Proof generation
is zero; the native transaction nonce still advances and is retained across
balance reaping to prevent replay.

Rotating wallets retain their permanent descriptor and account address while
updating the authorized key commitment and generation. They use the existing
registration/sponsorship flow, including the registration reserve, consent and
Shield behavior. Their `tao_` receiving descriptor and SS58 identify the same
account; only the descriptor contains the information needed to register it.
After restoration, the SDK reads finalized authority and derives the current
and next signing keys from the mnemonic. The existing admin enable switch also
applies to all four Falcon profiles.

## Wire and cryptographic contract

The outer extrinsic remains signed-v4 with `MultiAddress::Raw` and the existing
64-byte carrier marker. No `MultiSignature` variant is added. The actual Falcon
authorization is in the Raw address; the marker itself grants no authority.
Metadata-aware existing explorers can decode blocks. Wallet software must support
Falcon to create or sign its transactions.

| Parameter | Falcon-512 | Falcon-1024 |
| --- | --- | --- |
| Public key | 897 bytes | 1,793 bytes |
| Padded signature | 666 bytes | 1,280 bytes |
| Raw authorization | 1,669 bytes | 3,179 bytes |

Authorization bytes are `account[32] || version:u8 || profile:u8 ||
generation:u64-LE || public_key || next_commitment[64] || signature`. The key and
signature have the exact length selected by the profile, with no SCALE vector
prefixes inside Raw. Unknown profiles, malformed lengths and noncanonical
signature padding fail closed before or during verification.

Falcon signs a domain-separated SHAKE256-512 application transcript. It binds
the profile, purpose, public key, full authorization header and complete encoded
transaction payload, including call, signed extensions and implicit chain/era
data. The runtime streams the payload with a protocol size ceiling instead of
copying it or applying Substrate's 256-byte message-shortening convention.
Orders and messages use separate domains and independently derived capability
keys scoped to owner, chain, role and epoch.

The backend is pinned to `fn-dsa-{comm,kgen,sign,vrfy} = 0.3.0` in original-Falcon
mode with `no_avx2`. Only integer verification is needed by the production
runtime; signing/key generation are feature-gated. SDK keys use HKDF-SHA512 with
the Falcon derivation domain and `(version, profile, role, index)`; the backend
receives a domain-separated SHAKE256 seed stream. This exact seed-to-key behavior
is a wallet recovery contract. A dependency upgrade must preserve the pinned
vectors or use new profile identifiers. SDK signing obtains fresh OS randomness,
fails closed if it is unavailable, and verifies its output before returning it.

## Operations and release checks

Falcon reuses existing native calls, safe composite derivations, EVM association,
capability storage and registration calls. Mixed-key multisigs are supported;
their quantum resistance depends on the quorum and delegates. Separate ECDSA
Ethereum accounts do not become post-quantum. Existing hardware-wallet apps
need their own Falcon implementation before they can sign these profiles.

Reference measurements are required for Falcon authorization and detached-order
verification. Their new production weight entries remain fail-closed until those
measurements are supplied; test-only weights exercise the functional paths.
Smaller proofs alone do not establish fees or safe execution limits. Release
validation must cover native/Wasm verification, independent known-answer vectors,
recovery, malformed proofs, old-wallet compatibility, Shield sizing/nonces and
the full operation matrix. Backend integration tests are not an independent
cryptographic or side-channel audit.
