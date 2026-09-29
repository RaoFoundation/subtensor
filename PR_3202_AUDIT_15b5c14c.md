---
title: "PR 3202 security and correctness audit"
description: "Findings from the complete pinned diff review, excluding weight issues and nits."
---

**Verdict: changes requested. One MEDIUM security/correctness finding.**

## 1. [MEDIUM] Foreign null membership can veto an existing hotkey's global rotation

**Location:** `pallets/subtensor/src/swap/swap_hotkey.rs:820-822`, with the admission mismatch in `pallets/subtensor/src/null_consensus.rs:207-218`.

[Global swap integration at the audited head](https://github.com/RaoFoundation/subtensor/blob/15b5c14c62e77de8008a5b9d800ccd13c260a06e/pallets/subtensor/src/swap/swap_hotkey.rs#L820-L822) · [Null destination and ownership checks](https://github.com/RaoFoundation/subtensor/blob/15b5c14c62e77de8008a5b9d800ccd13c260a06e/pallets/subtensor/src/null_consensus.rs#L207-L218)

The null-only rotation path permits an attacker to rename their miner to a hotkey already globally associated with another coldkey, provided that destination is not registered on the selected subnet. It checks subnet membership and the source null owner's authority, but never checks the destination's global owner or obtains the destination hotkey's signature. The ordinary PoW registration path does check an existing global owner, so rotation bypasses that restriction.

A subsequent global rotation by the legitimate staking owner iterates every active subnet (`swap_hotkey.rs:594-608`) and unconditionally invokes `swap_null_miner`. On the attacker's subnet, the null owner differs from the legitimate signing coldkey, so `NonAssociatedColdKey` propagates out and rolls back the entire global swap. This applies even if the victim never joined that subnet, and remains effective when its null mode is paused because the null registry is retained.

**Concrete triggering sequence:**

1. Victim coldkey V already owns staking hotkey H. Select an open null subnet N on which H has neither null nor Yuma membership.
2. Attacker coldkey A registers a fresh, globally unassociated null hotkey X on N using valid PoW.
3. A calls `swap_hotkey_v2(X, H, Some(N), false)`. The null-only branch succeeds and creates `NullMiners[N, H]` owned by A while `Owner[H]` remains V. The configured single-subnet swap cost is 0.001 TAO, plus transaction fees and the initial registration work/fee.
4. V attempts `swap_hotkey_v2(H, H2, None, false)` to a clean destination. Processing N fails the null-owner check and the transaction rolls back. `keep_stake=true` does not avoid the ownership check.

**Impact:** A third party can deny all-subnet hotkey rotation, including rotation needed after compromise, without the victim's signature or authority over any subnet the victim actually uses. Individual swaps on unaffected subnets remain possible; this finding does not establish theft or a chain-wide halt. The registration has no ordinary expiry/removal path that the victim controls.

**Fix direction:** Make the global staking swap independent of null registrations owned by other coldkeys: migrate only null rows the signer owns and preserve unrelated null state/endpoints. Also prevent null-only rotation from attaching to an existing third-party staking hotkey without appropriate authorization. Preserve recovery for legitimate pre-existing cases where the two ownership domains differ.

**Regression needed:** Create H under V first, PoW-register X under A on an unrelated null subnet, rotate X to H through the public dispatchable, then attempt V's global rotation. Assert that foreign registration cannot veto V's rotation or transfer A's null rewards. Cover active and paused null mode and both keep-stake settings. Existing tests cover coexistence and rollback but do not assert this victim-recovery property.

**Evidence level:** Confirmed by source tracing at the pinned head; the sequence was not executed against a runtime or live chain.

## Review scope and validation

- PR: https://github.com/RaoFoundation/subtensor/pull/3202
- Head: `15b5c14c62e77de8008a5b9d800ccd13c260a06e`.
- Base (`main`): `c004cebf360f4088187ee49d851dfb1a1eaaf710`.
- Complete changed diff: **241 files, 5,515 additions and 1,046 deletions**. Reviewed runtime/pallet logic, tests, benchmarks, SDK/CLI, and generated artifacts. Weight issues and nits are excluded from findings.
- Traced registration authorization and signed-mode binding, mode transitions, integer reward accrual/claims, dissolution settlement, hotkey/coldkey ownership, serving, voting maintenance, transaction extensions, and proxy/utility behavior.
- Generated documentation and catalog changes were checked structurally and semantically against source; 126 documentation files change only source-line references. Checked all 1,020 source-link occurrences in changed documentation/catalog files for valid ranges and all 552 catalog source records for matching symbols.
- `git diff --check` passed for the pinned base/head. No builds, runtime tests, SDK tests, or live transactions were run for this audit. Validation counts in the PR description were not independently rerun.
- The earlier audit's PoW-to-paid mode confusion, blocked conviction succession, and stale voting-power maintenance have corresponding fixes in this head and are not carried forward as current findings.
- Rechecked the remote PR head and main base at completion; both remained unchanged. Concurrent uncommitted edits appeared in the shared checkout. Findings and source citations refer to the pinned PR commit, not those edits; implementation files were not modified by this audit.
