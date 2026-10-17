---
title: "ML-DSA localnet execution validation"
description: "Finalized local execution, SDK regressions, and legacy decoding evidence for the six wallet modes."
---

# Results

All **26 end-to-end scenarios passed** after the four SDK fixes below.
The complete [machine-readable result](ml-dsa-localnet-results.json) records
case names, artifact identities, source hashes, and the original logs' hashes.

| Suite | Passed | Live coverage |
| --- | --- | --- |
| CLI wallet parity | 6 | Exact six creation commands; both key roles; 36 sender/recipient combinations through named and descriptor transfers; mnemonic/private-key/seed recovery; watch-only import; messages; multisig; proxies; subnet ownership; password failures. |
| Wallet lifecycle | 6 | Zero-funded PoW registration; native and EVM transfers; staking/unstaking; EVM association; recovery; HTTP authentication; replay rejection; delegation floors; native EVM aliases; announced coldkey swaps preserving ownership and stake. |
| ML-DSA authority and composition | 6 | Both profiles' offline signing, tamper/replay rejection, fixed/rotating commitment behavior, binary audience separation, revocation, EVM call/create/create2, protected derivatives and pure proxies; mixed two-of-two multisigs; all six ordered signing pairs of a protected two-of-three; runtime rejection of a wholly classical quorum. |
| Original limit-order calls | 2 | Both profiles' account approval and detached proofs; execution, replay, cancellation, revocation, atomic rollback, batched execution, and on-chain exhaustion of a shared budget by already-signed orders. |
| Shield across three validators | 6 | Production slot timing; successful and failed encrypted calls; correct nonce/generation advancement; subsequent signing from a stale client. |

Additional checks passed: **2,872 Python unit tests** (one skipped), **113 native
SDK Rust tests** (one ignored), both SDK Clippy configurations, formatting,
generated documentation, and live SDK metadata drift checks. After the final
composition-version guard and native rebuild, **208 focused SDK tests** passed.
The result file distinguishes the native binary used for the complete unit run
from the final rebuild; the native difference was header documentation only.

Early failures are retained in the local logs. The extended suite's six
non-order cases passed before its order setup encountered a higher subnet lock
cost. Funding now uses the live quote, and both order cases passed a targeted
rerun. A separate initial overlap with the six-wallet block capture exercised
the node's bounded admission rejection; serialized functional runs passed.

All temporary validators were stopped after saving final health. Both active
topologies finished with three healthy validators, each connected to its two
peers. No commit, push, production activation, or deployment was performed.

# Scope and runtime identity

This run exercises the uncommitted `codex/ml-dsa-accounts-v2` branch based on
`f867191868d29c5b28d3afdf13f967a12f0af128`, using the repository's
`scripts/localnet.sh` three-validator topology. Disposable copies of the launcher
change only paths, ports, the binary location, and the existing development Wasm
override. The tests use temporary wallets and development sponsors.

The exact production Wasm was first started and checked against served `:code`:
`3930cb27c6910a0d36d21759fc9e618301847291604715d112600b7d25dcabcb`.
Its activation flags default to false. Even after enabling the flags on this
disposable chain, fixed and rotating ML-DSA registration correctly failed with
`ExhaustsResources`: the production weights still fail closed.

Functional execution therefore uses an isolated source copy replacing only the
new `Weight::MAX` placeholders with the existing runtime test constant
`Weight::from_parts(10_000_000, 0)`. The original production weight files and
production Wasm remain unchanged. These constants are **test instrumentation**;
they are neither measurements nor suitable production weights. The machine-readable
report records the exact modified files and both source and test hashes.

Two independent three-validator chains use the fast runtime for ordinary
operations and production 12-second slots for Shield's timed announce/reveal
protocol. Activation and any permissive subnet settings apply only to these
disposable chains. Ordinary EVM contract deployment allowlists still apply;
test accounts receive explicit local grants.

# Live regressions corrected

1. The native metadata codec returns the supported profile byte array as hex.
   Fixed ML-DSA checks now accept the actual wire representation and reject
   malformed or absent advertisements before using a private key.
2. Ordinary CLI submissions from registered accounts now wait for finalization,
   so the next command can read the next authoritative generation. This covers
   rotating sr/ed/ms and the replay counter of fixed ms.
3. Capability audience decoding now preserves arbitrary bytes. UTF-8 text
   `0xff` remains distinct from the binary byte `ff`; unrelated legacy byte-vector
   display behavior is unchanged.
4. Composed multisig protection calls can be passed directly to the approval API
   without rebuilding their bytes. Calls composed against another runtime
   version are rejected for review and recomposition.

# Finalized block decoding

Block `0x5f42b5d0754377fa35c369eee1c4d48fd7f0eabfde00722326e9d3c3d86daa25`
contains all six wallet modes in one finalized block, with nine total extrinsics.
Unmodified Polkadot.js and unchanged PAPI codecs decoded and round-tripped every extrinsic with
metadata v14, v15, and v16. Signed status, transaction hashes, call bytes, block
header identity, and the 5,367-byte ML-DSA Raw carrier were checked.

Both current and installed legacy native SDK bindings decoded the block with
metadata v14 and v15. Their existing rejection of metadata v16 is retained and
reported explicitly. These are codec tests; this run does not claim a graphical
Polkadot.js application session, hardware-wallet firmware support, or load capacity.

The capture and independent readers are reproducible with
`ml-dsa-localnet-blocks.py`, `ml-dsa-localnet-codecs.cjs`, and
`ml-dsa-localnet-native.py` in this directory. They use the repository's pinned
client packages. The compressed capture contains public metadata and finalized
transactions only.

The repository's PAPI installation includes pre-existing submission and block
tracking patches. The tested runtime-context decoder is byte-identical to the
installed upstream copy; the block was also decoded by loading that upstream
file directly. The results record its hash and the dependency versions.

# Test entry points

Use the freshly built native extension before the Python SDK on `PYTHONPATH`.
Point `E2E_WALLET_ENDPOINT` at the activated fast localnet. Run these sequentially
against a shared chain to avoid racing development sponsors, global subnet
settings, and the intentionally bounded ML-DSA admission workers:

```bash
pytest sdk/python/tests/e2e/test_wallet_parity.py -vv
E2E_EVM_ASSOCIATION_DELAY=1 pytest sdk/python/tests/e2e/test_wallet_types.py -vv
pytest sdk/python/tests/e2e/test_mldsa_localnet.py -vv
```

Set `E2E_WALLET_PEERS` to the three RPC URLs of the separately activated chain
using production slot timing, then run:

```bash
pytest sdk/python/tests/e2e/test_wallet_shield.py -n 6 -vv
```

The Shield cases use separate development sponsors. They check cross-validator
signing, successful and failed encrypted inner calls, and signing again from a
client whose cached nonce/generation predates both Shield transactions.

The test suite covers representative operation families and security invariants.
It is not an execution of every dispatchable or every argument/state combination.
Reference weights and production capacity remain outside this run's scope.
