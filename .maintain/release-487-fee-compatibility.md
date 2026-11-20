---
title: "Release 487 fee compatibility"
description: "Owner-authorized restoration of pre-release fee expectations and pending-work pricing."
---

The release owner explicitly requested restoring fee baselines to the state before
PR #3294, then explicitly extended that authorization to all affected calls,
including proxy, EVM withdrawal and basket operations. The owner subsequently
requested removing the additional pending-settlement term entirely, rather than
scaling it. The owner also reported CTO authorization for more actual work per block.

Existing snapshots are restored byte-for-byte from PR base
`8d7f4212f7390695f5bd656e06e16537712d7b7c`. Expectations for new dispatchables remain.
The existing native 20% tolerance and exact EVM comparisons are unchanged.
Restored native cases additionally reject any increase over their pre-PR snapshot.
For the two subnet-registration calls, the existing benchmark/guard model was
already 25,000 rao above that historical snapshot. A fee-only credit of one
DB-write-priced unit bridges that offset; it is a monetary subsidy, not a new
storage-operation estimate or an admission-weight change.
Original benchmark results are unchanged; no recalibration coefficient is used.

Root stake calls and bulk exits omit the additional pending-dividend settlement
term from both declarations and post-dispatch weights. Settlement and deferred
ownership accounting still run. Consequently, actual execution can exceed the
charged execution weight, and busy blocks can take longer. This is the authorized
capacity policy, not a claim that the work became cheaper or fits a measured bound.
The background drain retains its full settlement model.

Additional hashed-owner/proxy authorization work retains its checks and proof
accounting but does not increase legacy execution prices. The additional native
EVM-withdrawal transfer envelope uses that same policy. Native EVM calls receive fee-only discounts for their new alias lookups.
Announcement wrappers and root registration likewise receive fee-only credits
for their added validity/selection reads. Frontier retains the
full alias-mapping resource meter and atomic out-of-gas behavior. Basket and root-claim fee allowances use their pre-487 model; admission retains
the new stability checks.
The immediate staking-list read in the EVM exit adapter is recorded in Frontier's
external resource meter without an additional opcode gas charge.

Claims have independent holdings and pending-credit budgets again. The declared
claim weight covers both full allowances. Pending dust cannot push a full
129-row basket over its 130-unit holdings budget.

Verification keeps `INSTA_UPDATE=no`. Regression cases cover unchanged queue-based
native pricing, signed EVM add/remove receipts with 1, 112 and 256 pending entries,
pre-487 5M/3M gas limits, and a full basket with pending dust.
