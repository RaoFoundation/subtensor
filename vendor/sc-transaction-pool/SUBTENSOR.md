---
title: "Pinned transaction pool maintenance patch"
description: "Upstream provenance and the local treatment of indeterminate runtime validation errors."
---

This is `sc-transaction-pool` 40.1.0 copied from
[`RaoFoundation/polkadot-sdk` at `cacb4310f20c7cac83eb3ccd8ed5a5ad4212608a`](https://github.com/RaoFoundation/polkadot-sdk/tree/cacb4310f20c7cac83eb3ccd8ed5a5ad4212608a/substrate/client/transaction-pool).
Upstream source headers and the Substrate GPL-3.0 license are retained. The
manifest resolves workspace inheritance explicitly; SDK dependencies keep the
same repository and revision, and registry dependency requirements are unchanged.
Trailing whitespace on two license lines and one mixed indentation line in a test are
normalized for the repository whitespace gate; the license text is unchanged.

The local patch adds `ChainApi::is_retriable_validation_error`, defaulting to
false. `FullChainApi` identifies only `Error::RuntimeApi` as indeterminate.
Single-state revalidation, fork-aware view revalidation, and fork-aware mempool
revalidation retain previously admitted transactions when that error occurs.
They do not refresh the cached validity, expiry, or mempool validation height.
Fork-aware mempool batches use a separate last-attempt timestamp for ordering,
updated even after indeterminate failures, so repeatedly deferred entries cannot
monopolize the 1,000-entry batch. Retry eligibility still uses the unchanged
validation height; a deferred entry can be retried on the next finalized block.
An explicit invalid result still removes a transaction. Initial submissions
(plain or watched) still fail on indeterminate errors, but do not ban their hash,
so the identical bytes can be retried immediately after local recovery. Explicit
invalidity still bans the hash. Unknown-result handling, pool size bounds, and
expiry rules are unchanged.

A failed fork-aware watched submission also removes the external controller
created by that attempt before returning its error, allowing an immediate retry.
Duplicate-watch rejection returns before this cleanup and preserves the original
watcher; its still-live receiver prevents controller replacement during cleanup.

The single-state pool retains retracted-block candidates whose fresh validation
fails indeterminately in a separate raw retry queue. Its independent count and
byte caps equal the configured ready-plus-future limits. Candidates remain
outside ready/future until fresh validation succeeds. Each candidate expires
after 60 seconds or 64 block heights from its first retraction, whichever comes
first; duplicates and retries do not renew those deadlines. Each enactment
retries at most 16 queued candidates with rotating order. Definitive failures,
expiry, or exhausted queue capacity discard candidates. Ordinary initial reorg
validation remains unchanged.

This prevents local Wasm admission overload from becoming a transaction-eviction
primitive. Regressions exercise the public pool APIs in
`node/src/pool_revalidation_tests.rs` without adding the SDK's test dependency
graph to Subtensor's lockfile. Production changes are confined to
`src/graph/pool.rs`, `src/graph/validated_pool.rs`, `src/common/api.rs`,
`src/single_state_txpool/revalidation.rs`,
`src/single_state_txpool/single_state_txpool.rs`,
`src/fork_aware_txpool/view.rs`, `src/fork_aware_txpool/tx_mem_pool.rs`,
`src/fork_aware_txpool/view_store.rs`, and
`src/fork_aware_txpool/multi_view_listener.rs`.
