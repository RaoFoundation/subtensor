---
title: "Pending Ethereum RPC repair validation"
description: "Production-node before-and-after regression evidence, resource limits, fixture corrections and audit completion."
---

The final results and source hashes in this package describe the local candidate
at `2d5089da3bcb292cbf557d9b22ff34582e23b3f4` plus the uncommitted corrections.
No production runtime code, weights, spec version, dependency or lockfile changed
for the RPC follow-up. The runtime Wasm remains
`8a928b07796c445d864ae021562629cf9524e2a30d940e8b490234c33e769377`.

## What changed

Trusted pending reconstruction has a host-only execution marker and an independent
reservation split from the former Other bucket. Unknown raw exports cannot claim
that reservation by using an allowed method name. Aggregate limits remain eight
active metered executions, 64 MiB of active encoded input, 64 burst tokens and 32
refills per second. Validation, EVM and Contracts keep their independent quotas.

Reconstruction pins one pool snapshot and retries only the exact capacity error
on the same transactional step. Successful prior extrinsics remain in the
pending overlay. API errors and failed inherents cannot become a successful
partial result. Included user dispatch errors retain fee and nonce effects;
invalid pool transactions are skipped transactionally.

Pending call and gas estimation use the reconstructed typed runtime API, with
fresh proof recording and rollback for every simulation. Pending storage/code
queries propagate errors instead of silently returning empty values.

## Executed comparison

`live_rpc.py` runs against a fresh three-validator localnet for each artifact,
with normal 12-second slots, archive storage, production runtime weights and the
MS master switch off. `localnet.sh` is the repository launch script adapted to
isolated paths/ports; `genesis.py` establishes the separate network identity.
The baseline and repaired comparisons deliberately share this isolated genesis
and sequentially purge chain databases; they do not run concurrent same-genesis
networks. Both wrappers shut down validators on success or failure.

The baseline node is SHA-256
`5cb0dc2735acdcc1d3afc16c3c389ac10020ef7d9ce0e686ad4497c7d749ae3c`;
the repaired node is
`53ceef8b720f2352ecf701da5f1faf6a480c87e7e5bcf11d8bf0fbaa5574e853`.
See the individual JSON results and compressed full logs for exact values,
transaction hashes, finality and timings.

The corrected baseline proves that a valid pending value call loses pending
funding, while the same call succeeds at latest after funding finalizes. Its gas
estimate hits admission capacity, unknown-export flooding starves pending reads,
and a 32-entry native transfer pool cannot be reconstructed. The repaired suite
asserts the opposite successful outcomes, complete balances, simulation rollback,
valid empty code/storage, and unchanged latest-state behavior.

`ethereum_pending_transfers.py` adds 32 actual sequential Ethereum transactions
with fresh keys. It verifies pending sender nonce and complete recipient balance
before a block advances, then checks every successful receipt against canonical
finalized blocks and reconciles exact native/EVM balances against the existing
base-fee-only runtime policy. It explicitly records the confirmed receipt fee
reporting discrepancy; receipt accuracy is not a passing assertion. The original
failed fee assertion and recovered original receipts are preserved. See
[the separate finding](receipt-fee-observation.md). Private keys
remain in memory and are not included in this package.

`pending_contract_semantics.py` separately exercises the typed call/create paths;
its result file documents the actual assertions and supported local setup.

## Fixture corrections and retained failures

The earliest balance expectation forgot that `eth_getBalance` exposes reducible
balance, excluding the 500-rao existential deposit. The next fixture requested a
1-rao transfer to a new account, which is below that minimum. Frontier maps the
currency rejection to `OutOfFund`, so those call/estimate failures do not prove
an overlay defect. Files prefixed `below-ed-`, and the original first-attempt
files, preserve these superseded runs rather than counting them as passes.

The final common fixture transfers twice the existential deposit. It also proves
that a 1-rao latest call fails and the corrected latest call succeeds after
funding finalizes. Compare each result's `script_sha256` with its retained script;
`live_rpc_baseline.py` is the earlier, superseded baseline source, while
`live_rpc.py` is the corrected common comparison.

## Limits and compatibility

At most two pending jobs hold leases. Each snapshot permits 256 total
pool/inherent entries and 8 MiB of copied encoded payload. The already bounded
pool iterator additionally clones its index of Arc references; encoded byte caps
are not a total heap-memory measurement. A job shares a 15-second deadline,
1,024-attempt cap and 64 MiB attempted-input cap across clones and rollback.
Dense workloads can return an explicit overload; this is not an unlimited
availability or production throughput claim.

The deadline is cooperative between synchronous runtime executions. A running
Wasm call retains its execution permit until completion, and a simulation result
that finishes after the deadline is discarded. Consensus remains Onchain; pending
work is never mislabeled consensus to bypass admission.

Nonempty state overrides combined with `pending` explicitly return
`state overrides are not supported with pending state`. Public ApiExt cannot
merge the raw override overlay into the retained pending overlay. Latest and
historical overrides retain their original implementation. Current API version 6
is exercised live; versions 1–5 retain typed source branches and compilation
coverage, not additional live historical-runtime coverage.

## Local gate

The final production source passed the full local preflight, including both
Clippy configurations, formatting, zepter, SDK/docs generation checks, release
node build and metadata drift verification. This follow-up run selected all
uncommitted corrections against HEAD and reported 2,277 passing tests, zero
failures and 19 ignored across 24 targets. The prior PR-wide workspace run is
preserved separately; the totals overlap and must not be added together.
Seventeen unique admission tests run in both node library and binary targets.
No claim is made that a new remote CI suite ran.

## Final outcome

The corrected baseline comparison, repaired pending RPC suite, corrected
Ethereum state/accounting suite and contract simulation suite completed
successfully. The receipt fee accuracy finding remains explicitly unresolved;
it is not erased by the narrower state/accounting pass.

The contract checks verified code returned by simulated creation, creation and
call rollback, write/revert storage isolation, the existing base-fee GASPRICE
policy, and access-list warming through actual opcode gas measurements. Returned
gas estimates successfully executed the corresponding calls. The temporary
creator allowlist was restored and all feature flags remained unchanged.

All original and follow-up task validators are stopped; `shutdown.json` records
an empty process scan and closed RPC/P2P/preflight/metrics ports. Changes are local
and uncommitted. `source-manifest.json` pins the repair source before preflight;
`final-source-manifest.json` records the final local code and unchanged lockfiles.
The parent `initial-manifest.json` is a preserved historical snapshot, superseded
by the parent's current `manifest.json` and the completed follow-up evidence.
