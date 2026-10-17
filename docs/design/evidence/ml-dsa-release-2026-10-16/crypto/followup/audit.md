---
title: "ML-DSA implementation audit and independent interoperability check"
description: "Completed source-scoped cryptographic audit with pinned mldsa-native cross-checks and current NIST errata review."
---

The additional local implementation audit is complete as of October 16, 2026.
**No new exploitable cryptographic authorization defect was established in the
reviewed source.** This conclusion is scoped to the files and artifacts recorded
in [source-hashes.json](source-hashes.json). It does not dismiss independently
reported runtime authorization or availability defects.

The baseline remains `2d5089da3bcb292cbf557d9b22ff34582e23b3f4`. The primitive,
vendored ML-DSA implementation, Rust wallet/keyfile code and Python native-key
bindings are unchanged from that commit. The prior keyfile publication fix and
other local changes are preserved. This pass made no production cryptographic
change, dependency/lockfile change, commit, push or chain launch.

## Findings and conclusion

There are **zero new confirmed critical, high or medium cryptographic findings**
from this pass. The previous wallet-file short-write defect was already repaired
and tested; that evidence remains in [the preceding review](../review.md).

The work completes a local source audit and adds an independently implemented
reference. It is not an external professional audit, FIPS 140 validation,
mathematical proof, or evidence that every compiler/CPU is immune to timing,
cache, power or fault attacks. The existing provenance statement that no external
independent security audit has been established must not be rewritten as a
certification claim. Whether a separately commissioned review remains an
organizational activation requirement is distinct from completing this local
audit.

## Independent implementation evidence

The original 4,838 checks primarily compared the patched implementation with its
own upstream Rust implementation and NIST fixtures. They remain useful but are
not counted again. This pass builds a different implementation:

- Official [liboqs 0.16.0](https://github.com/open-quantum-safe/liboqs/releases/tag/0.16.0),
  commit `5a1a854b0dc9f2141bdc771c555ee60c37950183`.
- Its [mldsa-native source](https://github.com/pq-code-package/mldsa-native/tree/9b0ee84f4cf399043eca59eca4e5f8531ca1d61b),
  commit `9b0ee84f4cf399043eca59eca4e5f8531ca1d61b`, using both portable C and
  x86-64 AVX2 implementations. These are two backends of one independent
  implementation, not two independent cryptographic designs.
- Only `SIG_ml_dsa_65` enabled, Release shared-library build, distribution CPU
  support enabled, OpenSSL disabled, two C compiler jobs. Source checkout is
  clean and build/library identities are retained.
- A test-only Rust FFI bridge links the exact `subtensor_hashed` library selected
  by the production release-node Cargo fingerprint. It does not reimplement the
  verifier, change production code, or invoke Cargo. The library SHA-256 is
  `b8d2c60a5a2aae6d4610516050f8b2efdc5c82a4ad48e264e59f28fcbda3e4ce`.
- SDK tests use the previously validated native extension, SHA-256
  `fdc0c3dbcb29d75a73956b6c6ff26b497ab7ca273f65d1640daa28f3aa7e7849`.
  The recorded relevant source identities match the prior audit.

All checks passed. The [machine-readable results](crosscheck-results.json)
distinguish test cases from individual comparison assertions:

| Coverage | Executed result |
| --- | --- |
| 128 distinct primitive seeds/messages/contexts | Public keys and signatures byte-identical with both C backends for identical randomness; verification works in both directions |
| Pure-mode message/context boundaries | Message sizes 0, 1, 255, 256, 257, 8,192, 16,384 and 65,536; context lengths 0, 1, 18 and 255; deterministic and nonzero hedged randomness |
| Signature mutations | Every one of 3,309 bytes mutated individually; all rejected by Rust, C and AVX2 |
| Public-key mutations | Every one of 1,952 bytes mutated individually; all rejected by Rust, C and AVX2 |
| Malformed hints | Out-of-range endpoints, duplicate/descending indices and nonzero unused padding rejected by all three verifiers |
| Protocol ceilings | Exact 16 MiB message with 255-byte context accepted across implementations; oversized message and 256-byte context rejected by the local API |
| Wallet identity | 16 fixed/rotating wallets derived independently with Python HKDF/HMAC, C key generation, SHAKE256 commitment and Blake2b-256 account hashing |
| Generation derivation | 128 wallet-generation cases at 0, 1, 63, 64, 255, 256, 2^32 and 2^64−2 |
| Transaction proofs | SDK signatures accepted by both C backends; C signatures accepted by the native SDK's actual authorization verifier; wrong payload/generation rejected |
| Signed headers | 112 mutations of account/version/profile/generation/public-key/next-commitment fields rejected |
| Capability derivation | 256 order/message key derivations agree with independent HKDF and C key generation |
| Message proofs | 128 bidirectional message-capability cases; independently signed C proofs accepted with trusted registry arguments and rejected with a different genesis |
| Order proofs | 128 exact runtime-order transcripts verified by both C backends; message-context substitution rejected |

The signing material is deliberately public test input. The new tests include
real randomness from the SDK for its normal signing path, but explicitly chosen
randomness for deterministic reference comparisons. They are correctness tests,
not an entropy-source certification. Failures encountered while constructing the
harness were test-fixture errors; the final results correspond to the retained,
formatted harness and successful exit.

## Static audit coverage

| Source | Reviewed property and result |
| --- | --- |
| `primitives/hashed/src/mldsa.rs:18-93` | Profiles 4 and 5 are immutable descriptor inputs; unsupported/zero descriptors fail. Fixed mode preserves its initial commitment; rotating mode requires a distinct nonzero next commitment. |
| `primitives/hashed/src/mldsa.rs:118-174` | Signed header includes AccountId, version, profile, generation, public key and next commitment. Streaming verification rebuilds pure-mode `mu` from the actual key, exact context and full canonical payload; no external digest is trusted and no Substrate 256-byte shortening is used. |
| `primitives/hashed/src/mldsa.rs:194-239` | Exact 5,367-byte authorization decoding rejects wrong length/profile/version. The 16 MiB counted SCALE adapter stops hashing and fails on overflow. |
| `primitives/hashed/src/mldsa.rs:249-304` | Commitments use 64 SHAKE256 bytes; the account descriptor is separately bound to a 32-byte Blake2 identifier. The fixed commitment domain deliberately retains algorithm 4 while profile is bound by descriptor, KDF and signed header. Cross-profile substitution does not preserve identity or key. |
| `vendor/fips204/src/conversion.rs:339-409` | Hint endpoints are nondecreasing and at most omega; indices are strictly increasing within each polynomial; unused padding must be zero. No off-by-one acceptance or signature malleability was found. |
| `vendor/fips204/src/ml_dsa.rs:435-517` | Response norm is strictly below gamma1-beta; challenge comparison uses all 48 bytes for ML-DSA-65. Both ordinary verification and locally reconstructed-mu verification share this arithmetic. |
| `vendor/fips204/src/hashing.rs:10-77` | Samplers reject before requesting byte 895 or 222; per-polynomial accounting and initial eight challenge sign bytes are included. Prior exact-bound tests remain applicable. |
| `vendor/fips204/src/ntt.rs`, `helpers.rs`, `high_low.rs` | NTT butterflies, Montgomery signed representatives, inverse normalization, decomposition and hint wraparound match the algorithm and independent implementation. No floating point, clock, network or random verification input is used. |
| `sdk/bittensor-core/src/keys/mldsa.rs:29-106` | HKDF-SHA512 binds version/profile/role/index with fixed-width unambiguous SCALE fields. Transaction, order and message keys are separate. Fixed transaction keys use index zero while replay generation still advances. OS RNG failure propagates; normal signing does not fall back to deterministic output on RNG failure. |
| `sdk/bittensor-core/src/keys/mldsa.rs:156-208,308-420` | Offline verification binds trusted descriptor/commitment/generation. Capabilities bind owner, genesis, role and epoch; order bytes include owner, genesis, epoch and nonce. Proof-carried authority is not treated as a grant. The independently reproduced transcripts agree. |
| `sdk/bittensor-core/src/keys/mod.rs:310-361,460-471` | MS mnemonic import requires 24 words; standard BIP39 seed derivation and explicit passphrase are used. The first 32 seed bytes feed the master KDF. Password-derived seed recovery does not incorrectly advertise the phrase alone as sufficient. Random generation uses the pinned BIP39/rand path. |
| `sdk/bittensor-core/src/keyfiles/mod.rs:304-399` | Recovery rederives and checks descriptor, mode, scheme, account and SS58 identity. Files retain master material rather than a stale active-generation private key. The prior encrypted recovery tests remain applicable. |
| `sdk/bittensor-core/src/keys/receiving.rs:97-141` | Exact receiving encoding/checksum and canonical descriptor validation preserve identity. Receiving address network independence does not remove transaction genesis binding. |
| `sdk/python/bittensor/mldsa_signing.py:147-222` | Offline requests are recomposed with trusted metadata/genesis and checked against the wallet's descriptor/current commitment before proof assembly. |
| `sdk/python/bittensor/mldsa.py:70-106,147-162` | Message authorization resolves a finalized grant, current epoch, role, expiry and exact audience; detached proof alone is insufficient. |
| `runtime/src/hashed_extrinsic.rs`, `runtime/src/hashed_auth.rs:186-269` | The legacy carrier signature cannot authenticate. Current-state/resource checks and normal extensions remain; registered protected accounts reject ordinary classical signatures. Validation tickets recheck state when consumed. |
| `pallets/hashed-accounts/src/mldsa.rs:409-486` | Generation advance uses checked addition and verifies current profile/commitment/generation again. Successful authorization consumes generation outside inner-call rollback; stale proofs cannot become current after an inner call fails. |

Generation exhaustion deliberately stops authorization at `u64::MAX`, and master
compromise reveals future derivations. Neither condition is hidden by the phrase
“rotating key.” A 32-byte account identifier supplies approximately a 128-bit
generic quantum targeted-preimage bound; the account system is not uniformly
NIST category 3. The existing specification documents these surrounding limits.

## Current NIST errata

The [official FIPS 204 page](https://csrc.nist.gov/pubs/fips/204/final) points to
the potential-updates spreadsheet dated July 31, 2026. Its exact source digest
and extracted cells are retained in [nist-errata.json](nist-errata.json).
The relevant corrections do not require a production algorithm change here:

- NTT prose incorrectly describes an extra polynomial evaluation. The code uses
  the correct roots and butterfly transform.
- The message representative must hash formatted `M'`, including mode/context,
  and the challenge input is `mu || w1Encode(w1)`. The implementation uses both
  in that order; independent pure-mode signatures agree.
- Montgomery reduction needs a signed 32-bit intermediate and the correct
  range. `helpers.rs:149-157` uses an `i32` wrapping product, sign extension and
  arithmetic shift; its result is bounded strictly between minus-q and q.
- ML-DSA-65 `UseHint` returns 0 through 15, not 16. The code's `& 15` wraparound
  implements this corrected bound.
- The minimum allowable bounded internal-signing loop rises from 814 to 821
  attempts. This implementation does not impose the old 814-attempt cap, and
  this correction does not change the separate 894/221 verification byte
  ceilings. The signing counter is 16-bit with step five, far beyond 821
  attempts; no practical overflow path was demonstrated.
- Remaining entries are notation, terminology or document-link corrections.

## Secret handling and side-channel limits

SDK master/derived seed holders use `Zeroizing`; vendor private keys and
polynomial buffers zeroize on drop. Secret key fields are not emitted in audit
artifacts or normal error strings. The retained keyfile fix handles partial
writes and atomic publication independently of cryptographic correctness.

Complete process-memory erasure is **not established**: upstream short-lived
byte arrays and SHAKE/HKDF internal state are not all explicitly zeroized, and
Python/caller-owned exported byte buffers may retain copies. These facts matter
to a memory-disclosure threat model but do not establish a remote key-recovery
exploit in the reviewed transaction path. No memory-erasure guarantee is made.

Verification handles public inputs and need not have secret-independent timing.
Signing uses rejection sampling; the number of attempts varies. The upstream
source explicitly isolates a modified `CTEST` path for statistical tests. Its
existence and upstream “constant-time” statements are not a production-binary
side-channel proof. The source pass checked fixed-index polynomial arithmetic,
secret-independent normal array indexing, and expected rejection decisions; it
did not run power/EM analysis, controlled microarchitectural attacks or a
formal compiler-level noninterference proof. No cryptographic key-recovery
finding was demonstrated from these paths.

## Reproduction

On an x86-64 host supporting AVX2, BMI2 and POPCNT, copy this directory to a fresh
temporary directory. Build the official reference there, leaving the application
workspace and its dependencies untouched:

```bash
git clone https://github.com/open-quantum-safe/liboqs.git liboqs
git -C liboqs checkout 5a1a854b0dc9f2141bdc771c555ee60c37950183
cmake -S liboqs -B build -DOQS_MINIMAL_BUILD=SIG_ml_dsa_65 \
  -DOQS_BUILD_ONLY_LIB=ON -DBUILD_SHARED_LIBS=ON -DOQS_DIST_BUILD=ON \
  -DOQS_USE_OPENSSL=OFF -DCMAKE_BUILD_TYPE=Release
cmake --build build -j2
python3 build_bridge.py --root /absolute/path/to/subtensor
PYTHONPATH=/path/to/matching/native/module:/absolute/path/to/subtensor/sdk/python \
  /absolute/path/to/subtensor/sdk/python/.venv/bin/python crosscheck.py
```

The bridge requires an existing release-node build and its Cargo fingerprint
graph; it performs no Cargo build. Adjust its explicit Rust compiler path if
reproducing on another machine. All inputs are deterministic public test values,
except ordinary SDK hedged signatures whose validity is checked rather than
compared byte-for-byte. `source-hashes.json`, `bridge-artifacts.json` and
`manifest.json` identify what was reviewed and executed. Private wallets, node
databases, compiled libraries and downloaded reference source are excluded from
the committed evidence candidate.
