# bittensor-core: one Rust core under every client surface

Status: draft spec (not yet scheduled)

## 1. What we would have built on day one

If we had known from the start that the project would need (a) wallet crypto
that can never drift from the chain, (b) Ledger and other hardware/remote
signers, (c) timelock and MEV-shield cryptography, (d) fast SCALE
encode/decode, and (e) more client surfaces than Python (TypeScript, mobile),
we would not have built three separate native artifacts (`py-sp-core`,
`bittensor-drand`, cyscale) glued together in Python. We would have built:

- **`bittensor-core`** — one pure-Rust library crate in the monorepo
  workspace, pinned to the same `sp-core` / `frame-metadata` revisions as the
  runtime. No binding code inside. It owns everything that is *compute or
  cryptography with a chain-defined right answer*.
- **`bittensor-core-py`** — a thin PyO3/maturin binding crate (abi3, Python
  ≥ 3.10) exposing the core to the Python SDK. Later, sibling binding crates
  (`-uniffi` for Swift/Kotlin, `-napi` for TypeScript) expose the *same* core
  without forking logic.
- **The Python SDK and `btcli` as the semantic layer**: intents, reads,
  policy, codegen from metadata, CLI UX, wallet file/password/keychain glue,
  and the websocket transport. Python keeps everything whose right answer is
  a *product decision*; Rust keeps everything whose right answer is defined
  by the chain.

The existing seams were built for exactly this swap and do not move:

- `bittensor/_transport/codec.py` — the only module importing the SCALE codec.
- `bittensor/sp_core.py` — the only module importing `py_sp_core`.
- `bittensor/timelock.py` — the only module importing `bittensor_drand`.
- `bittensor/signing.py` protocols (`Signer`, `ExtensionPayloadSigner`,
  `MetadataVerifyingSigner`) — unchanged; new signers slot in structurally.
- The `Substrate` protocol in `_substrate.py` — unchanged; the transport
  stays Python.

## 2. Motivation (measured, not assumed)

Benchmarks against live finney (`sdk/python/scripts/bench_transport.py`,
`bench_neurons.py`, 2026-07-09) established that network round-trips dominate
every chain operation (CPU is 0–7% of wall time), so **speed is not the
primary motive**. The motives, in order:

1. **Capability: Ledger.** The transport already implements the
   `CheckMetadataHash` signing path and calls an optional
   `metadata_digest(SigningContext)` hook — but nothing implements the
   RFC-0078 merkleized-metadata digest. Parity's `merkleized-metadata` crate
   is the reference implementation; a Python reimplementation would be a
   large, security-sensitive project with no upside.
2. **One audited crypto core, one build.** Keys, keyfiles, timelock, ML-KEM,
   digest, and codec become one wheel with one CI pipeline (today:
   `build-sp-core-wheels.yml` + `build-drand-wheels.yml`), version-pinned as
   a unit against the runtime workspace.
3. **Multi-surface future.** A TS SDK or mobile signer reuses the identical
   core instead of reimplementing keyfile encryption or payload assembly.
4. **The two real CPU costs.** Startup codec build (337 ms of pure CPU per
   process — every `btcli` invocation) and `compose_call` (1.1 ms per inner
   call — ~1 s of CPU to build a 1,000-op batch). Both live in the codec
   seam, both are fixed as a side effect.

## 3. Non-goals

- **No Rust websocket/RPC transport.** Measured benefit ≤7%; cost is the
  fake-node test harness, cancellation/subscription semantics across FFI,
  and reconnect/nonce-cache behavior that is subtle and battle-tested in
  `rpc.py` / `interface.py`.
- **No Rust codegen or intent layer.** The generated-from-metadata Python
  (`_generated/`, codegen gates) and the intent/read registries are the
  SDK's product identity and stay hackable Python.
- **No behavior changes.** Same results, same error taxonomy, same golden
  signing vectors, byte-identical keyfiles.

## 4. Crate architecture

```
bittensor-core/               # pure Rust, no PyO3 anywhere
  src/
    keys/                     # absorbs py-sp-core: sr25519/ed25519 Keypair,
                              #   mnemonic/seed/uri/PolkadotJS-json import,
                              #   sign/verify (incl. <Bytes> wrapping),
                              #   sealed-box encrypt/decrypt, ss58
    keyfiles/                 # absorbs py-sp-core keyfile modules: NaCl,
                              #   Ansible Vault, legacy Fernet, detectors,
                              #   keypair <-> keyfile JSON codec
    timelock/                 # absorbs bittensor-drand: drand quicknet
                              #   encrypt/decrypt, epoch-schedule commit v2
    mlkem/                    # ML-KEM-768 + XChaCha20Poly1305 (MEV shield)
    runtime/                  # Runtime: parsed MetadataVersioned (V14/V15).
                              #   storage entries, constants, calls, errors,
                              #   runtime-API defs, type registry, IR export
    codec/                    # SCALE encode/decode against a Runtime:
                              #   by type id (native) and by type string
                              #   (compat resolver); bulk APIs; frozen
                              #   hand-written legacy decoders (StakeInfo etc.)
    extrinsic/                # signature payload (signed-extension table),
                              #   era, extrinsic assembly, multisig
                              #   MultiAccountId derivation
    digest/                   # RFC-0078 via merkleized-metadata
    signers/ledger.rs         # feature "ledger": ledger-transport-hid +
                              #   Substrate generic-app protocol
bittensor-core-py/            # PyO3 bindings only; abi3-py310; maturin
```

Rules:

- `bittensor-core` depends on workspace `sp-core`, `frame-metadata`, and the
  `scale-decode`/`scale-value` stack at the same revisions as the runtime —
  the "wallet crypto can never drift from the chain" property extends to
  metadata and codec.
- Feature gates: `ledger` (HID deps), `c-abi` (keeps the cbindgen C header
  that `bittensor-drand` ships today for non-Python consumers).
- The binding crate contains no logic: constructors, method forwarding,
  error mapping, and Python-object materialization only.

### 4.1 The central object: `Runtime`

Day-one design: the runtime is data. One object parsed from raw
`MetadataVersioned` bytes (the same blob `RuntimeManager` already caches on
disk) answers every metadata question:

```rust
let rt = Runtime::parse(metadata_bytes, spec_version, transaction_version, ss58_format)?;
rt.storage_entry("System", "Account")?;         // keys, hashers, value type, default
rt.compose_call("Balances", "transfer_keep_alive", params)?;  // -> CallBytes
rt.constant("Aura", "SlotDuration")?;
rt.module_error(idx, err_idx)?;
rt.runtime_api("SubnetInfoRuntimeApi", "get_metagraph")?;     // in/out type ids
rt.metadata_digest(genesis_hash)?;              // RFC-0078
rt.metadata_ir()?;                              // for codegen (unchanged shape)
```

Parsing targets ≤15 ms (vs. 337 ms today), which also lets the disk cache
keep storing raw bytes only — no pickled registry, no cache invalidation
subtleties.

### 4.2 FFI design rules

- **Bulk-first.** Every decode API accepts vectors and returns vectors
  (`decode_many(type_ids, blobs)`, `decode_map_pairs(entry, changes)`),
  materializing Python objects on the Rust side in one GIL acquisition. The
  win evaporates if we cross the boundary per storage entry.
- **Plain data across the boundary.** bytes/int/str/dict/list in and out —
  same contract `codec.py` documents today. The one opaque handle is the
  composed call (today a `GenericCall`; tomorrow `CallBytes`: raw bytes +
  the runtime's spec version), which the SDK already treats as opaque.
- **GIL released** during parse/decode/digest so decode overlaps RPC traffic
  on the event loop.
- **Sync core, async edges.** The core does no I/O except the feature-gated
  Ledger HID transport and drand's round fetch (both already device/HTTP
  bound). Asyncio stays a Python concern.
- **Errors**: one `CoreError` enum mapped to the existing Python exceptions
  (`KeyfileError`, `WrongPasswordError`, `StorageFunctionNotFound`,
  `ValueError` for encode/decode) so no caller changes.

### 4.3 Type identification: ids native, strings compat

cyscale's contract is type *strings* ("Vec<u8>", "Option<T>",
"scale_info::123", "Call", "Era"). The core resolves those for compatibility
(the `_name_maps` logic in `codec.py` moves into `runtime/`), but the native
currency is portable-registry type ids. Follow-up once stable: codegen emits
type ids directly into `_generated/` descriptors, skipping string resolution
on hot paths entirely.

The legacy pre-scale-info registry (only consumer: `LEGACY_RUNTIME_APIS` for
archive blocks, `_BITTENSOR_LEGACY_TYPES` = StakeInfo et al.) is small and
frozen. It gets hand-written Rust decoders with golden tests, not a generic
legacy-registry engine.

## 5. The Python surface afterward (file-by-file)

| Module | After |
| --- | --- |
| `_transport/codec.py` | Reimplemented over `bittensor_core`; same class/function signatures; cyscale import (and the `scalecodec` namespace-conflict guard in `_transport/__init__.py`) deleted |
| `_transport/storage.py` | Key hashing + `decode_map_pairs` delegate to core bulk APIs; Default/Option miss semantics stay here (documented behavior, not crypto) |
| `_transport/extrinsics.py` | Payload/assembly calls forward to `extrinsic/`; nonce cache, era normalization, outcome resolution stay |
| `_transport/runtime.py` | Unchanged except `RuntimeCodec` construction; disk cache format unchanged |
| `sp_core.py` | Re-exports from `bittensor_core` instead of `py_sp_core`; public API identical |
| `keyfiles.py`, `wallet.py`, `wallets.py` | Unchanged (OS glue; already call through `sp_core.py`) |
| `timelock.py` | Imports from `bittensor_core.timelock`; round math and UX stay Python |
| `signing.py` | Unchanged; gains `LedgerSigner` re-export |
| `extension/` | Unchanged (browser JS bridge) |
| `_generated/`, `intents/`, `reads/`, `cli/` | Unchanged |
| pyproject | `cyscale`, `py-sp-core`, `bittensor-drand` → single `bittensor-core` dependency; `xxhash` dropped (twox moves to core) |

## 6. New capability: Ledger

- `digest::metadata_digest(&Runtime, genesis_hash)` binds
  `merkleized-metadata`; exposed to Python so *any*
  `MetadataVerifyingSigner` can use it — the transport hook and mode-byte
  plumbing in `extrinsics.py` already work and do not change.
- `LedgerSigner` (feature `ledger`): device discovery over HID, sr25519/
  ed25519 per the app, implements the `Signer` protocol (async `sign`, 65-byte
  MultiSignature-prefixed signatures — already handled) *and*
  `metadata_digest`. CLI: `--ledger` on `btcli tx` commands resolves to it at
  the existing `resolve_signer` seam.
- Extension/QR flows unchanged; they already avoid in-process keys.

## 7. Batched transactions

No protocol change. The measured costs move:

- `compose_call`: 1.1 ms → target ≤50 µs; a 1,000-op `Batch` intent composes
  in ~50 ms instead of ~1.1 s.
- `Batch` encoding of inner calls happens core-side in one crossing
  (`compose_calls(Vec<...>)`).
- Signing/assembly already ~0.1 ms; submission remains network/chain-bound.
  We do not promise faster inclusion — only faster construction.

## 8. Packaging

- One PyPI package `bittensor-core` (abi3 wheels, same matrix the two
  existing wheel workflows cover; workflows merge into
  `build-core-wheels.yml`).
- Monorepo: `tool.uv.sources` path override, exactly like today.
- `py-sp-core` and `bittensor-drand` each publish a final shim release that
  depends on `bittensor-core` and re-exports its old module surface, then
  freeze. External `bittensor-drand` consumers (it has users beyond the SDK)
  keep working through the shim and the retained C ABI.

## 9. Performance targets (gate the codec phase on these)

Baselines from `bench_transport.py` on finney, 2026-07-09:

| Metric | Baseline | Target |
| --- | --- | --- |
| Codec/Runtime build from metadata bytes | 337 ms | ≤15 ms |
| SCALE decode throughput (metagraph/neurons payloads) | 3–6 MB/s | ≥50 MB/s |
| `compose_call` | 1.1 ms | ≤50 µs |
| Sign + assemble extrinsic | 0.1 ms | no regression |
| `query_map` decode (System.Account) | 92k entries/s | ≥500k entries/s |
| Warm `btcli` connect (ws + codec) | ~1.1 s | ~0.75 s (network floor) |

## 10. Testing and gates

- **Parity harness during migration**: property tests (hypothesis) feed the
  same type/value pairs to cyscale and core; byte-equal encodes, value-equal
  decodes. Runs in CI until cyscale is deleted.
- **Golden vectors**: existing signing-payload goldens, keyfile goldens
  (`test_keyfiles_golden.py`), and sp-core parity suites transfer as-is;
  add RFC-0078 digest vectors cross-checked against polkadot-js.
- **Codegen drift gates unchanged** — `metadata_ir()` must produce the same
  IR, which the `--drift` gate proves against a live node.
- **`fake_node.py` untouched** — the transport still speaks Python
  websockets.
- Rust-side: `cargo test -p bittensor-core` joins the workspace CI.

## 11. Delivery phases (each independently shippable)

1. **Consolidate** — create `bittensor-core` + `-py`; move `py-sp-core` and
   `bittensor-drand` sources in (mechanical; APIs frozen); shim releases;
   merge wheel workflows. No SDK behavior change.
2. **Runtime + codec** — `runtime/` + `codec/`; rewrite `codec.py` innards;
   parity harness green, perf targets met; delete cyscale dependency.
   (Fixes startup parse + batch compose.)
3. **Extrinsic + digest** — payload/assembly/multisig to core;
   `metadata_digest` exposed; golden vectors green.
4. **LedgerSigner** — feature-gated driver + `--ledger` CLI flag + docs
   (`docs/guides/` page alongside `extension-signing.mdx`).
5. **Later, separate decisions** — uniffi/napi binding crates; codegen
   emitting type ids; Rust `btcli` is explicitly *not* planned.

## 12. Reference propagation checklist

When phases land, update every place that names the old parts:

- `py-sp-core/README.md`, `bittensor-drand/README.md` (+ CHANGELOG) → point
  at `bittensor-core`, document the shim.
- `sdk/python/README.md` install/dev notes; `pyproject.toml` comments about
  py-sp-core/drand path sources; `uv.lock`.
- `docs/internals/repo-layout.mdx`, `docs/internals/rust-setup.mdx`,
  `docs/internals/sdk-tests.mdx`.
- `.github/workflows/build-sp-core-wheels.yml`, `build-drand-wheels.yml`,
  `publish-sdk-dev.yml`, `cargo-audit.yml`, `release-train.yml`.
- Workspace `Cargo.toml` members; `zepter.yaml` if feature propagation rules
  apply to the new crates.
- Docstrings that name cyscale: `_transport/__init__.py`, `codec.py` header.

## 13. Risks and open questions

- **cyscale removal risk**: cyscale has years of quirks baked into decoded
  value *shapes* (e.g. how enums, Options, and AccountIds render as plain
  values). The parity harness must cover shape, not just semantics — the
  reads layer pattern-matches on those shapes.
- **Weight v1/v2 probe, `ExtrinsicPayloadValue` field table**: today
  data-driven quirks in `codec.py`; must reproduce exactly (golden vectors
  pin them).
- **Contributor experience**: SDK contributors who touch only Python never
  need a Rust toolchain (wheels resolve from PyPI); contributors touching
  core need the workspace toolchain — same situation as today with
  py-sp-core, now with a bigger surface.
- **Ledger app reality check**: confirm the generic app's sr25519 support
  status for our derivation paths before promising coldkey flows; worst
  case ed25519-only for hardware, which the protocol already expresses via
  `crypto_type`.
- **Open**: does anything still consume `bittensor-drand`'s C ABI
  (`bindings.h`)? If nothing does, drop the `c-abi` feature instead of
  carrying it.
