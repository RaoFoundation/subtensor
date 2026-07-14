# pallet-multi-collective

Membership storage for one or more named collectives, keyed by a
runtime-defined `CollectiveId`. Each collective is configured by a
`CollectivesInfo` impl: name, min/max members, optional term duration.

The pallet only stores membership. Voting, proposing, and tallying are
left to the consumer (e.g. `pallet-referenda` + `pallet-signed-voting`),
which read members through the `CollectiveInspect` trait.

## Concepts

| Type | Provided by | Purpose |
| ---- | ----------- | ------- |
| `CollectiveId` | runtime | Enum naming each collective. |
| `CollectivesInfo` | runtime | Returns the static config for each id (name, bounds, term). |
| `CollectiveInfo` | this crate | `{ name, min_members, max_members, term_duration }`. |
| `Members<_>` | this crate | `BoundedVec<AccountId, MaxMembers>` per id, sorted by `AccountId`. |

## Extrinsics

| Call | Origin | Effect |
| ---- | ------ | ------ |
| `add_member`    | `T::AddOrigin`    | Insert one member. Fails on `AlreadyMember`, `TooManyMembers`, `CollectiveNotFound`. |
| `remove_member` | `T::RemoveOrigin` | Remove one member. Fails on `NotMember`, `TooFewMembers`, `CollectiveNotFound`. |
| `swap_member`   | `T::SwapOrigin`   | Atomic remove + insert. Count is preserved, so the per-collective `min_members` / `max_members` bounds are not re-checked; works at either boundary. |
| `set_members`   | `T::SetOrigin`    | Replace the full list. Sorts the input and rejects `DuplicateAccounts` if any duplicates are present (the input is not silently deduplicated). |
| `force_rotate`  | `T::RotateOrigin` | Trigger `OnNewTerm` for a rotating collective on demand. |

Every mutation fires `T::OnMembersChanged` with the incoming and
outgoing accounts so downstream pallets can react (e.g. clean up
votes). The Subtensor runtime currently wires this to `()`: active
polls snapshot the voter set at creation, so member changes cannot
retroactively invalidate votes, and no cleanup is needed.

## Rotation

A collective whose `CollectiveInfo::term_duration` is `Some(d)` rotates
every `d` blocks: `on_initialize` calls `T::OnNewTerm::on_new_term(id)`
when `block_number % d == 0`. The runtime-supplied handler typically
recomputes membership from on-chain data and writes it back through
`set_members`.

`force_rotate` runs the same hook on demand. Used to bootstrap the
first term (the natural cadence only fires after the first boundary,
which can be days or months in) and as a privileged override during
incidents. Calls against a collective with `term_duration: None` are
rejected with `CollectiveDoesNotRotate`.

Curated collectives (no term duration) are managed directly via the
membership extrinsics.

## Integrity check

`integrity_test` runs at runtime construction and panics on a
misconfigured `CollectivesInfo`:

- `min_members > T::MaxMembers` (collective can't reach its min)
- `max_members > T::MaxMembers` (storage can't hold the declared max)
- `min_members > max_members` (collective is unreachable)
- `term_duration: Some(0)` (silently disables rotation; use `None` to opt out)

## Migrations

Pinned at `StorageVersion::new(0)` to satisfy try-runtime CLI; the
project tracks migration runs through a per-pallet `HasMigrationRun`
storage map (see `pallet-crowdloan`), not via FRAME's `StorageVersion`
bump. Add a `migrations` module and an `on_runtime_upgrade` hook on
the next breaking change to `Members<_>` or any future persisted state.

## Configuration

```rust
parameter_types! {
    pub const MaxMembers: u32 = 20;
}

impl pallet_multi_collective::Config for Runtime {
    type CollectiveId    = CollectiveId;
    type Collectives     = Collectives;
    type AddOrigin       = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type RemoveOrigin    = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type SwapOrigin      = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type SetOrigin       = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type RotateOrigin    = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type OnMembersChanged = ();
    type OnNewTerm       = TermManagement;
    type MaxMembers      = MaxMembers;
    type WeightInfo      = pallet_multi_collective::weights::SubstrateWeight<Runtime>;
}
```

`T::MaxMembers` bounds storage; per-collective `max_members` from
`CollectivesInfo` may be smaller but never larger (enforced by
`integrity_test`).

## License

Apache-2.0.
