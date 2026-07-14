# pallet-signed-voting

A per-account voting backend for a poll producer (typically
`pallet-referenda`). Each call records a single voter's aye or nay; the
tally is pushed back to the producer in real time so it can re-evaluate
thresholds and conclude polls without scheduler nudges.

The pallet is generic over the producer. It does not know what is being
voted on, only that polls have an index, a voting scheme, and an
eligibility roster.

## Architecture

```
                   ┌──────────────────┐
                   │ Producer pallet  │   (e.g. pallet-referenda)
                   │  is_ongoing      │
                   │  voting_scheme   │   <─── implements Polls
                   │  voter_set_of    │
                   │  on_tally_updated│
                   └──┬────────────┬──┘
       on_poll_created│            │ on_tally_updated
       on_poll_completed           │
                      ▼            │
                  ┌──────────────────┐
                  │ pallet-signed    │
                  │   -voting        │   <─── this pallet
                  │                  │
                  │ vote(poll, aye)  │
                  │ remove_vote(...) │
                  └──────────────────┘
```

The producer asks the pallet's hooks (`OnPollCreated`,
`OnPollCompleted`) when polls open and close; the pallet asks the
producer's `Polls` trait for the voter set and pushes tally updates
back through it.

## Lifecycle

| Event              | What the pallet does                                     |
| ------------------ | -------------------------------------------------------- |
| `on_poll_created`  | Snapshot the voter set into `VoterSetOf` (sorted and deduplicated), seed `TallyOf` with `total = snapshot.len()`. Skipped for polls whose scheme does not match `T::Scheme`, or if a tally already exists for the index. |
| `vote`             | Verify eligibility against the snapshot via `binary_search`, update `VotingFor` and `TallyOf`, push the new tally to the producer. |
| `remove_vote`      | Roll back the caller's `VotingFor` entry, decrement `TallyOf`, push the new tally to the producer. |
| `on_poll_completed`| Remove `TallyOf` and `VoterSetOf` synchronously; enqueue the poll on `PendingCleanup` for lazy `VotingFor` cleanup. No-op if no tally exists for the index. |
| `on_idle`          | Drain `PendingCleanup` head in `CleanupChunkSize` chunks until the queue is empty or the idle budget is exhausted. |

## Design notes

### Frozen voter-set snapshot

The eligibility roster is whatever `Polls::voter_set_of` returns at
poll creation. After that the underlying collective can rotate freely
without affecting active polls:

- Removed members keep the voting rights they had when the poll
  opened.
- New members cannot vote on polls created before they joined.
- The denominator (`SignedVoteTally::total`) stays fixed so thresholds
  cannot drift mid-poll.

The snapshot is sorted once at creation so eligibility checks are
`O(log n)` per vote.

### Lazy `VotingFor` cleanup

`VotingFor` grows linearly with `voters × active polls`. Clearing the
prefix synchronously in `on_poll_completed` would put unbounded work
on the producer's call. Instead, completion enqueues the poll on
`PendingCleanup` and `on_idle` reclaims the storage in
`CleanupChunkSize`-sized chunks. Cleanup of one poll may span multiple
idle blocks; the resume cursor returned by `clear_prefix` is persisted
between passes so already-removed entries are not re-iterated.

If `on_idle` cannot keep up and the queue overflows
`MaxPendingCleanup`, the pallet emits `CleanupQueueFull`, logs an
error, and leaks the overflowing poll's `VotingFor` entries.
Correctness is preserved (the entries are unread once `TallyOf` is
gone) but the storage is only reclaimable via a follow-up migration.

Sizing `MaxPendingCleanup` is a throughput question, not just a
simultaneous-active-poll question: drain rate (`on_idle` budget,
`CleanupChunkSize`) must keep up with completion rate over time.
Setting it to a small multiple of the producer's `MaxQueued` gives
headroom for bursts where many polls complete in close succession
while `on_idle` is starved by full blocks. The pallet's
`integrity_test` rejects a zero value for `MaxPendingCleanup`,
`CleanupChunkSize`, or `MaxVoterSetSize` at boot.

## Configuration

```rust
parameter_types! {
    pub const Scheme:               VotingScheme = VotingScheme::Signed;
    pub const MaxVoterSetSize:      u32 = 64;   // ≥ widest track's voter set
    pub const MaxPendingCleanup:    u32 = 40;   // ≥ producer's MaxQueued, with headroom for bursts
    pub const CleanupChunkSize:     u32 = 16;   // entries per idle drain step
    pub const CleanupCursorMaxLen:  u32 = 128;  // bound for clear_prefix cursor
}

impl pallet_signed_voting::Config for Runtime {
    type Scheme              = Scheme;
    type Polls               = Referenda;
    type MaxVoterSetSize     = MaxVoterSetSize;
    type MaxPendingCleanup   = MaxPendingCleanup;
    type CleanupChunkSize    = CleanupChunkSize;
    type CleanupCursorMaxLen = CleanupCursorMaxLen;
    type WeightInfo          = pallet_signed_voting::weights::SubstrateWeight<Runtime>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper     = SignedVotingBenchmarkHelper;
}
```

## License

Apache-2.0.
