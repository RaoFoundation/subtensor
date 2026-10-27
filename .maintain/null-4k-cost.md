---
title: Null 4K epoch reference measurements
description: Reference costs, remaining block-budget limits, and the settlement decision for 4K Null subnets.
---

The reference run for commit `4211af94a8bff0b6a8c8c9e669b0c2f9ca0d23ae`
is [36895525694](https://github.com/RaoFoundation/subtensor/actions/runs/36895525694).
Both 4,096-UID fixtures completed without constructing the full weight matrix.
The artifact contains reference measurements, but its patch has not been applied:
the resulting hook charges exceed the runtime's four-second block limit.

| Full hook fixture | Execution weight, seconds | Reads | Writes | DB read charge, seconds | DB write charge, seconds | Total charge, seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Sole-owner pools, no collateral | 0.953670 | 94,914 | 15,376 | 2.372850 | 1.537600 | 4.864120 |
| Shared pools with collateral capture/release | 1.358864 | 96,964 | 31,760 | 2.424100 | 3.176000 | 6.958964 |

Totals use the runtime's RocksDB charges: 25 microseconds per read and
100 microseconds per write. These are charged weights, not measured wall-clock
durations. The fixtures include surrounding subnet/block bookkeeping, a fully
funded epoch, positive stake at every UID, five-parent/five-child registered rings,
and historical dense weight rows. The shared-pool fixture forces general
deposit accounting by including nominators at every UID.

The sole-owner path does not rewrite member shares or the share denominator.
Its largest remaining writes are 4,096 pool balances, 4,096 dividend records,
and 4,096 last-epoch stake snapshots. Shared ownership and collateral require
additional writes. Both fixtures still read stake inputs and ownership state
for thousands of UIDs. Loading the elected validator's single row has removed
the quadratic matrix cost, but does not remove this linear accounting cost.

The atomic path is not safe to deploy at 4K under either measured fixture.
Further work must either reduce the complete worst-case accounting envelope
below the block budget or freeze the reward vector and funded budget at the
epoch and settle it in bounded subsequent blocks. The latter changes payout
timing and is a pending product decision. Mainnet remains gated; no shared
testnet upgrade has been submitted.

Explicit pruning is separately being converted to bounded transactions. Its
new dispatchables, the populated consensus switch, and mechanism removal need
fresh reference weights on the final implementation before deployment.

A read-only mainnet population check on 2026-10-01 found 129 populated subnets,
including subnet 18 with 257 UIDs. The consensus-switch benchmark therefore
covers the target Null mode's entire accepted 4,096-UID population. It must not
assume that historical Yuma state obeys today's configured 256-UID ceiling.

The legacy trim selector adds one mode-selection storage read. The runtime fee
guard confirms its 100-byte baseline increases from 126,404 to 132,654 rao;
only that deliberate change is repinned. These are interim charges before the
new reference measurements, not release fee promises.

These delegation fixtures are not a complete upper bound. Outgoing child lists
are capped at five, but incoming parent lists are unbounded and parents need
not be registered on the subnet. Full stake and dividend processing therefore
also depends on the number of incoming parent edges, including external
recipients. A bounded delegation workload is needed before claiming a complete
4K resource envelope. The measured costs above describe these fixtures only.

The next Null optimization shares alpha/root delegation inputs and caches raw
balances for registered UIDs within one calculation. It preserves per-asset
saturating arithmetic and truncation, and reads external parents without
retaining them in the cache. Cache memory is bounded by the population. This
reduces repeated host reads and fixed-point work; benchmark database counters
can already coalesce repeated accesses, so no charged-weight reduction is
claimed until fresh reference measurements complete.

The release capacity was reduced to a shared 2,500 UIDs on 2026-10-01 at the
owner's request. The 4,096-UID figures above remain historical measurements,
not measurements of the reduced limit. Payouts remain atomic; further accounting
optimizations are deferred. Fresh reference measurements and review findings
remain visible before deployment.
