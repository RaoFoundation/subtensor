---
title: "ML-DSA validation evidence"
description: "Reproduce client compatibility checks and mainnet-state upgrade validation for the ML-DSA candidate."
---

# ML-DSA validation evidence

The subsequent [localnet execution report](ml-dsa-localnet-report.md) records
real finalized transactions from all six wallet modes, cross-validator Shield
execution, and SDK fixes found by live CLI testing. It separates the unchanged
production weight gate from an isolated functional-test weight overlay. Its
compressed block capture can be replayed without running a chain:

```bash
gzip -dc docs/design/evidence/ml-dsa-localnet-blocks.json.gz > /tmp/ml-dsa-live-blocks.json
node docs/design/evidence/ml-dsa-localnet-codecs.cjs /tmp/ml-dsa-live-blocks.json /tmp/ml-dsa-live-codecs.json
sdk/python/.venv/bin/python docs/design/evidence/ml-dsa-localnet-native.py /tmp/ml-dsa-live-blocks.json /tmp/ml-dsa-live-native.json installed
```

The current result files were refreshed after adding the fixed-wallet profile (5 / SDK crypto 8), alongside the unchanged rotating profile (4 / SDK crypto 7). `ml-dsa-wallet-modes-results.json` records the six-command CLI matrix, immutable account modes, test totals and artifact/log hashes. Evidence applies only to the recorded artifacts and profiles; a prior Wasm hash cannot validate a later artifact.

`ml-dsa-wire-codec.cjs` tests the 5,367-byte Raw signer and canonical nonzero Ed25519 carrier with the repository's installed, unmodified Polkadot.js packages. The runtime exporter generates and checks a real ML-DSA proof. The JavaScript script checks decoding separately; it does not verify cryptographic authority.

Run from the repository root after installing the repository's pinned TypeScript dependencies. Generate a fresh metadata fixture from the intended runtime commit:

```bash
ACCOUNT_ENVELOPE_FIXTURES=/tmp/account-envelope-fixture.json cargo test -p node-subtensor-runtime export_client_envelope_fixtures -- --nocapture
node docs/design/evidence/ml-dsa-wire-codec.cjs /tmp/account-envelope-fixture.json /tmp/ml-dsa-wire-results.json
```

The exporter supplies metadata versions 14, 15, and 16 plus ordinary Sr25519, Ed25519 and ECDSA, v1 hashed Sr25519/Ed25519, a real ML-DSA transaction, and an unsigned extrinsic. The script checks exact bytes, hashes, fields and signer prefix, and inserts the ML-DSA transaction at every position in a mixed block. A separate synthetic control verifies that an all-zero carrier reproduces the legacy call-decoding failure. Record the runtime commit or working-tree provenance, fixture SHA-256, package version, dependency lockfile, and script revision with each result.

`ml-dsa-wire-codec-results.json` records the current experiment using `@polkadot/types` 16.5.6. Its fixture hash identifies the runtime export used; the large metadata fixture is not vendored here. These are local working-tree checks, not finalized mainnet evidence.

`ml-dsa-native-client.py` uses the selected Python interpreter's `bittensor_core` binding. It strictly decodes all seven runtime-produced extrinsics and checks each of the seven possible ML-DSA positions among the other six transactions. It verifies the original `System.remark`, the ML-DSA nonce, the full 5,367-byte Raw address, and the nonzero Ed25519 carrier. Select a different already-built extension with `PYTHONPATH`; the script does not install or build dependencies.

```bash
sdk/python/.venv/bin/python docs/design/evidence/ml-dsa-native-client.py /tmp/account-envelope-fixture.json /tmp/ml-dsa-native-results.json --label "selected installed binding"
```

The two preserved native results distinguish the installed legacy wheel from the implementation extension by binary SHA-256 and the presence of the new crypto-type export. Both report installed distribution metadata `bittensor-core` 0.1.8; that version alone does not identify the loaded binary. The legacy wheel has no `CRYPTO_MLDSA65` export and cannot create this key type, yet its generic decoder accepts the new envelope.

| Native result | Metadata 14 | Metadata 15 | Metadata 16 |
| --- | --- | --- | --- |
| `ml-dsa-native-installed-results.json` — installed legacy wheel | 7 original extrinsics, 7 mixed-body positions, and 32 signed SDK calls pass | Same | Unsupported metadata format |
| `ml-dsa-native-current-results.json` — implementation extension | 7 original extrinsics, 7 mixed-body positions, and 32 signed SDK calls pass | Same | Unsupported metadata format |

Both native bindings reject metadata 16 at runtime-codec construction with `unsupported metadata version V16`, before decoding any extrinsic. This is an existing metadata support limitation, not an ML-DSA envelope decoding failure. These results do not claim native metadata16 support.

`ml-dsa-papi-codec.cjs` uses the repository's installed, pinned PAPI dependency tree, including its existing repository patches. It exercises the actual runtime-context `getMortalityFromTx` decoder. Because that API exposes only mortality, a separate composition of PAPI's public metadata codecs also decodes each complete body, checks the original call and ML-DSA fields, and re-encodes exactly the input bytes.

```bash
node docs/design/evidence/ml-dsa-papi-codec.cjs /tmp/account-envelope-fixture.json /tmp/ml-dsa-papi-results.json
```

`ml-dsa-papi-codec-results.json` records **21 mortality decodes and 21 complete-body roundtrips**, seven extrinsics each under metadata 14, 15, and 16. The tested versions are `polkadot-api` **1.23.3**, `@polkadot-api/observable-client` **0.17.3**, `@polkadot-api/substrate-bindings` **0.17.0**, and `@polkadot-api/metadata-builders` **0.13.9**. The result includes the lockfile hash and the actual runtime-context source hash. These are decoder checks; they do not demonstrate wallet signing support or transaction submission.

The additional native and PAPI results record the exact current runtime fixture SHA-256 in each result file. Regenerating the runtime fixture can change signatures and therefore its hash. Each script records the supplied fixture hash and its own source hash for comparison with a new run.

`ml-dsa-node-admission.py` checks a running local release node. It first compares
the served `:code` with the supplied Wasm file, then sends 64 calls to a nonexistent
runtime export. At least one must receive the executor's local capacity error,
while the exempt `Core_version` read must still succeed. Run this against an idle,
temporary local node; it deliberately consumes the execution-admission burst.

```bash
python3 docs/design/evidence/ml-dsa-node-admission.py \
  --wasm target/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm \
  --output /tmp/ml-dsa-node-admission-results.json
```

The preserved result identifies the exact Wasm and probe script. This exercises
the real raw-RPC entry point and read availability; it does not activate ML-DSA,
prove production throughput, or demonstrate a finalized ML-DSA transaction.

The complete specification is in `../ml-dsa-accounts-spec.md`. Production release tests additionally need valid ML-DSA proofs, actual finalized blocks, metadata upgrades, and the broader client and operation matrix described there.

## Consolidated API coverage

`ml-dsa-sdk-codec.py` composes and ML-DSA-signs 32 registration, check,
order, and Shield transactions with candidate metadata: 16 cases under each
fixed/rotating ML-DSA profile. It checks all four registered wallet descriptors
(SDK types 4, 6, 7, 8), mixed order authorization, exact bounded-signature encoding, and
Shield payload sizes 1, 8,192, 8,193, and 32,768. Its result retains the development
key envelopes so other unmodified clients can decode the same calls. The runtime
metadata regression rejects fixed arrays exceeding Polkadot.js's 2,048-element
limit; the 3,309-byte embedded order signature uses a bounded vector and runtime
verification requires its exact length.

```bash
PYTHONPATH=/tmp/mldsa-native-python:sdk/python sdk/python/.venv/bin/python \
  docs/design/evidence/ml-dsa-sdk-codec.py /tmp/account-envelope-fixture.json \
  /tmp/ml-dsa-sdk-codec-results.json
node docs/design/evidence/ml-dsa-wire-codec.cjs /tmp/account-envelope-fixture.json \
  /tmp/ml-dsa-wire-results.json /tmp/ml-dsa-sdk-codec-results.json
node docs/design/evidence/ml-dsa-papi-codec.cjs /tmp/account-envelope-fixture.json \
  /tmp/ml-dsa-papi-results.json /tmp/ml-dsa-sdk-codec-results.json
```

Pass `--sdk-cases /tmp/ml-dsa-sdk-codec-results.json` to
`ml-dsa-native-client.py` to check those same 32 full signed transactions with
either native binding, in addition to the original seven fixtures.

Both JavaScript clients decode and exactly re-encode all 32 consolidated-call
transactions under metadata v14/v15/v16, in addition to their original mixed-block
checks. Polkadot.js also decodes a complete 39-extrinsic block interleaving all
32 consolidated transactions with the seven ordinary/hashed/unsigned fixtures. This catches call-field decoding failures that a System.remark-only
fixture cannot expose. The tested client versions decode these constructed block
bodies; this does not demonstrate finalized ML-DSA blocks, application UI behavior,
or signing support without a compatible signer.

`ml-dsa-api-compatibility.py` takes a finalized mainnet snapshot JSON with
`block`, `version` (the `state_getRuntimeVersion` result), and `metadata` (the
`state_getMetadata` hex result), all read at the same finalized block hash.
It compares 29 deployed legacy call encodings per candidate metadata version
(v14/v15): V1/V2 order payloads, every classical signature variant, both partial
fill forms, cancellation, and old Shield payload sizes. Calls must have identical
bytes, indices, argument values, and hashes. The intentional Shield argument
bound/type-label change is ignored only for the decoded type label.
It also checks 16 new authorization-call decoding cases and absence of the
removed duplicate APIs using the installed legacy native binding.

```bash
sdk/python/.venv/bin/python docs/design/evidence/ml-dsa-api-compatibility.py \
  /tmp/mainnet-metadata-snapshot.json /tmp/account-envelope-fixture.json \
  /tmp/ml-dsa-api-compatibility-results.json
```

The saved comparison identifies finalized mainnet block
`0x51acfdbaeff6cfea99318e35840dde8a7f995169df4d0f727f5023c7229ded2f`,
spec 479, and the baseline metadata hash. HashedAccounts was absent at that block;
LimitOrders and MevShield were present. New proof cases in this comparison use
dummy signatures and establish encoding only; the signed SDK envelopes and
runtime execution regressions provide separate evidence.

## Mainnet-state upgrade and post-upgrade execution

`ml-dsa-mainnet-upgrade-results.json` records a successful local replay from
finalized mainnet block **9,290,353**, hash
`0xc6f4878f6a5ea1c102f4f5db383b076811cfb8815287d5bafef8bb3bbcbe6706`.
The freshly acquired snapshot contained 3,298,185 top-level keys and 56 child
tries. Its loaded storage root matched the finalized header exactly. The
production-profile `try-runtime` Wasm hash is
`f14681d75696dbed9c0e61d86a0ea182fc4162dd0b2fc8e6eba6e5570b5931d7`.

The upgrade from spec 479 to 480 passed all upgrade checks, spec-name/version
checks, and migration idempotency. Ten locally constructed post-upgrade blocks,
9,290,354 through 9,290,363, then executed successfully with signature and
state-root checks, `try-state All`, and complete storage decoding on every block.
These are simulated blocks over mainnet state, not finalized mainnet transactions
or evidence of ML-DSA activation. No remote chain state was written. Reference
weight measurement/warnings were excluded as requested; multi-block migration
checks were disabled because this runtime's `MultiBlockMigrator` is `()`.

The portable runner pins and verifies try-runtime CLI 0.10.1. Build its input with:

```bash
env -u SKIP_WASM_BUILD cargo build --locked --offline --profile production \
  -p node-subtensor-runtime --features try-runtime
```

Acquire the snapshot using the command and finalized block recorded in
`ml-dsa-mainnet-source.json`; retain the acquisition log with
`RUST_LOG=info,remote-ext=debug,runtime=debug`. Then run:

```bash
python3 docs/design/evidence/ml-dsa-mainnet-upgrade.py \
  --try-runtime /path/to/try-runtime \
  --runtime target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm \
  --snapshot /path/to/mainnet.snap \
  --source docs/design/evidence/ml-dsa-mainnet-source.json \
  --output-dir /tmp/mldsa-mainnet-validation
python3 docs/design/evidence/ml-dsa-mainnet-log-check.py \
  --result /tmp/mldsa-mainnet-validation/ml-dsa-mainnet-upgrade-results.json \
  --runner docs/design/evidence/ml-dsa-mainnet-upgrade.py \
  --source docs/design/evidence/ml-dsa-mainnet-source.json \
  --log-dir /tmp/mldsa-mainnet-validation \
  --snapshot-log /path/to/create-snapshot.log \
  --output /tmp/mldsa-mainnet-validation/verified-results.json
```

The saved `ml-dsa-mainnet-upgrade-raw-results.json` preserves the untouched runner
result. The separate log checker verifies its source/script/log hashes, the exact
contiguous ten-block sequence, and every block's check/completion markers before
writing the enriched result. Missing completion, disabled signature checking,
disabled try-state, and missing full-state decoding were rejected in negative
controls. Current replay logs remain in `/tmp/mldsa-mode-mainnet`; the snapshot
and acquisition log remain in `/tmp/mldsa-deploy-validation`. Their hashes are
recorded rather than committing the 925 MB snapshot.

`ml-dsa-local-validation-results.json` records the full Rust/Python suite totals,
both Clippy configurations, build artifacts, commands, and original log hashes.
The repository preflight remains unsuccessful solely at its configured push-actor
check (`UnArbosSix`, expected `unarbos`). Its heavy checks were run independently;
that does not turn the complete preflight result into a pass.

## Native and release-Wasm verifier comparison

`ml-dsa-release-verifier-differential.py` compiles the adjacent Rust harness
against the release node's cached dependency graph. It changes no Cargo files,
runtime APIs, Wasm bytes, or production weights. The harness requires the native
runtime's embedded `WASM_BINARY` to equal the supplied release Wasm byte-for-byte,
and records dependency and source hashes.

```bash
python3 docs/design/evidence/ml-dsa-release-verifier-differential.py \
  --wasm target/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm \
  --output /tmp/ml-dsa-release-verifier-differential-results.json
```

All twelve cases passed on the recorded release Wasm. Deterministic and randomized
valid order proofs each debit exactly one TAO, credit one alpha, fulfill the order,
and emit `OrderExecuted`. Ten malformed proofs return `InvalidSignature` while
preserving the owner's assets and order state: empty/short/zero signatures, wrong
context/message, modified challenge/response, invalid hint count, duplicate hint,
and nonzero hint padding. Native and Wasm API return bytes, event bytes, and entire
post-execution storage roots agree for every case.

The isolated harness uses the existing `Sudo.sudo_unchecked_weight` and
`Utility.dispatch_as` APIs to reach the original `LimitOrders.execute_orders`
verifier despite deliberately uncalibrated dispatch admission. Its explicit test
state and unchecked dispatch are not activation or throughput evidence. This
compares the shared pure-mode verifier; direct account transactions still stop at
their compiled `Weight::MAX` gate before transaction-specific streaming transcript
verification. That remaining Wasm path and finalized ML-DSA workflows require an
artifact with approved admission weights. Internal comparisons also do not
substitute for an independent cryptographic audit.

## Operation coverage inventory

`ml-dsa-operation-inventory.py` reads the actual release metadata and reuses
`codegen.check`'s covered-pallet/raw-only lists and the SDK intent registry. Its
result records all 275 calls in that existing SDK scope, source-backed origin
guards, embedded authorization paths, wrapper availability, and precise regression
references. Other metadata calls are listed explicitly outside that scope.

```bash
PYTHONPATH=/tmp/mldsa-native-python:sdk/python sdk/python/.venv/bin/python \
  docs/design/evidence/ml-dsa-operation-inventory.py \
  /tmp/account-envelope-fixture.json /tmp/ml-dsa-operation-inventory-results.json
```

The saved result contains 123 wrapped calls and 152 intentionally raw-only calls.
Twenty-five calls map to 22 explicit regression references with limited scopes;
158 have generic origin-path compatibility reasoning, and 92 have existing
restricted/special origins or no targeted mapping. This is a reproducible static
inventory, not execution of every operation through every proxy/multisig shape.
Every row separately marks activated production end-to-end execution as untested.
It must not be used to claim complete operation parity or hardware/UI validation.
