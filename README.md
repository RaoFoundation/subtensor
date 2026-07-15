```commandline             _      _
███████╗██╗   ██╗██████╗ ████████╗███████╗███╗   ██╗███████╗ ██████╗ ██████╗
██╔════╝██║   ██║██╔══██╗╚══██╔══╝██╔════╝████╗  ██║██╔════╝██╔═══██╗██╔══██╗
███████╗██║   ██║██████╔╝   ██║   █████╗  ██╔██╗ ██║███████╗██║   ██║██████╔╝
╚════██║██║   ██║██╔══██╗   ██║   ██╔══╝  ██║╚██╗██║╚════██║██║   ██║██╔══██╗
███████║╚██████╔╝██████╔╝   ██║   ███████╗██║ ╚████║███████║╚██████╔╝██║  ██║
╚══════╝ ╚═════╝ ╚═════╝    ╚═╝   ╚══════╝╚═╝  ╚═══╝╚══════╝ ╚═════╝ ╚═╝  ╚═╝

```

# Subtensor

[![CodeQL](https://github.com/RaoFoundation/subtensor/actions/workflows/github-code-scanning/codeql/badge.svg)](https://github.com/RaoFoundation/subtensor/actions)
[![Discord Chat](https://img.shields.io/discord/308323056592486420.svg)](https://discord.gg/bittensor)

The Bittensor monorepo. It contains the **subtensor** chain (a Substrate-based
blockchain), the **Python SDK and `btcli`**, the **documentation**, and the
**website** that renders it — all in one place so a single change can be
validated end-to-end before it ships.

**Full documentation:** [bittensor.com/docs](https://bittensor.com/docs)

That site is the source of truth for using Bittensor: install, wallets,
staking, subnets, EVM, every transaction and query, error codes, and
contributor guides. The markdown lives in [`docs/`](./docs/) and is published
by [`website/`](./website/).

## What Bittensor is

Bittensor is an open network where independent **subnets** produce digital
commodities — compute, inference, storage, prediction — and the chain pays
participants in **TAO** in proportion to the value they contribute. **Miners**
produce, **validators** score, **subnet owners** define incentive mechanisms,
and **stakers** back validators.

The **subtensor** chain is the trust layer: it runs consensus, records subnet
and neuron state, handles staking and emissions, and moves value. Work itself
happens off-chain inside subnets; the chain coordinates and pays for it.
Subtensor also exposes a full **EVM** (Frontier) and **ink!/wasm** smart
contracts alongside native Substrate extrinsics.

## What's in this repo

| Area | Path | What it is |
|------|------|------------|
| Chain runtime | [`pallets/`](./pallets/), [`runtime/`](./runtime/) | FRAME pallets and the composed runtime. `pallets/subtensor/` is the core (staking, subnets, emissions, weights). |
| Node | [`node/`](./node/) | The `node-subtensor` binary: networking, RPC, chain specs. |
| EVM | [`precompiles/`](./precompiles/) | Precompiles that expose substrate functionality to EVM contracts. |
| Wasm contracts | [`chain-extensions/`](./chain-extensions/), [`ink-contract/`](./ink-contract/) | Chain extensions and ink! contract sources. |
| Shared Rust | [`common/`](./common/), [`primitives/`](./primitives/), [`support/`](./support/) | Shared types, math primitives, lints, macros, weight tooling. |
| Client core | [`sdk/bittensor-core/`](./sdk/bittensor-core/) | The Rust core for Bittensor clients (keypairs, keyfiles, SCALE codec, extrinsic assembly, RFC-0078 digest, drand timelock, ML-KEM) — built against the same crate revisions as the chain; Python bindings in [`sdk/bittensor-core-py/`](./sdk/bittensor-core-py/). |
| Python SDK | [`sdk/python/`](./sdk/python/) | The `bittensor` package and `btcli` CLI. Generated from chain metadata so reads, writes, and docs cannot drift. |
| Documentation | [`docs/`](./docs/) | Source for bittensor.com/docs. `docs/tx/`, `docs/query/`, and `docs/errors/` are generated — do not hand-edit. |
| Website | [`website/`](./website/) | Yarn + Turbo monorepo; `apps/bittensor-website` renders marketing pages and these docs. |
| Tests | [`ts-tests/`](./ts-tests/), [`clones/`](./clones/), [`eco-tests/`](./eco-tests/) | Moonwall/zombienet integration tests, mainnet-clone regression harness, indexer contract tests. |
| Operations | [`chainspecs/`](./chainspecs/), [`scripts/`](./scripts/), [`Dockerfile`](./Dockerfile) | Chain specs, CI/dev scripts, production and localnet Docker images. |

Deeper map: [Repository layout](https://bittensor.com/docs/internals/repo-layout).

## How the monorepo fits together

A runtime change can break the SDK, the docs, or live mainnet state. This
repo keeps those surfaces in sync:

1. **Metadata-driven SDK.** The Python layer (`bittensor.storage`, `runtime_api`,
   `calls`, `constants`) is emitted from a node's runtime metadata. Hand-written
   **reads** and **intents** sit on top; the CLI (`btcli query …`, `btcli tx …`)
   is generated from the same registries. CI proves every chain call is either
   wrapped by an intent or explicitly marked raw-only.

2. **Docs generated from the same registries.** Transaction, query, and error
   reference pages in `docs/` are built from the SDK catalogs, so
   bittensor.com/docs always matches what `btcli` and `import bittensor` expose.

3. **CI validates the whole stack.** On every PR, Rust checks run alongside a
   [mainnet-clone upgrade](https://bittensor.com/docs/internals/mainnet-clone)
   (your runtime applied to a copy of live state), Python SDK suites, and a docs
   build. Runtime migrations are replayed with try-runtime.

4. **Release train.** Merging to `main` builds the runtime once (deterministic
   srtool build) and promotes it devnet → testnet → mainnet. The SDK and docs
   publish after the chain upgrade lands. See
   [Release process](https://bittensor.com/docs/internals/release-process).

## Where to start

Pick the path that matches what you are doing — the linked docs have the detail.

| You want to… | Start here |
|--------------|------------|
| Install the SDK and run your first query/tx | [Quickstart](https://bittensor.com/docs/quickstart) |
| Use `btcli` or the Python API day-to-day | [CLI](https://bittensor.com/docs/cli) · [SDK README](./sdk/python/README.md) |
| Build an agent on top of the chain | [For agents](https://bittensor.com/docs/agents) · [`/catalog/intents.json`](https://bittensor.com/catalog/intents.json) |
| Understand subnets, TAO, epochs, wallets | [Concepts](https://bittensor.com/docs/concepts/network) |
| Run a local chain for development | [Local development](https://bittensor.com/docs/guides/local-development) |
| Operate a mainnet/testnet node | [Running a node](https://bittensor.com/docs/guides/running-a-node) |
| Develop or change the chain/runtime | [Rust setup](https://bittensor.com/docs/internals/rust-setup) · [Testing](https://bittensor.com/docs/internals/testing) |
| Contribute a PR | [Contributing](https://bittensor.com/docs/internals/contributing) · [CONTRIBUTING.md](./CONTRIBUTING.md) |

### Minimal local chain (Docker)

```bash
docker run --rm --name local_chain \
  -p 9944:9944 -p 9945:9945 \
  ghcr.io/raofoundation/subtensor-localnet:devnet
```

Use `:testnet` or `:main` to match those branches; `:latest` is an alias for
`:main`. Immutable `:sha-<commit>` tags are also published for reproducible CI.

Then connect with `btcli --network local query is-fast-blocks` or
`bittensor.Client("local")`. See
[local development](https://bittensor.com/docs/guides/local-development)
for building from source, the Alice dev account, and faucet behavior.

### Minimal chain build (from source)

```bash
./scripts/init.sh        # Rust toolchain + wasm target
./scripts/localnet.sh    # build and launch a fast-blocks localnet
```

The node binary is `node-subtensor` (`cargo build --release` →
`./target/release/node-subtensor`). Extrinsic and query reference:
[Transactions](https://bittensor.com/docs/tx) ·
[Queries](https://bittensor.com/docs/query).

### Minimal SDK dev (from this repo)

```bash
cd sdk/python
just sync    # uv environment; builds bittensor-core from ../bittensor-core-py (in sdk/)
just check   # lint, typecheck, unit tests, codegen gates (same as CI)
```

Chain-facing SDK coverage lives in the Rust core e2e suite:
`cargo test -p bittensor-core --test e2e -- --nocapture`. See
[SDK tests](https://bittensor.com/docs/internals/sdk-tests).

## Chain architecture (high level)

```
                    ┌─────────────────────────────────────┐
                    │           node-subtensor            │
                    │  (libp2p, RPC, tx pool, executor)   │
                    └─────────────────┬───────────────────┘
                                      │ executes
                    ┌─────────────────▼───────────────────┐
                    │     node-subtensor-runtime (wasm)     │
                    │  ┌─────────────┐  ┌────────────────┐  │
                    │  │ pallet-     │  │ Frontier EVM   │  │
                    │  │ subtensor   │  │ + precompiles  │  │
                    │  │ + swap,     │  │ + pallet-      │  │
                    │  │   shield,   │  │   contracts    │  │
                    │  │   proxy, …  │  └────────────────┘  │
                    │  └─────────────┘                       │
                    └───────────────────────────────────────┘
```

- **Runtime** (`runtime/src/lib.rs`): composes pallets, defines
  `spec_version`, hosts migrations and runtime APIs.
- **Core pallet** (`pallets/subtensor/`): subnets, registration, staking,
  weights, emissions, delegates, commitments.
- **EVM**: Substrate accounts (ss58) and EVM accounts (h160) are separate
  signing domains on the same chain; precompiles bridge staking and balances
  into Solidity. Guide: [EVM](https://bittensor.com/docs/guides/evm).
- **Networks**: `finney` (mainnet), `test` (testnet), `local` (dev node).
  Public endpoints and lite vs archive nodes:
  [The network](https://bittensor.com/docs/concepts/network).

## Contributing

Open PRs against `main`. Runtime-affecting changes need tests, a `spec_version`
bump (or the `no-spec-version-bump` label), and three positive reviews. Full
lifecycle and release train:
[Contributing](https://bittensor.com/docs/internals/contributing).

## License

See [LICENSE](./LICENSE). Individual crates may declare their own SPDX
identifier in `Cargo.toml` / `pyproject.toml`.

## Community

- [Discord](https://discord.gg/bittensor)
- [GitHub](https://github.com/RaoFoundation/subtensor)
