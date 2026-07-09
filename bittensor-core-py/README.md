# bittensor-core

The chain-defined compute core for Bittensor clients, as a single Python
extension module (`import bittensor_core`). Built from the bittensor monorepo
against the same crate revisions as the subtensor runtime itself.

What lives here (one rule): everything whose right answer is defined by the
chain — sp-core key primitives (sr25519/ed25519, SS58), keyfile
encryption/decryption, drand timelock encryption, ML-KEM-768, the SCALE
codec and runtime-metadata engine, extrinsic assembly, and the RFC-0078
merkleized-metadata digest. Product decisions (intents, policy, CLI UX,
transports) stay in the Python SDK.

This package replaces and supersedes `py-sp-core` and `bittensor-drand`.

## Install

```
pip install bittensor-core
```

Wheels ship for manylinux (x86_64, aarch64) and macOS (arm64, x86_64); the
sdist builds anywhere with a Rust toolchain.

## Development

The Rust logic lives in the sibling `bittensor-core` crate; this crate is
bindings only. Build a development wheel from the repo root:

```
uvx maturin develop -m bittensor-core-py/Cargo.toml
```
