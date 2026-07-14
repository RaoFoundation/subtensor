# On-Chain Governance

Subtensor governance is implemented as track-based referenda backed by
signed collective voting. The live runtime wiring is in
`runtime/src/governance`; the generic building blocks are
`pallets/referenda`, `pallets/signed-voting`, and
`pallets/multi-collective`.

Governance has two stages:

1. A proposal is submitted by an authorized proposer and decided by the
   three-member Triumvirate.
2. If the Triumvirate approves it, the call is handed to a separate
   collective review track where the Economic and Building collectives can
   accelerate, delay, or cancel enactment.

The governed call is dispatched as root only if it survives this flow.

## Runtime Tracks

| Track | Name | Submitters | Voters | Decision |
| ---- | ---- | ---- | ---- | ---- |
| `0` | `triumvirate` | `Proposers` collective | `Triumvirate` collective | `PassOrFail`: 7 day decision period, 2/3 approve, 2/3 reject. Approval delegates to track `1`. |
| `1` | `review` | None | Union of `Economic` and `Building` | `Adjustable`: scheduled at 24 hours by default, adjustable up to 2 days, 75% approval fast-tracks, 51% rejection cancels. |

Track `1` is intentionally not directly submittable. Its only entry point
is the `ApprovalAction::Review` handoff from track `0`, so a proposer
cannot bypass Triumvirate approval and place a root call directly into the
review delay.

Both tracks use `pallet-signed-voting`. When a referendum opens, the voting
backend snapshots the eligible voter set and uses that snapshot for the
entire poll. Members rotated out after the poll opens keep their vote on
that poll; members rotated in later cannot vote on old polls. For the review
track, the Economic and Building member lists are unioned and deduplicated,
so an account present in both collectives counts once.

## Collectives

| Collective | Size | Rotation | Purpose |
| ---- | ---- | ---- | ---- |
| `Proposers` | min `1`, max `20` | Manual | Accounts allowed to submit on the Triumvirate track. |
| `Triumvirate` | exactly `3` | Manual | Approval body for submitted proposals. |
| `Economic` | exactly `16` | Every 60 days | Top root-registered validator coldkeys by smoothed stake value. |
| `Building` | exactly `16` | Every 60 days | Top subnet-owner coldkeys by their best mature subnet price. |
| `EconomicEligible` | max `64` | Automatic sync, no voting role | Candidate pool for `Economic`; mirrors coldkeys with at least one root-registered hotkey. |

Membership is stored by `pallet-multi-collective`. In the runtime all
membership mutation origins are root-gated, so changes to curated
collectives are expected to go through governance once sudo/root authority
is replaced by the governance flow. The rotating collectives can also be
force-rotated by root.

The rotating collectives have `min_members == max_members == 16`. If a
rotation computes fewer than 16 eligible accounts, `set_members` fails the
minimum-member check and the previous membership remains in storage. The
failure is logged instead of partially rotating the set.

## Economic Selection

The Economic collective is selected from `EconomicEligible`, not directly
from every account on chain.

`EconomicEligible` is synchronized from root registration:

- When a coldkey's root-registered hotkey count moves from `0` to `1`, the
  coldkey is added to `EconomicEligible` and its root-registered EMA is
  initialized at zero.
- When the count moves from `1` to `0`, the coldkey is removed and its EMA
  state is cleared.
- The cap is `64`, matching the root subnet UID limit.

Each block, `pallet-subtensor` advances the root-registered EMA sampler.
The governance runtime provides the sample value through
`StakeValueProvider`: liquid TAO balance plus the TAO value of alpha held
by the coldkey's owned hotkeys across all subnets. The provider works in
chunks of 8 subnets and values at most 256 owned hotkeys per sample.

The EMA uses alpha `0.02`. A coldkey must have at least `210` completed
samples before it can be selected for `Economic` membership. With the
current sampler cadence this is roughly a 30 day warmup. At rotation time,
the runtime ranks eligible coldkeys by descending EMA value and takes the
top 16.

## Building Selection

The Building collective represents subnet owners.

At rotation time, the runtime iterates all subnets and ignores any subnet
younger than `MIN_SUBNET_AGE`, which is 180 days in production. For each
remaining subnet it reads:

- `NetworkRegisteredAt`
- `SubnetMovingPrice`
- `SubnetOwner`

An owner may control more than one mature subnet. The runtime keeps only
that owner's highest observed `SubnetMovingPrice`, then ranks owners by
that best price and takes the top 16. This means one coldkey can receive at
most one Building seat, even if it owns multiple high-priced subnets.

## Referendum Lifecycle

1. A member of `Proposers` calls `referenda.submit(0, call)`.
2. `pallet-referenda` checks the proposer set, global queue limit
   (`MaxQueued = 20`), and per-proposer limit (`MaxActivePerProposer = 5`).
3. Triumvirate voters use `signed_voting.vote(index, approve)` or
   `signed_voting.remove_vote(index)`.
4. If 2/3 of the Triumvirate snapshot votes approve before 7 days elapse,
   the parent referendum becomes `Delegated` and a child review referendum
   is created on track `1`.
5. If 2/3 reject, the referendum becomes `Rejected`. If neither threshold
   is reached before the deadline, it becomes `Expired`.
6. The review child schedules the root call at `submitted + 24 hours`.
   Economic and Building voters can approve, reject, change their vote, or
   remove their vote while the review is ongoing.
7. If review approval reaches 75% of the snapshot, the call is rescheduled
   for the next block and the referendum becomes `FastTracked`.
8. If review rejection reaches 51%, the scheduled call is cancelled and the
   referendum becomes `Cancelled`.
9. Otherwise, net approval moves the scheduled block earlier and net
   rejection moves it later, up to the 2 day maximum delay.
10. When the scheduler invokes `referenda.enact`, the inner call is
    dispatched with root origin and the referendum becomes `Enacted`. The
    event records whether the inner dispatch returned an error.

There is no proposer-only withdraw or cancel extrinsic in the current
implementation. Privileged termination is `referenda.kill`, gated by root,
and can kill an ongoing, approved, or fast-tracked referendum before
dispatch.

## Review Delay Formula

Review uses the runtime's `EaseOutAdjustmentCurve`, so net vote progress is
shaped as `1 - (1 - p)^3`. Early net collective signal has a visible effect
on the dispatch delay, then the curve tapers off as the vote approaches the
hard fast-track or cancel threshold.

If approval is greater than or equal to rejection:

```text
net = approval - rejection
progress = net / fast_track_threshold
curved = 1 - (1 - progress)^3
delay = initial_delay * (1 - curved)
```

If rejection is greater than approval:

```text
net = rejection - approval
progress = net / cancel_threshold
curved = 1 - (1 - progress)^3
delay = initial_delay + curved * (max_delay - initial_delay)
```

With production constants, `initial_delay = 24 hours`,
`max_delay = 2 days`, `fast_track_threshold = 75%`, and
`cancel_threshold = 51%`. If a recomputed target is already in the past,
the referendum is fast-tracked.

## Storage and Audit Trail

Referendum statuses remain queryable after conclusion. Votes are stored by
`pallet-signed-voting` while a poll is active, then cleaned lazily after the
poll completes. Per-voter records are no longer read after the tally is
removed, so lazy cleanup affects storage hygiene rather than governance
correctness.

Relevant events:

- `referenda.Submitted`
- `referenda.Delegated`
- `referenda.Rejected`
- `referenda.Expired`
- `referenda.FastTracked`
- `referenda.Cancelled`
- `referenda.Killed`
- `referenda.Enacted`
- `signed_voting.Voted`
- `signed_voting.VoteRemoved`
- `multi_collective.MemberAdded`
- `multi_collective.MemberRemoved`
- `multi_collective.MemberSwapped`
- `multi_collective.MembersSet`

## Implementation Map

- `runtime/src/governance/collectives.rs`: collective ids, sizes, term
  duration, and root-registration sync for `EconomicEligible`.
- `runtime/src/governance/tracks.rs`: track ids, thresholds, delays, and
  decision strategies.
- `runtime/src/governance/member_set.rs`: single and union collective voter
  sets with deduplication.
- `runtime/src/governance/term_management.rs`: Economic and Building
  rotation selection.
- `runtime/src/governance/ema_provider.rs`: Economic stake-value sample
  provider.
- `pallets/referenda`: generic track state machine and scheduler wrapping.
- `pallets/signed-voting`: per-account aye/nay voting with frozen voter-set
  snapshots.
- `pallets/multi-collective`: named collective membership and term
  rotation hooks.
