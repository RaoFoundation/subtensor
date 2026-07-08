# Subtensor Contributor Guide

The full contributor guide lives in the rendered docs:
**[docs.bittensor.com/docs/internals/contributing](https://docs.bittensor.com/docs/internals/contributing)**
(source: [`docs/internals/contributing.mdx`](./docs/internals/contributing.mdx)).

The short version:

- Open PRs against `main`; draft until ready. Pallet/runtime changes must ship
  with tests — see [`docs/internals/testing.mdx`](./docs/internals/testing.mdx).
- CI sudo-upgrades a mainnet clone with your runtime and runs the clone
  regression tests, the Python SDK suites, and the docs build against it.
- Runtime-affecting PRs must bump `spec_version` above mainnet's (or carry the
  `no-spec-version-bump` label).
- Three positive reviews are required to merge.
- Merging to `main` starts the release train: one deterministic srtool build
  promoted devnet → testnet → mainnet, with the mainnet upgrade going through
  the deployment multisig and triumvirate approval.
