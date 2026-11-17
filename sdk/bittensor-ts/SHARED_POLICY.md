---
title: "Shared SDK policy"
description: "Rust policy ownership, language adapters, compatibility, and verification for the Python and TypeScript SDKs."
---

# Shared SDK policy

`sdk/bittensor-core/src/policy/` owns the portable decisions used by Python,
native Node, and browser WASM. The N-API, PyO3, and WASM bindings all call
`policy::evaluate_json`. The policy layer performs no network or filesystem I/O.
Existing core modules continue to own keys, cryptography, SCALE, runtime metadata,
signing payloads, native wallet persistence, and native transport.

## Ownership

| Core domain | Shared decisions | SDK adapters |
| --- | --- | --- |
| `validation` | Amount parsing and units; weight normalization and admission; hyperparameter scaling; PoW seals, targets, freshness and admission; timelock rounds and envelopes; pruning eligibility; upgrade SCALE/WASM inspection | Balance, weights, hyperparameters, registration mining, timelock, pruning, upgrade |
| `financial` | Emission shares; Gamma budgets and limits; registration deposits and sponsorship; fee filters, payers and affordability; root-claim declarations and fees; order terms and authorization; basket math; staking routes and calls; lock, Gamma and collateral projections | Financial helpers, semantic intents, preflight adapters and read models |
| `transaction` | Wallet profiles and capabilities; signature formats; external signer identity; metadata eligibility; transaction and batch policy; semantic call shapes; receiving routes; HTTP admission and freshness; nested dispatch outcomes; nonce reuse rules | Node/browser execution, Python executor, signers, receipts, HTTP authentication and receiving |
| `protected` | Proxy grants and delays; setup, rotation and recovery plans; configuration transitions; announcement identity and status; replay validation; funding and refill; registration traversal | Protected-account APIs, announcement stores and Python CLI workflows |
| `upgrade` | Artifact identity and provenance checks; eligible deployment proxies; signer ordering; weight margins; approval state | Upgrade APIs, Python CLI and multisig preferences |

The TypeScript source review also covered the modules outside this table.
Generated catalogs and namespace/type definitions remain generated data. RPC
clients, endpoint health, subscriptions, historical reads, local persistence,
browser extensions, Ledger/Vault transports, and submission watchers remain host
adapters. They collect inputs, invoke core decisions, execute I/O, and restore
language values. Python dataclasses, exceptions and CLI presentation, and
TypeScript promises, callbacks and exported interfaces remain language APIs.

Stateful orchestration stays beside the resource it controls: pinned-state RPC
reads, nonce queues, retries and reorg observation, confirmation callbacks, and
atomic journal/file updates. Policy receives the relevant state and returns a
decision or call plan. In particular, approved staking replay still consumes the
saved amounts and limits without refreshing them from the chain.

## Binding contract

The internal operation namespace uses a versioned JSON envelope:

```json
{"version":1,"value":"9007199254740993"}
```

Errors carry `kind` (`type`, `range`, or `invalid`) and `message`. Adapters map
these to language exceptions. Wide integers cross the boundary as decimal
strings and bytes as `0x` hexadecimal. Arithmetic uses exact integers or rational
numbers; conversion to a floating display value happens only at the API edge.
Structural call operations use a separate adapter to preserve opaque codec values
such as Python integers and JavaScript `bigint`/`Uint8Array` without confusing
them with decimal input to arithmetic operations.

Compatibility differences are explicit inputs to core policy, rather than
separate implementations. Examples include Python's historical amount rounding,
legacy refill estimates and optional manifest checks, and TypeScript's stricter
decimal and manifest validation. Runtime admission and chain-derived monetary
rules are shared. Fixed root-claim monetary weights intentionally differ from
current declared execution weights, matching `runtime/src/staking_fee.rs`.

The browser resolves the policy binding through the package's `browser` import
condition. Call `await initBrowser()` before invoking policy-backed APIs.
Constants needed during module loading are generated from core and do not
initialize a native addon or WASM module. There is no JavaScript or Python
fallback implementation when the matching native core is missing.

## Updating and verifying policy

Change core policy and the adapters together. Build the native binding, then run
`npm run policy:generate` from `sdk/bittensor-ts` when core constants change.
That command generates both `src/policy-catalog.ts` and Python's
`bittensor/_policy_catalog.py`. `npm run check:policy-catalog` detects drift.
Rebuild/install the Python core wheel as well as the Node and WASM artifacts;
an older installed wheel cannot provide new policy operations.

The shared fixture `sdk/bittensor-core/tests/fixtures/sdk-policy.json` executes
through Rust, N-API, WASM and PyO3 with identical expected results. Additional
domain tests exercise the public language adapters and existing compatibility
tests preserve approved replay, wallet, signing and protected-account behavior.
Run the complete TypeScript/Python suites, Rust tests and Clippy, browser smoke
tests, and source/catalog parity checks after a refactor. Live signed-transaction
tests require a development runtime built with `metadata-hash`, as in CI's
metadata-hash test configuration.

For host-independent policy tests, run
`cargo test -p bittensor-core --no-default-features --test policy_portable`.
This exercises the shared corpora without compiling the host-only unit tests.

## Approved root-claim expectation corrections

The owner explicitly approved the corrections listed in
`/tmp/sdk-shared-policy-fee-review.md` with “Approve the listed corrections.”
The scope is `sdk/python/tests/unit/test_root_claim_fee.py`; it does not authorize
other fee baselines or changes to the protected admission test. These are runtime
model vectors, not claims of fresh live-node fee measurements.

| Expectation | Previous | Corrected |
| --- | ---: | ---: |
| Hotkey declared weight, including extension | 2,051,581,040,648 | 2,093,438,599,648 |
| Coldkey declared weight, including extension | 2,476,947,290,768 | 2,518,804,849,768 |
| Either single-call fee, including extension | 8,224,263 | 7,488,653 |
| Batch declared weight | 4,528,660,932,786 | 4,612,376,050,786 |
| Batch fee | 16,481,677 | 15,010,457 |
| Reserved fee | 8,209,263 | 7,473,653 |

The charged allowance is explicitly 29,894,612,996 ref-time rather than equated
to the current declaration helper. The scan-only work quote remains 7,806,076,
and its charged fee is capped at the 7,473,653 reserve. The old estimator counted
a fixed cost twice and conflated current declaration weights with the runtime's
fixed monetary model.
