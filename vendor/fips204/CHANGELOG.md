# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.6 (2024-12-21)

- Added support deterministic signatures via `_seed`
- Trivial typo on signature return error doc

## 0.4.5 (2024-11-08)

- Bug fix in Hash-ML-DSA - thank you @codespree
- Two new fuzzers with tons of new coverage: fuzz_sign and fuzz_verify

## 0.4.4 (2024-10-29)

- Significant shrink of required stack size
- Internal-only refactoring, clean-up and polishing

## 0.4.3 (2024-10-16)

- Adapted ExpandedPrivateKey into PrivateKey and ExpandedPublicKey into PublicKey, removed the former(s)
- Internal revision to align comments with released spec; added try_hash_sign (using OS rng)
- Revisit/revise supporting benchmarks, embedded target, dudect, fuzz and wasm functionality
- Fixed a bug in verify relating to non-empty contexts; asserts on all doctests

## 0.4.2 (2024-10-05)

- Fixed size of SHAKE128 digest in `hash_message()`
- Added sk.get_public_key()


## 0.4.1 (2024-09-30)

- Now exports the pre-hash function enum


## 0.4.0 (2024-09-29)

- Now aligned with **released** FIPS 204 including hash sig/verif and keygen with seed.


## 0.2.2 (2024-08-02)

- Bug fix to debug_assert in `power2round` and t_not_reduced in `keygen`; thank you @skilo-sh !!


## 0.2.1 (2024-06-19)

- Internal revision based on review 2 feedback
- API: try_verify() -> verify() change to prevent usage mistakes


## 0.2.0 (2024-05-25)

- Reworked for constant-time key generation and signature.
  This necessitated adapting the primary API (removing suffixes).
- Significant internal refinement and increased performance.


## 0.1.2 (2024-05-06)

- Significant internal refinement and increased performance.


## 0.1.1 (2024-03-08)

- Extensive internal refinement.
- Rework of expanded keys (in place of precomputes).
- Benchmarking, constant time checks, embedded sample, fuzz testing, wasm example.


## 0.1.0 (2024-01-01)

- Initial release
