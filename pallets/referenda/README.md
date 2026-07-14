# pallet-referenda

Track-based on-chain referenda. Proposals are filed against a track
that defines who may submit, who may vote, and how a tally is turned
into a decision. The pallet runs the state machine and dispatches the
governed call when approved; voting itself is delegated to a separate
backend (e.g. `pallet-signed-voting`) through the `Polls` trait.

The pallet only stores referendum status and a thin scheduler-cleanup
handle. Tallies, voter lists, and per-account vote records live in the
voting backend.

## Architecture

```
                  ┌──────────────────┐
                  │ pallet-referenda │   <─── this pallet
                  │                  │
                  │  submit, kill    │
                  │  advance         │
                  │  enact           │
                  └──┬────────────┬──┘
   on_poll_created   │            │ Polls
   on_poll_completed │            │  is_ongoing
                     ▼            │  voting_scheme_of
                ┌──────────────────┐  voter_set_of
                │ Voting backend   │  on_tally_updated
                │ (e.g. signed-    │
                │  voting)         │
                └──────────────────┘
```

Tracks come from a runtime-supplied `TracksInfo` impl: each track
declares its proposer set, voter set, voting scheme, and decision
strategy.

## Decision strategies

| Strategy | Decision | Outcome |
| -------- | -------- | ------- |
| `PassOrFail` | Approve / reject by deadline. | On approval the call is dispatched directly, or handed off to a child review referendum filed on an `Adjustable` track. On rejection or deadline elapse the referendum terminates. |
| `Adjustable` | Timing decision over an already-scheduled call. | Submit schedules the call at `submitted + initial_delay`. Voters can fast-track it sooner, cancel it, or shift the dispatch time via interpolation on net votes: net approval shrinks the delay toward zero, net rejection extends it toward the track's `max_delay` before the cancel threshold fires. The shape of that interpolation is set by `Config::AdjustmentCurve`. |

## Extrinsics

| Call | Origin | Effect |
| ---- | ------ | ------ |
| `submit`             | signed (must be in the track's proposer set) | Open a new referendum carrying `call`. |
| `kill`               | `T::KillOrigin` | Privileged termination of an undispatched referendum; cancels pending scheduler entries and concludes as `Killed`. |
| `advance_referendum` | root | Drive the state machine for one referendum. Invoked by the alarm; available as a manual recovery path. |
| `enact`              | root | Dispatch the inner call and mark the referendum as enacted. Invoked by the scheduler at the configured dispatch time; no-op on terminal-no-dispatch statuses. |

## State machine

`PassOrFail`:

```text
                       submit
                         │
                         ▼
    vote re-arms     ┌───────┐   kill
    alarm         ┌─►│Ongoing│─────────────────────► Killed
                  │  └───┬───┘
                  │      │ alarm fires:
                  │      ├─ approve (Execute) ─► Approved ─► enact ─► Enacted
                  │      ├─ approve (Review)  ─► Delegated
                  │      ├─ reject_threshold  ─► Rejected
                  │      ├─ deadline reached  ─► Expired
                  │      └─ no decision yet   ─► re-arm alarm at deadline
                  └──────┘
```

`Adjustable`:

```text
                       submit
                         │
                         │ schedule enact at submitted + initial_delay
                         ▼
    vote re-arms     ┌───────┐   kill
    alarm         ┌─►│Ongoing│─────────────────────► Killed
                  │  └───┬───┘
                  │      ├─ enact fires (natural)   ─► Enacted
                  │      │ alarm fires:
                  │      ├─ fast_track_threshold    ─► FastTracked ─► enact ─► Enacted
                  │      ├─ cancel_threshold        ─► Cancelled
                  │      └─ otherwise               ─► reschedule enact (earlier on
                  └──────┘                             net approval, later on net rejection)
```

`kill` is also accepted from `Approved` and `FastTracked` until
`enact` dispatches: the wrapper task is cancelled and the inner call
never runs.

## Design notes

### Dispatch wrapping

Approval and adjustable submission both schedule a wrapper call
`Pallet::enact(index, call)` rather than the governed call directly.
The wrapper marks the referendum as enacted in the same call that
dispatches the inner call, so dispatch and the `Enacted` status
transition are atomic. A stale wrapper that fires after a failed
cancel cannot run the call twice: `enact` no-ops on terminal-no-
dispatch statuses.

### Tally hook deferral

`Polls::on_tally_updated` only stores the new tally and arms an alarm
at `now + 1`. All decision logic runs from the alarm via
`advance_referendum`, which keeps the tally hook free of re-entrancy
with the voting backend.

### Track-config snapshotting

`submit` snapshots the track's decision strategy into the referendum.
State-machine evaluation reads the snapshot, so a runtime upgrade
that changes thresholds, swaps strategies, or removes a track only
affects new submissions; live referenda continue to resolve under the
rules they started with.

Voter-set membership stays dynamic: percentages reflect current
membership of the underlying collective.

### Per-proposer quota

`MaxActivePerProposer` bounds the number of simultaneously-active
referenda one account can hold. This caps the blast radius of a
compromised proposer key when many proposers compete for the global
`MaxQueued` slots.

### Adjustment curve

The mapping from net-vote progress to delay fraction is supplied by
the runtime as `Config::AdjustmentCurve`. The pallet calls
`AdjustmentCurve::apply(progress)` on each side, where `progress` is
the position of the net vote between zero and the side-specific
threshold (`fast_track_threshold` for net approval,
`cancel_threshold` for net rejection). The same curve is applied to
both sides for symmetry. The choice is runtime-global and not
snapshotted: a runtime upgrade that swaps the impl takes effect for
all in-flight referenda on the next state-machine evaluation.

## Integrity check

`integrity_test` runs at runtime construction and panics on a
misconfigured track table:

- Duplicate track ids.
- `ApprovalAction::Review { track }` referencing an unknown track or
  one whose strategy is not `Adjustable`.
- `PassOrFail` with zero `decision_period`, `approve_threshold`, or
  `reject_threshold`; or with
  `approve_threshold + reject_threshold ≤ 100%` so the reject branch
  could be masked by an approval that fires first on the same tally
  split.
- `Adjustable` with zero `initial_delay`, `fast_track_threshold`, or
  `cancel_threshold`; with `max_delay < initial_delay` (so net
  rejection cannot extend the delay); or with
  `fast_track_threshold + cancel_threshold ≤ 100%` so the cancel
  branch could be masked by a fast-track that fires first on the same
  tally split.

## Migrations

Pinned at `StorageVersion::new(0)` to satisfy try-runtime CLI; the
project tracks migration runs through a per-pallet `HasMigrationRun`
storage map (see `pallet-crowdloan`), not via FRAME's `StorageVersion`
bump.

## Configuration

```rust
parameter_types! {
    pub const MaxQueued: u32 = 20;
    pub const MaxActivePerProposer: u32 = 5;
}

impl pallet_referenda::Config for Runtime {
    type RuntimeCall          = RuntimeCall;
    type Scheduler            = Scheduler;
    type Preimages            = Preimage;
    type MaxQueued            = MaxQueued;
    type MaxActivePerProposer = MaxActivePerProposer;
    type KillOrigin           = EnsureRoot<AccountId>;
    type Tracks               = tracks::Tracks;
    type AdjustmentCurve      = tracks::EaseOutAdjustmentCurve;
    type BlockNumberProvider  = System;
    type OnPollCreated        = SignedVoting;
    type OnPollCompleted      = SignedVoting;
    type WeightInfo           = pallet_referenda::weights::SubstrateWeight<Runtime>;
}
```

## License

Apache-2.0.
