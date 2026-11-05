---
title: "Hashed-account cryptographic dependency provenance"
description: "Version, publisher, source, checksum, and release review for the hashed-account ML-DSA and HKDF dependencies."
---

The current implementation has removed ML-DSA and the `fips204` dependency.
The review below is retained as a historical record of the specified commit;
only the HKDF dependency remains relevant to hashed Sr25519/Ed25519 accounts.

This records a static dependency provenance review for PR #3220 at commit `684c05e8f4adad71dcdb5787bfb477b88097352c`, compared with main at `5c6e83e2f4936f257234f246957f75684c35227c`. Registry and repository metadata were checked on **2026-10-11 UTC**. Download counts and ownership are snapshots, not security guarantees. No dependency code, builds, or tests were executed during this review.

| Item | `fips204` | `hkdf` |
| --- | --- | --- |
| Selected version | `0.4.6` | `0.12.4` |
| Manifest requirement | Exact `=0.4.6` | Compatible `0.12.4`, meaning `>=0.12.4, <0.13.0` |
| Role in this branch | ML-DSA-65 verification in the shared runtime primitive; key generation and signing in the SDK | HKDF-SHA256 derivation of mnemonic-backed signing keys |
| Declared package author | Eric Schorn | RustCrypto Developers |
| Selected-version publisher | `integritychain` | `newpavlov` (Artyom Pavlov) |
| Current registry owners | `integritychain` | `newpavlov`, `github:rustcrypto:kdfs` |
| Selected-version publication, UTC | 2024-12-22 00:33:50 | 2023-12-13 13:35:31 |
| Latest stable registry release at review | `0.4.6`, published 2024-12-22 | `0.13.0`, published 2026-03-30 |
| Total crate downloads | 1,069,513 | 291,536,556 |
| Selected-version downloads | 1,003,559 | 207,840,571 |
| Selected version yanked | No | No |
| Declared license | MIT OR Apache-2.0 | MIT OR Apache-2.0 |

The publication, download, version, checksum, and publisher data come from the [fips204 registry metadata](https://crates.io/api/v1/crates/fips204) and [hkdf registry metadata](https://crates.io/api/v1/crates/hkdf). Ownership was checked separately through the [fips204 owners endpoint](https://crates.io/api/v1/crates/fips204/owners) and [hkdf owners endpoint](https://crates.io/api/v1/crates/hkdf/owners). Author and license declarations were read from the published package manifests and their corresponding [fips204 source manifest](https://github.com/integritychain/fips204/blob/8d456d989dce681233ef963d3c3306da8f76d7c7/Cargo.toml) and [hkdf source manifest](https://github.com/RustCrypto/KDFs/blob/1ac16e8b9d4ee7a67613c9396c6cc1327652eaba/hkdf/Cargo.toml).

The `Cargo.lock` SHA-256 checksums match both crates.io metadata and independently hashed copies of the cached published `.crate` archives:

```text
fips204 0.4.6
c9fb5a367b9846933e271a3c2a992930743f82ae5e8cb7faa780715a80fa0b15

hkdf 0.12.4
7b5f8eb2ad728638ea2c7d47a21db23b7b58a72ed6a38256b8a1849f15fbbdf7
```

The archived `.cargo_vcs_info.json` points to `8d456d989dce681233ef963d3c3306da8f76d7c7` for fips204, matching the upstream tag [`0.4.6`](https://github.com/integritychain/fips204/tree/0.4.6). For hkdf it identifies `1ac16e8b9d4ee7a67613c9396c6cc1327652eaba`, path `hkdf`, matching [`hkdf-v0.12.4`](https://github.com/RustCrypto/KDFs/tree/hkdf-v0.12.4/hkdf). These checks establish consistency between the selected registry artifacts, lockfile, declared source revision, and tags; they do not independently authenticate the maintainers or prove every packaged byte corresponds to a reproducible build.

**fips204 release review.** The selected [changelog](https://github.com/integritychain/fips204/blob/8d456d989dce681233ef963d3c3306da8f76d7c7/CHANGELOG.md) dates 0.4.6 to December 21, 2024, while crates.io records publication early December 22 UTC. It introduces seed-based deterministic signing helpers and documentation corrections. The [0.4.5-to-0.4.6 source comparison](https://github.com/integritychain/fips204/compare/v0.4.5...0.4.6) agrees: signing-trait helpers and tests were added, with small documentation, license-allowlist, and version changes. This branch uses seeded key generation and explicit RNG-based signing; the new deterministic signing helpers are not its signing path. The earlier changelog records Hash-ML-DSA, non-empty-context verification, and SHAKE128 fixes in 0.4.5, 0.4.3, and 0.4.2 respectively, all included in the selected release.

The primitive enables only `ml-dsa-65` and disables fips204's default features. Its newly added lock entry uses existing `rand_core 0.6.4`, `sha2 0.10.9`, `sha3 0.10.8`, and `zeroize` packages. The published crate contains no `build.rs`. Its [source](https://github.com/integritychain/fips204/blob/8d456d989dce681233ef963d3c3306da8f76d7c7/src/lib.rs) denies unsafe Rust; that is a source-level property, not a cryptographic correctness or side-channel guarantee.

**hkdf release review.** This branch adds a direct SDK dependency, but `hkdf 0.12.4`, its checksum, and its `hmac 0.12.1` dependency were already present in the base lockfile. The published [changelog](https://github.com/RustCrypto/KDFs/blob/1ac16e8b9d4ee7a67613c9396c6cc1327652eaba/hkdf/CHANGELOG.md) has no dedicated 0.12.4 entry, so the [0.12.3-to-0.12.4 source comparison](https://github.com/RustCrypto/KDFs/compare/hkdf-v0.12.3...hkdf-v0.12.4) was reviewed instead. Changes under `hkdf/` cover salt-usage documentation, generic `Debug` bounds, tests, a test-dependency rename, and version metadata; the extract/expand computation is unchanged in that comparison. No build script is packaged, and the crate forbids unsafe Rust.

The newer [0.13.0 changelog](https://github.com/RustCrypto/KDFs/blob/hkdf-v0.13.0/hkdf/CHANGELOG.md) describes an HMAC dependency update, type/API changes, Rust 2024 and MSRV 1.85, and removal of the `std` feature. It does not identify these as a security fix for 0.12.4. The branch's compatible 0.12 requirement does not silently select 0.13; the current lockfile selects exactly 0.12.4. This review does not request a dependency upgrade or lockfile regeneration.

The RustSec advisory database tree was checked at immutable revision [`7eebec69c352c7191b1f13eb95dd510eeca5d1de`](https://github.com/RustSec/advisory-db/tree/7eebec69c352c7191b1f13eb95dd510eeca5d1de). Its complete tree contained no `crates/fips204/` or `crates/hkdf/` entries. This is a limited advisory-source check for these two packages, not a clean bill of health for their dependencies or an exhaustive vulnerability search.

The selected fips204 [README](https://github.com/integritychain/fips204/blob/8d456d989dce681233ef963d3c3306da8f76d7c7/README.md) explicitly describes the software as experimental and limits its constant-time assurances to source-level work and statistical/manual checks. Its [security policy](https://github.com/integritychain/fips204/blob/8d456d989dce681233ef963d3c3306da8f76d7c7/SECURITY.md) supports only the latest release. The repository was not archived at review time; its reported last push was 2026-10-06. The package name does not establish FIPS validation of the implementation.

No version, checksum, repository, or source-tag mismatch was found. This record supplies the dependency provenance and release-review evidence requested by finding `fc572e90`; it is not a full cryptographic implementation audit or production-activation approval. External signature vectors, resource measurements, recovery behavior, and deployment gates remain separate requirements.
