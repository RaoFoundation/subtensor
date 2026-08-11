---
title: Alpha imbalance accounting
description: Plan for linear alpha imbalance accounting across issuance and supply-moving paths.
---

# Alpha imbalance accounting

## Goal

Use linear alpha imbalances across issuance and supply-moving paths in the same
way TAO uses credits. Minted alpha must exist as a short-lived credit until it
is resolved into an accounting bucket or explicitly recycled. This makes
unallocated issuance visible in the type system and prevents silent supply
drift.

This work does not change root stake accounting. Root stake is TAO-backed and
is outside the per-subnet alpha-assets system.

## Accounting contract

At transaction and block boundaries, every live non-root subnet should satisfy:

```text
TotalAlphaIssuance[netuid]
    = SubnetAlphaIn[netuid]
    + SubnetAlphaOut[netuid]
    + BalancerAlphaReservoir[netuid]
```

While an imbalance is alive, it represents the temporary difference:

```text
TotalAlphaIssuance
    = stored alpha + outstanding credits - outstanding debts
```

The existing burn semantics are preserved:

- `AlphaBurned` is cumulative telemetry and a logical subdivision of
  `SubnetAlphaOut`; burning does not reduce issuance.
- `AlphaRecycled` is cumulative telemetry for alpha removed from issuance.
- Recycling reduces `TotalAlphaIssuance`.

All new accounting operations should use checked arithmetic. Saturation must
not conceal an accounting underflow or overflow.

## Current gaps

The current alpha imbalance types are not suitable as conservation resources:

- They implement `Clone` and `Copy`, so a value can be duplicated.
- They can be dropped without finalizing their issuance.
- Their constructors are public.
- They implement a single-currency imbalance trait even though alpha is a
  multi-asset system keyed by `netuid`.
- Infallible `merge` and `offset` operations log a netuid mismatch and discard
  one side.
- Coinbase currently ignores the pool-side mint record.
- Subnet genesis and registration directly initialize alpha buckets.
- The alpha-assets issuance map is not yet the canonical value used by
  `get_alpha_issuance`.

## Phase 1: Linear imbalance primitive

Replace the copyable records with a FRAME-style, multi-asset imbalance:

- Mark it `#[must_use]`.
- Remove `Clone`, `Copy`, codec, metadata, and frozen-structure derives.
- Keep constructors private to alpha-assets.
- Include `netuid` and amount in every imbalance.
- Add drop handlers. Dropping an unresolved credit decreases issuance and
  records the amount as recycled; dropping a debt increases issuance.
- Make `merge`, `subsume`, and `offset` fallible when netuids differ.
- Return both original values on a failed combination.
- Add an associated credit type to `AlphaAssetsInterface`.
- Add an explicit settlement operation that consumes an imbalance without
  invoking its drop handler after a destination has accepted the alpha.

For compatibility during the staged rollout, raw `burn_alpha(netuid, amount)`
and `recycle_alpha(netuid, amount)` remain temporarily available. They are
removed after source balances can be withdrawn into credits in later phases.

No imbalance may be stored on chain or exposed through runtime metadata.

## Phase 2: Central resolution layer

Add the alpha equivalent of `spend_tao` in the subtensor pallet:

```text
resolve_to_alpha_in(credit, amount)
resolve_to_alpha_out(credit, amount)
resolve_to_alpha_reservoir(credit, amount)

withdraw_from_alpha_in(netuid, amount) -> credit
withdraw_from_alpha_out(netuid, amount) -> credit
withdraw_from_alpha_reservoir(netuid, amount) -> credit
```

Each resolver must:

1. Verify the credit's netuid.
2. Split only the requested amount.
3. update the destination using checked arithmetic.
4. Return the remainder on success.
5. Return the original credit on failure.

Production code should stop directly mutating `SubnetAlphaIn` and
`SubnetAlphaOut` after this layer is available.

## Phase 3: Coinbase and balancer reservoir

Convert the two alpha emission streams:

- Issue the participant emission as a credit and resolve it into
  `SubnetAlphaOut`.
- Issue the pool emission as a credit and divide it between active
  `SubnetAlphaIn` and `BalancerAlphaReservoir`.

Previously deferred reservoir alpha may become active alongside a new
emission. The correct flow is:

1. Withdraw the old reservoir into a credit.
2. Merge it with the newly issued credit for the same netuid.
3. Resolve the price-active portion into `SubnetAlphaIn`.
4. Resolve the remainder back into the reservoir.

Every error path must either return the credit to its caller or explicitly
recycle it.

## Phase 4: Supply-moving operations

Convert the remaining supply paths:

- TAO-to-alpha swaps withdraw from `AlphaIn` and resolve into `AlphaOut`.
- Alpha-to-TAO swaps withdraw from `AlphaOut` and resolve into `AlphaIn`.
- Stake transfers withdraw and resolve within `AlphaOut`.
- Recycling withdraws a credit and drops or explicitly recycles it.
- Burning withdraws from an individual stake and resolves into the burned
  subdivision of `AlphaOut`, preserving issuance.
- Collateral purchase splitting divides one alpha credit between locked stake
  and burn.
- Protocol-owned alpha resolves into `AlphaOut` and its
  `SubnetProtocolAlpha` subdivision.
- Basket escrow operations use the same withdrawal and resolution helpers.

The main implementation sites are:

- `pallets/subtensor/src/staking/stake_utils.rs`
- `pallets/subtensor/src/staking/recycle_alpha.rs`
- `pallets/subtensor/src/subnets/collateral.rs`
- `pallets/subtensor/src/staking/basket_flush.rs`
- `pallets/subtensor/src/staking/claim_root.rs`

## Phase 5: Genesis, registration, and dissolution

New subnet pool alpha must be issued and resolved into `SubnetAlphaIn`, not
inserted independently of alpha-assets.

Subnet dissolution must:

1. Withdraw `SubnetAlphaIn`, `SubnetAlphaOut`, and reservoir alpha.
2. Merge and recycle the aggregate credits exactly once.
3. Clear subordinate stake, basket, collateral, and protocol records without
   recycling them again.
4. Reset alpha-assets state for the dissolved netuid according to the netuid
   reuse policy.

Genesis builders, benchmark helpers, and test setup utilities must initialize
alpha-assets issuance whenever they seed alpha buckets.

## Phase 6: Migration and canonical issuance

Before making the alpha-assets map authoritative, run a storage-versioned
migration over live non-root subnets:

```text
issuance = AlphaIn + AlphaOut + BalancerAlphaReservoir
```

The migration must overwrite the alpha-assets value rather than increment it,
because existing reserve and genesis alpha was not necessarily created through
alpha-assets.

The migration requires:

- `pre_upgrade` snapshots of the three accounting buckets.
- A benchmarked bound based on the maximum/live subnet count.
- `post_upgrade` checks for every migrated netuid.
- An explicit policy for stale alpha-assets entries belonging to dissolved
  netuids.

After the migration and all production mutations use the resolution layer,
`get_alpha_issuance` can read
`AlphaAssets::total_alpha_issuance(netuid)` directly.

## Tests and rollout gates

### Primitive tests

- Credits and debts cannot be copied or cloned.
- Dropping an unresolved issued credit reverses issuance.
- Settling a credit preserves issuance.
- Split, extract, merge, subsume, and offset conserve the amount.
- Cross-netuid combinations fail and return both values unchanged.
- Zero imbalances do not touch storage.
- Overflow and underflow cannot be hidden by saturation.

### Integration tests

- Coinbase resolves every issued rao.
- Deferred reservoir alpha can become active later without duplication.
- Stake and unstake round trips preserve issuance.
- Burn preserves issuance and recycle reduces it.
- Subnet registration initializes issuance correctly.
- Dissolution leaves zero issuance without double recycling.
- Basket, collateral, protocol-owned alpha, and transaction rollback paths
  preserve the boundary invariant.

### Runtime rollout

1. Ship the hardened primitive without changing economic calculations.
2. Run the alpha-assets tracker in shadow mode and compare it with the derived
   bucket total.
3. Convert coinbase and reservoir handling.
4. Convert swaps, staking, burn, recycle, collateral, and basket paths.
5. Run the reconciliation migration.
6. Make alpha-assets issuance canonical.
7. Remove legacy raw amount APIs and add a CI guard against direct production
   mutations of alpha accounting buckets.

Weights must be regenerated for any extrinsic or hook whose storage accesses
change.
