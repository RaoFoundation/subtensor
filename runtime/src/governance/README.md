# Runtime Governance

This directory wires Subtensor's concrete governance configuration into the
generic governance pallets.

The runtime uses:

- `pallet_multi_collective` for named membership sets.
- `pallet_referenda` for the track state machine.
- `pallet_signed_voting` for per-account aye/nay voting.
- `pallet_subtensor` root-registration and subnet state to select rotating
  collective members.

## Tracks

`tracks.rs` defines two static tracks.

| Id | Name | Proposer set | Voter set | Strategy |
| -- | ---- | ---- | ---- | ---- |
| `0` | `triumvirate` | `MemberSet::Single(Proposers)` | `MemberSet::Single(Triumvirate)` | `PassOrFail`: 7 day decision period, 2/3 approve, 2/3 reject, approval hands off to track `1`. |
| `1` | `review` | `None` | `MemberSet::Union(Economic, Building)` | `Adjustable`: 24 hour initial delay, 2 day max delay, 75% fast-track threshold, 51% cancel threshold. |

Track `1` must stay non-submittable (`proposer_set: None`). It is reached
only through `ApprovalAction::Review` after track `0` approval. This is the
runtime invariant that prevents direct submission of a root call into the
review delay.

`EaseOutAdjustmentCurve` shapes review delay changes as `1 - (1 - p)^3`.
Early net collective signal has a visible effect on the dispatch delay, and
then tapers off as the vote approaches the hard fast-track or cancel
threshold. Net approval pulls the scheduled call toward the submission
block; net rejection pushes it toward `max_delay`.

## Collectives

`collectives.rs` defines the consensus-facing `CollectiveId` values:

| Id | Codec index | Members | Term |
| -- | -- | -- | -- |
| `Proposers` | `0` | min `1`, max `20` | none |
| `Triumvirate` | `1` | exactly `3` | none |
| `Economic` | `2` | exactly `16` | 60 days |
| `Building` | `3` | exactly `16` | 60 days |
| `EconomicEligible` | `4` | max `64` | none |

Codec indices are consensus-facing. Do not reorder or renumber them.

The pallet-level `MaxMembers` is `64` because it is the storage bound shared
by all collectives. The per-collective `max_members` values above are the
logical limits.

## Voting Sets

`member_set.rs` adapts collectives into the `SetLike<AccountId>` interface
used by referenda tracks.

- `Single(id)` reads exactly one collective.
- `Union(ids)` concatenates members from several collectives, sorts them,
  and deduplicates them.

The review track uses `Union(Economic, Building)`, so an account that is in
both collectives is counted once in the signed-voting snapshot and in the
threshold denominator.

## Economic Rotation

`EconomicEligible` is a staging set for Economic selection. It is maintained
by `EconomicEligibleSync`, which implements `OnRootRegistrationChange` for
`pallet-subtensor`.

- A coldkey is added when its root-registered hotkey count moves from `0`
  to `1`.
- A coldkey is removed when its count moves from `1` to `0`.
- `EconomicEligibleInspector` lets Subtensor try-state verify that the
  collective matches the root-registered coldkey set.

`term_management.rs` rotates `Economic` by calling
`TermManagement::top_validators(16)`.

Selection steps:

1. Read all `EconomicEligible` coldkeys.
2. Read `RootRegisteredEma` for each coldkey.
3. Ignore candidates with fewer than `ECONOMIC_ELIGIBILITY_THRESHOLD`
   samples (`210`, roughly 30 days with the current sampler cadence).
4. Sort remaining candidates by descending EMA value.
5. Set `Economic` to the top 16.

The EMA sample value is provided by `ema_provider.rs`. A sample is:

```text
liquid TAO balance
+ TAO value of alpha held by owned hotkeys across all subnets
```

Sampling is incremental: 8 subnets per provider step and at most 256 owned
hotkeys valued per sample. Subtensor calls `tick_root_registered_ema()` from
its `on_initialize` hook, so the sampler advances once per block. The EMA
blend alpha is `0.02` and new root-registered coldkeys start from zero.

## Building Rotation

`term_management.rs` rotates `Building` by calling
`TermManagement::top_subnet_owners(16, MIN_SUBNET_AGE)`.

Selection steps:

1. Iterate all subnet netuids.
2. Ignore subnets younger than `MIN_SUBNET_AGE` (`180` days in production).
3. For each mature subnet, read its owner and moving price.
4. Keep only each owner's highest moving price across all mature subnets.
5. Sort owners by descending best price.
6. Set `Building` to the top 16.

This gives one seat per owner coldkey, based on that owner's strongest
mature subnet.

## Rotation Behavior

`pallet_multi_collective` runs term hooks from `on_initialize` whenever
`block_number % term_duration == 0`. For this runtime only `Economic` and
`Building` have a term duration, so only those collectives rotate
automatically.

Both rotating collectives require exactly 16 members. If selection returns
fewer than 16 accounts, `do_set_members` fails with `TooFewMembers`; the
runtime logs the failure and leaves the previous member list unchanged.

Root can call `force_rotate` for a rotating collective to run the same hook
outside the normal cadence.

## Referenda Runtime Constants

`mod.rs` wires these constants:

| Constant | Value | Meaning |
| ---- | ---- | ---- |
| `MaxQueued` | `20` | Maximum active referenda. |
| `MaxActivePerProposer` | `5` | Maximum active referenda per proposer. |
| `MaxVoterSetSize` | `64` | Bound for signed-voting snapshots. |
| `MaxPendingCleanup` | `40` | Cleanup queue capacity for completed polls. |
| `CleanupChunkSize` | `16` | Per-idle-block vote-record cleanup chunk. |

Compile-time assertions keep these constants aligned with the collective
sizes. The widest voter set is currently `Economic + Building` (`32`
before deduplication).

## Operational Notes

- `referenda.submit` is signed and only works on tracks with
  `proposer_set: Some(_)`. In this runtime, that means only track `0`.
- There is no proposer-only cancel or withdraw call. Emergency termination
  is `referenda.kill`, gated by root.
- Voting is snapshot-based. Active polls are not affected by later
  collective rotations.
- Dispatch is wrapped through `referenda.enact(index, call)`, which marks
  the referendum `Enacted` in the same root call that dispatches the inner
  proposal.
