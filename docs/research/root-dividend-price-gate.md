---
title: "Root dividend price-gate calibration"
description: "Mainnet evidence and admission limits for weighted root dividend settlement."
---

# Root dividend price-gate calibration

## Decision

Replace the requirement that every held subnet is within 2% of its fast EMA with
a **0.1% basket NAV impact budget** for out-of-band holdings. Keep a separate
**2% gross incoming-value budget** and **2% combined mint-ratio budget**. The
incoming-value check anchors all origins, including those inside the 2% band;
opposing origin moves cannot cancel away changes between deferred claimant groups.

This implements the basket-weighted proposal without claiming that a 0.1% NAV
limit guarantees a 0.1% limit on the number of newly minted shares. The latter
also depends on the incoming dividends' valuation.

## Mainnet sample

Read-only archive RPC: `https://archive.sub.latent.to`. The sample covers blocks
**9,442,681–9,467,881**, every 420 blocks (61 timestamps), with basket quantities
frozen at block **9,468,000**. There are **51 baskets with at least 100 holdings**,
giving **3,111 basket/timestamp observations**. The block interval is about 84
minutes at 12 seconds/block. Every sample's block hash and results are recorded
in [the compact evidence](root-dividend-price-gate-mainnet.json).

| Holding-price admission rule | All large baskets | Taostats |
| --- | ---: | ---: |
| All holdings within 2% | 1.74% | 1.64% |
| Net impact within 0.05% | 33.33% | 31.15% |
| Net impact within 0.10% | 56.48% | 63.93% |
| Net impact within 0.15% | 67.89% | 70.49% |
| Net impact within 0.25% | 83.83% | 81.97% |
| Net impact within 0.50% | 97.11% | 96.72% |

0.1% raises denominator admission from 33% to 56%, while
0.15% and above allow progressively larger NAV discrepancies. This is a policy
tradeoff supported by the observed curve, not an economically optimal threshold
proved by the sample.

These percentages are **denominator eligibility estimates, not deposit counts**.
The study holds basket quantities fixed and uses historical spot prices; it does
not replay the historical holdings, queue, validator scheduler, claimant changes,
or the runtime's realizable marginal quotes. The production incoming and joint
checks may reject some denominator-eligible batches. The observations are
correlated, and are not an independent statistical sample or an APY forecast.
Missing anchors are excluded rather than treated as passing (none were missing
in this post-488 sample). This does not reproduce the author's 30-day study.

## The reported Taostats example

At block **9,448,000**, hash
`0x386f3e2fba2778091871a1c4a2c932b724e3c7db5dba7a033f8f3ee48812746f`,
the basket hotkey is
`5GKH9FPPnWSUoeeTJp19wVtd84XqFW4pyK2ijV2GsFbhTrP1`.
Its **6,178,568,439,438 shares** exactly match the Taostats report; realizable NAV
is **5,201.946135239 TAO**, with 128 holdings and 15 out-of-band prices.

The independently calculated spot-weighted net impact is about **+0.080%**,
close to the report's +0.073% (the report uses an earlier snapshot's weights).
The 0.05% denominator gate rejects this state; 0.1% admits it. This does not mean
the whole queued batch has a mint error below 0.1%: it includes incoming dividends
from volatile origins as well as the report's hypothetical stable SN28 dividend.
Using the runtime's realizable holdings gives **+0.0783%** NAV impact. The spot
proxy for the actual incoming queue gives **1.691% gross incoming impact** and
**-0.1646% combined mint-ratio impact**; these fit the separate 2% budgets.
At block **9,468,000**, Taostats instead has **-0.1907%** realizable NAV impact,
so the proposed NAV gate still defers settlement despite its smaller incoming
impact. This is intentional: the policy permits small weighted discrepancies,
not unconditional settlement.
Point-in-time queue and incoming-value results are retained in
[the point evidence](root-dividend-price-gate-taostats.json).

Source proposal: [Taostats runtime 488 price-gate analysis](https://s3.hippius.com/rufus/public/root-dividend-flush-price-gate.html).

## Runtime calculation and limits

The existing realizable NAV sweep supplies each holding's value `v`. Outside the
existing 2% spot/fast band, its admission reference is `floor(v * fast / spot)`;
inside the band it remains `v`. Root cash, fixed-price mechanisms, and dissolving
subnets retain their existing treatment. Missing or zero price anchors reject
nonzero dynamic values. Terminal zero-value rows do not pin the batch.

The already-computed marginal value of each incoming credit is rebased to its
fast anchor for the incoming reference. This preserves its live liquidity
discount without extra swap simulations. It is an oracle-based admission
reference, not a counterfactual replay of a different AMM or a fair-price oracle.

Admission requires:

1. `abs(N_live - N_ref) / N_ref <= 0.1%`.
2. `sum(abs(B_i_live - B_i_ref)) / sum(B_i_ref) <= 2%`.
3. When pricing outstanding shares, the deviation of `B_live / N_live` from
   `B_ref / N_ref` is at most 2%.

The third condition also bounds the ordinary mint with unminted escrow yield:
adding a nonnegative yield term to the denominator dampens the relative ratio
error. The initial par mint is covered by the incoming-value check. Empty/zero
NAV handling and actual mint math retain their existing rules.

Accepted deposits **still use live realizable values on both sides**. No alpha
is swapped during accumulation. A rejected batch rolls back tentative credits,
share changes, and deferred conversions, and returns the credits to the queue.
No dispatchable, storage schema, fee/gas baseline, or measured weight changes
are introduced; the existing quote envelope covers the same NAV sweep and
per-origin simulations. No spec-version bump is included on top of release-489.

## Reproduction

Install the repository's `ts-tests` dependencies, then run:

```sh
BASKET_RESEARCH_RPC=https://archive.sub.latent.to \
BASKET_RESEARCH_START=9442681 BASKET_RESEARCH_END=9468000 \
BASKET_RESEARCH_STEP=420 BASKET_RESEARCH_OUTPUT=/tmp/basket-gate-mainnet.json \
node scripts/analyze-root-dividend-gate.cjs
```

Use `BASKET_RESEARCH_POINT=9448000` and
`BASKET_RESEARCH_HOTKEY=5GKH9FPPnWSUoeeTJp19wVtd84XqFW4pyK2ijV2GsFbhTrP1`
for the actual point-in-time basket and pending credits. Incoming-value figures
in this read-only tool use spot marks; only the point's NAV comparison uses the
runtime's realizable holdings. They must not be presented as an exact runtime
mint replay. Public archive availability and rate limits can vary.

## Verification

Workspace nextest: **4,275 passed**, 16 skipped (one test reported a leaked
subprocess); full pallet unit suite: **1,940 passed**, 9 ignored. Workspace
all-features Clippy, Zepter, Rust CI path checks, generated-doc drift, SDK
coverage/name checks, and formatting pass.

The new protected regression reproduces a tiny volatile holding blocking an
otherwise healthy dividend, and verifies settlement followed by a claim that
increases actual root stake by the earned dividend. It fails with the original
all-holdings price gate restored and passes with the weighted gate. Existing
protected tests are unchanged. Arithmetic tests cover limits, large values,
opposing origin moves, and the combined numerator/denominator check.

The current RustSec database also reports **RUSTSEC-2026-0459** for Frontier's
`sqlx -> libsqlite3-sys 0.30.1`, which is present on release-489 independently of
this change. The finding remains visible and is not waived by this report.
