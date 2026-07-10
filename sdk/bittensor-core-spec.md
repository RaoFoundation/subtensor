# bittensor-core: one Rust core under every client surface

Status: **implemented through phase 4** (branch `bittensor-core-exploration`,
2026-07-10). Phases 0–4 are landed: the corpus and crate scaffold (phase 0),
py-sp-core + bittensor-drand consolidated into `bittensor-core` (phase 1),
RFC-0078 digest + `LedgerSigner` + `--ledger` (phase 2), the Rust
runtime/codec as the production `codec.py` seam with cyscale/xxhash/
`TYPE_REGISTRY` deleted (phase 3), and payload/assembly/multisig in core
with byte-equal golden vectors (phase 4 — pulled forward into phases 2–3).
§10 records the acceptance benchmark against the targets. Remaining: the
phase 5 "later, separate decisions" (uniffi/napi bindings, id-based
descriptor fast path), unscheduled by design.

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

**The no-regrets invariant**: the Python *public* API (`bittensor.*`) and
every protocol above are frozen for the duration. Any phase can be the last
phase and the product is still coherent; any phase can be reverted without
unwinding the ones before it.

The freeze deliberately does **not** cover the internal `_transport` seam:
`codec.py`'s cyscale-era signatures (ScaleType-accepting `encode`, the
`GenericCall` object, the re-encode workaround in
`encode_signed_extrinsic`) are workarounds for cyscale's object model, not
design. Emulating them in Rust would be migrating quirks. The seam interface
is redesigned to the core's model (`CallBytes`, bulk calls); its handful of
internal consumers (`storage.py`, `extrinsics.py`, `runtime.py`,
`runtime_api.py`, `interface.py`) are updated in the same phase.

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
- **No Rust `btcli`.** Explicitly out of scope; nothing in this plan should
  make it easier or harder.

## 4. Foundational decisions (data shapes first)

These are the decisions that are one-line diffs now and rewrites later. They
are made *before any logic moves*, and everything downstream follows from
them.

### 4.1 The decoded-value shape contract

The single largest hidden surface of this migration is not an API — it is
the *shape* of decoded values. cyscale renders SCALE data as plain Python
values with specific conventions (enums, Options, AccountIds as ss58
strings, Compact unwrapping, BoundedVec rendering, struct field naming), and
the entire reads/intents layer pattern-matches on those shapes.

Decision: the shape contract becomes an *artifact*, not folklore.

- A recorded **shape corpus**: for every type id in the current finney
  portable registry (and a sample of historical V14 registries), fixture
  files of `(type, SCALE bytes, cyscale-decoded value)` triples — synthetic
  values generated once via hypothesis, plus real chain data (metagraph,
  neurons, accounts, events) — recorded from cyscale and committed.
- The Rust codec is **defined as**: byte-equal on encode, shape-equal on
  decode, against this corpus. Not "semantically equivalent" — equal.
- Recording is the *only* time cyscale runs against the core. There is no
  live dual-run parity harness to build, maintain, and delete later: the
  corpus is the parity harness, checked into the repo as plain fixtures.
- The corpus outlives the migration: it becomes the permanent regression
  suite for any future codec change on any binding surface (a TS binding
  must produce the JSON-analog of the same shapes).

### 4.2 Type identity: names in descriptors, ids at runtime

Portable-registry type ids are **not stable across spec versions**, so they
must never be baked into `_generated/` descriptors or any persisted
artifact. The `Runtime` object owns per-spec-version name↔id maps (the
`_name_maps` logic in `codec.py` moves down); descriptors stay name-based;
ids are the core's native currency internally. Type strings remain the
compatibility API. This keeps archive-block decoding correct by construction
and leaves the door open to id-based fast paths without a data migration.

### 4.3 The composed call: `CallBytes`

Today the one opaque object crossing layers is cyscale's `GenericCall`.
Its replacement is a value, not an object: **raw SCALE call bytes + the spec
version they were composed against**. Everything currently derived from the
object (call hash for multisig, inner extrinsic for MEV shield, batch
embedding, decode-back for display) is a pure function of those bytes. This
makes the composed call serializable across processes and language surfaces
for free — which `call_from_data` in `codec.py` already half-acknowledges.

### 4.4 `Runtime`: one immutable object per spec version

All metadata questions are answered by one object parsed from the raw
`MetadataVersioned` bytes the disk cache already stores:

```rust
let rt = Runtime::parse(metadata_bytes, spec_version, transaction_version, ss58_format)?;
rt.storage_entry("System", "Account")?;      // keys, hashers, value type, default
rt.compose_call("Balances", "transfer_keep_alive", params)?;   // -> CallBytes
rt.constant("Aura", "SlotDuration")?;
rt.module_error(idx, err_idx)?;
rt.runtime_api("SubnetInfoRuntimeApi", "get_metagraph")?;      // in/out types
rt.metadata_digest(genesis_hash)?;           // RFC-0078
rt.metadata_ir()?;                           // for codegen (unchanged shape)
```

Concurrency corollary, answered up front: `Runtime` is **immutable and
Send + Sync**. Decoded values are materialized fresh per call; no mutable
state crosses the FFI boundary; the GIL is released during parse/decode/
digest. "What happens if another actor modifies this concurrently?" —
nothing can. The Python-side codec caches (`RuntimeManager`'s LRU, head-TTL)
keep working unchanged because what they hold is now cheap and thread-safe.

### 4.5 Error model

One `CoreError` enum in the core, mapped once in the binding crate to the
existing Python exceptions (`KeyfileError`, `WrongPasswordError`,
`StorageFunctionNotFound`, `ValueError` for encode/decode failures). No
caller above `codec.py`/`sp_core.py` changes an except clause.

### 4.6 FFI design rules

- **Bulk-first.** Every decode API accepts vectors and returns vectors
  (`decode_many`, `decode_map_pairs`, `compose_calls`), materializing Python
  objects in one GIL acquisition. Per-entry crossings are a design bug.
- **Plain data across the boundary** — bytes/int/str/dict/list, the same
  contract `codec.py` documents today; `CallBytes` is the one named value.
- **Sync core, async edges.** The core does no I/O except the feature-gated
  Ledger HID transport and drand's round fetch. Asyncio stays Python.

## 5. Crate architecture

```
sdk/bittensor-core/           # pure Rust, no PyO3 anywhere
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
    runtime/                  # Runtime (4.4): metadata parse, name<->id maps
    codec/                    # SCALE encode/decode against a Runtime; bulk
                              #   APIs. The "legacy" surface is deliberately
                              #   tiny: the Vec<u8> double-decode for old
                              #   runtime APIs + one hand-written StakeInfo
                              #   decoder (~50 lines), not a registry engine

    extrinsic/                # signature payload (signed-extension table),
                              #   era, assembly, multisig MultiAccountId
    digest/                   # RFC-0078 via merkleized-metadata
    signers/ledger.rs         # feature "ledger": hidapi HID transport +
                              #   Substrate generic-app protocol
sdk/bittensor-core-py/        # PyO3 bindings only; abi3-py310; maturin
```

Rules:

- `bittensor-core` depends on workspace `sp-core`, `frame-metadata`, and the
  `scale-decode`/`scale-value` stack at the same revisions as the runtime.
- Feature gates: `ledger` only. (The drand C ABI is removed, not carried —
  see §7.)
- The binding crate contains no logic: constructors, forwarding, error
  mapping, Python-object materialization. The pyo3 `extension-module`
  feature is enabled via maturin config exactly once (the pattern
  `bittensor-drand/pyproject.toml` already established, so
  `cargo test --workspace --all-features` keeps linking).

## 6. The Python surface afterward (file-by-file)

| Module | After |
| --- | --- |
| `_transport/codec.py` | Rewritten to the core's model (§1: `CallBytes`, bulk calls) — a much thinner file, not a cyscale emulation; the namespace-conflict guard in `_transport/__init__.py` deleted |
| `_transport/storage.py` | Key construction (prefix, hashers, param encoding) moves into `Runtime.storage_key(s)`; Python keeps only the Default/Option miss semantics and the `state_queryStorageAt` change plumbing |
| `_transport/extrinsics.py` | Payload/assembly calls forward to `extrinsic/`; nonce cache, era normalization, outcome resolution stay |
| `_transport/runtime.py` | Unchanged except `RuntimeCodec` construction; disk cache format unchanged (raw bytes stay the cache currency — `Runtime::parse` at ≤15 ms makes pickling registries permanently unnecessary) |
| `sp_core.py` | Re-exports from `bittensor_core`; public API identical, pre-migration aliases (`encode_ss58` etc.) live here, not in the core |
| `keyfiles.py`, `wallet.py`, `wallets.py` | Unchanged (OS glue; already call through `sp_core.py`) |
| `timelock.py` | Imports from `bittensor_core.timelock`; round math and UX stay Python |
| `signing.py` | Unchanged; gains `LedgerSigner` re-export |
| `extension/` | Unchanged (browser JS bridge) |
| `_generated/`, `intents/`, `reads/`, `cli/` | Unchanged |
| `settings.py` | `TYPE_REGISTRY` deleted (§7.1) |
| pyproject | `cyscale`, `py-sp-core`, `bittensor-drand`, `xxhash` → single `bittensor-core` dependency |

## 7. Subtraction first

Dead weight removed *before* the new foundations are laid, so it is never
migrated by inertia:

- **drand C ABI** (`ffi.rs`, `bindings.h`, `cbindgen.toml`): zero in-repo
  consumers (verified 2026-07-09). Deleted outright in the consolidation
  phase (decided 2026-07-09) — git history preserves it, re-adding a feature
  later is cheap, and a future non-Python consumer is better served by a
  uniffi/napi binding crate than a hand-rolled C header.
- **`get_encrypted_commit` v1**: the SDK is already on v2; v1 is not carried
  into the core.
- **Pre-migration alias names** (`encode_ss58`, `decode_ss58`,
  `verify_signature`): zero internal callers (verified); they stay as Python
  aliases in `sp_core.py` for external users and do not exist on the Rust
  surface.
- **Per-crate pyo3 version-matching comments/quirks**: two crates carefully
  pinning matching pyo3 versions becomes one crate with one pin.
- **Stray `scalecodec` imports above the seam**: `intents/coldkey.py` and
  `intents/multisig.py` import `scalecodec.utils.ss58` directly, violating
  the documented "codec.py is the only importer" rule. Repoint them at
  `sp_core.py` now — it shrinks the codec seam before anything replaces it.

### 7.1 Quirks that die instead of migrating

Each of these exists only because of cyscale's object model or string
registry. The Rust core must **not** reproduce them; the shape corpus proves
their absence is unobservable:

- The **`TYPE_REGISTRY` / extra-types overlay** (`{"Balance": "u64"}`). The
  V14/V15 portable registry fully describes Bittensor's types; the overlay
  feeds cyscale's string registry and the frozen legacy runtime-API path
  only. `Runtime::parse` takes no extra types; the legacy StakeInfo decoder
  hardcodes its own layout; `TYPE_REGISTRY` is deleted from `settings.py`
  and the transport constructors. (Verify in phase 0: no other overlay
  consumer exists — currently only `codec.py`, `interface.py` docstrings,
  and `runtime_api.py`'s legacy table reference it.)
- The **Weight v1/v2 probe** in `RuntimeCodec.__init__`: string-registry
  aliasing; the portable registry resolves `Weight` per runtime natively.
- **`_prime_runtime_configuration`** and its process-global singleton for
  multisig `MultiAccountId`: the core derives multisig accounts as a pure
  function.
- The **`scalecodec` namespace-conflict guard** in `_transport/__init__.py`:
  no cyscale, no namespace to defend.
- The **ScaleType-accepting `encode` / re-encode workaround** (§1): dies
  with `CallBytes`.
- **`xxhash`** Python dependency: twox hashing moves into the core's storage
  key construction.

## 8. Scaffold first

Everything here benefits every later phase, so it lands before any logic
moves (and doubles as the cheap-exit point if the direction is wrong):

1. **Crate skeleton**: `bittensor-core` + `bittensor-core-py` in the
   workspace (zepter picks them up automatically), wheel workflow
   `build-core-wheels.yml` (merging the sp-core and drand matrices),
   `cargo-audit` coverage, `tool.uv.sources` path override wired but the
   SDK not yet depending on it.
2. **Shape corpus** (§4.1): recorded from cyscale while cyscale is still the
   production codec — this must exist *before* the Rust codec is written,
   because it is the definition of done.
3. **Benchmark acceptance runs**: `bench_transport.py` (§10) run at each
   phase gate and results recorded in the phase's PR. Deliberately *not*
   permanent CI thresholds — machine-dependent perf CI is flaky scaffolding
   that would outlive its purpose.
4. **Golden vectors inventory**: signing-payload goldens, keyfile goldens
   (`test_keyfiles_golden.py`), sp-core parity suites — confirmed green and
   adopted as core acceptance tests; RFC-0078 digest vectors added,
   cross-checked against polkadot-js.

## 9. Full surface inventory

Every artifact the migration touches, so nothing is discovered mid-flight:

**Rust/workspace**: root `Cargo.toml` members; `Cargo.lock`; `zepter.yaml`
(feature propagation over the new crates); `py-sp-core/` and
`bittensor-drand/` (absorbed and deleted from the tree); `rust-toolchain.toml`
(no change expected, listed for completeness).

**CI/CD**: `build-sp-core-wheels.yml` + `build-drand-wheels.yml` → merged
`build-core-wheels.yml`; `publish-sdk-dev.yml` path triggers; `release-train.yml`
(the sed-stamping of two package versions into `sdk/python/pyproject.toml`
becomes one — lines ~300–380 today); `cargo-audit.yml`; SDK test workflows
(unchanged but re-verified).

**Python SDK**: `pyproject.toml` deps + uv sources + `uv.lock`; the modules
in §6; `tests/harness/fake_node.py` explicitly untouched; the
`websockets<17` cap unaffected.

**Packaging/compat**: PyPI `bittensor-core` (name verified available
2026-07-09; claim it with the first consolidation-phase release); `py-sp-core` and
`bittensor-drand` are **frozen, not shimmed** — PyPI is immutable, so every
existing pin keeps resolving forever, and the SDK (their only known
structural consumer; py-sp-core exists solely for it) moves its dependency
in the same release. A shim wheel would be speculative compatibility
machinery serving no observed user; if an external `bittensor-drand` user
surfaces (same announcement as the C ABI question, §7), a shim can be
published then — publishing later is cheap, carrying one is not.
`.gitignore` dist entries updated.

**Docs**: `docs/internals/repo-layout.mdx`, `rust-setup.mdx`,
`sdk-tests.mdx`; the final `bittensor-drand` PyPI page / CHANGELOG gets a
"continued as bittensor-core" notice; `sdk/python/README.md` dev notes;
docstrings naming cyscale (`_transport/__init__.py`, `codec.py` header);
later a Ledger guide in `docs/guides/` beside `extension-signing.mdx`.

**External consumers**: PyPI users of the two packages (existing pins keep
resolving forever; new development continues in `bittensor-core`); possible
C ABI users (§7, ask-then-delete); `btcli` users (no visible change except
startup time).

## 10. Performance targets (gate the codec phase on these)

Baselines from `bench_transport.py` on finney, 2026-07-09; acceptance run
2026-07-10 on the same machine/endpoint after phase 3 landed:

| Metric | Baseline | Target | Accepted |
| --- | --- | --- | --- |
| Codec/Runtime build from metadata bytes | 337 ms | ≤15 ms | **4 ms** |
| SCALE decode throughput (metagraph/neurons payloads) | 3–6 MB/s | ≥50 MB/s | **60–77 MB/s** |
| `compose_call` | 1.1 ms | ≤50 µs | **<50 µs** (below timer resolution) |
| Sign + assemble extrinsic | 0.1 ms | no regression | **0.03 ms** (29k/s) |
| `query_map` decode (System.Account) | 92k entries/s | ≥500k entries/s | **599k entries/s** |
| Warm `btcli` connect (ws + codec) | ~1.1 s | ~0.75 s (network floor) | **~0.6 s** |

What paid for the last two rows: decode hot paths cross the FFI once per
page, run the SCALE + ss58 work off the GIL (rayon-parallel above 64
entries), materialize Python objects behind a per-call repeated-object
cache, and ss58 rendering uses a limb-based base58 (~6x over the generic
byte-at-a-time algorithm, pinned byte-identical to sp-core's).

Batches inherit these: a 1,000-op `Batch` composes in ~50 ms instead of
~1.1 s, in one FFI crossing. Submission/inclusion remain chain-bound — we
promise faster *construction*, nothing else.

## 11. The signer ecosystem

The signer protocols in `signing.py` are the organizing principle: every
wallet interface is a `Signer` (plus optional capabilities), and the core
only enters where a signer needs *chain-defined computation* (crypto or the
RFC-0078 digest). Everything protocol- or UI-shaped stays where it is.

| Signer | Seam it uses | What this plan changes |
| --- | --- | --- |
| Native keyfiles (`WalletSigner`) | `Signer` | Crypto already Rust (`keys/`, `keyfiles/`); nothing visible |
| Browser extensions — polkadot-js, Talisman, SubWallet, PolkaGate | `ExtensionPayloadSigner` (`SignerPayloadJSON`) via the JS bridge in `extension/` | **Nothing.** Keys live in the extension; payload JSON construction stays in `extrinsics.py`; the bridge is JS by nature, not a Rust candidate |
| Air-gapped QR — Polkadot Vault (ex Parity Signer) | `prepare_call` → `UnsignedExtrinsic` (payload bytes + payload JSON) → `submit_signature`; no key material in-process | SDK side already complete. The gap is ecosystem-side: Vault must have Bittensor's chain spec/metadata loaded to render what it signs. The RFC-0078 digest (below) is the same mechanism Vault's shortened-metadata flow uses, so the `digest/` work serves Vault too |
| Ledger (generic app) | `Signer` + `MetadataVerifyingSigner` | **New**: `digest::metadata_digest(&Runtime, genesis_hash)` binds `merkleized-metadata`; `LedgerSigner` (feature `ledger`) does HID discovery and implements both protocols (async `sign`, 65-byte MultiSignature-prefixed signatures — already handled). CLI: `--ledger` resolves at the `resolve_signer` seam |
| Remote / HSM / service signers | `Signer` with async `sign` | Nothing; the transport already awaits coroutine signers |

Two properties worth stating because they are the future-proofing:

- `metadata_digest` is exposed to Python on `Runtime`, so *any* current or
  future `MetadataVerifyingSigner` — not just our `LedgerSigner` — gets
  RFC-0078 for free; the transport hook and mode-byte plumbing in
  `extrinsics.py` already work and do not change.
- The lingua franca for third-party wallets is the Polkadot-JS
  `SignerPayloadJSON`, which the SDK already emits. Any wallet that speaks
  it (present or future) plugs in with zero core involvement.

## 12. Delivery phases and decision gates

Each phase is independently shippable, reversible, and ends with an explicit
go/no-go. The Python API freeze (§1) is what makes every gate a real exit.

| Phase | Contents | Gate to proceed |
| --- | --- | --- |
| **0. Subtract + scaffold** | §7 removals (incl. repointing the stray scalecodec imports); §8 items 1–4 | Corpus recorded; wheel workflow produces installable wheels; *reassess direction here at near-zero sunk cost* |
| **1. Consolidate** | Move `py-sp-core` + `bittensor-drand` sources into the core (mechanical, APIs frozen); delete the C ABI; freeze the old PyPI packages and claim `bittensor-core`; release-train stamps one version | Existing golden suites green on the merged wheel; one release-train dry run |
| **2. Digest + Ledger** | `digest::metadata_digest` over raw `MetadataVersioned` bytes (standalone function now, method on `Runtime` once phase 3 lands — it does not depend on the codec work, which is why it comes early); `LedgerSigner` + `--ledger` + docs guide | Digest vectors match polkadot-js `merkleizeMetadata`; on-device verification against the generic app |
| **3. Runtime + codec** | §4.2–4.4; rewrite the `codec.py` seam to the core's model (§1) and update its internal consumers; delete cyscale + xxhash deps, `TYPE_REGISTRY`, and the §7.1 quirks | Shape corpus byte/shape-equal; §10 targets met (benchmark acceptance run recorded); full SDK test suite + e2e green |
| **4. Extrinsic assembly** | Payload/assembly/multisig to core | Golden signing vectors byte-equal |
| **5. Later, separate decisions** | uniffi/napi binding crates; id-based descriptor fast path | Not scheduled; the shape corpus and §4 decisions are what keep these cheap |

## 13. Testing model

- **Shape corpus** (§4.1) is the codec's definition of done and permanent
  regression suite; hypothesis contributes values at recording time only, so
  no dual-run harness exists to maintain or delete.
- **Golden vectors**: signing payloads, keyfiles, digest — adopted as core
  acceptance tests (§8.4).
- **Codegen drift gates unchanged**: `metadata_ir()` must produce the same
  IR, proven by the existing `--drift` gate against a live node.
- **`fake_node.py` untouched** — the transport still speaks Python
  websockets.
- Rust: `cargo test -p bittensor-core` in workspace CI; `cargo-audit` covers
  the new dependency tree (HID, merkleized-metadata).

## 14. Risks

- **Decoded-shape drift** is the migration's dominant risk; §4.1 converts it
  from a discovery problem into a recorded contract. Residual risk: shapes
  produced only by rare types absent from the corpus — mitigated by
  generating fixtures from the *whole* registry, not just hot types.
- **Data-driven quirks**: the Weight v1/v2 probe, `ExtrinsicPayloadValue`
  field table, era encoding — pinned by golden vectors before the code
  moves.
- **Contributor experience**: Python-only contributors never need a Rust
  toolchain (wheels resolve from PyPI); core contributors need the workspace
  toolchain — same as today with py-sp-core, larger surface.
- **Ledger readiness — largely resolved** (verified 2026-07-09): the
  generic app supports sr25519, and this runtime already declares
  `frame_metadata_hash_extension::CheckMetadataHash` in `TxExtension` with
  production builds compiled `--features metadata-hash` (`runtime/build.rs`
  enables it with `("TAO", 9)`; Dockerfile and justfile pass the feature).
  Remaining gate: on-device verification of our derivation paths.
- **External C ABI consumers**: ask before deleting (§7); the fallback is
  trivially re-adding a `c-abi` feature later, which is why deletion is the
  no-regrets default.
