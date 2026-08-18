# Reviewed precompile exceptions

This file records narrow, human-reviewed exceptions to the general state
coverage and bounded-work rules. Apply an exception only to the exact function
and invariant described here. Do not infer that a similar storage shape or
collection is also exempt.

When reviewing one of these functions, verify that its supporting invariant
still holds. If the runtime changes that invariant, stop treating the function
as an exception and reassess its interface, compatibility, cost, and tests.

## `getColdkeyLock(bytes32,uint256)`

`getColdkeyLock` returns the one individual lock for a `(coldkey, netuid)`.
Although `Lock` includes the target hotkey in its storage key and the
implementation locates the row with `iter_prefix(...).next()`, multiple lock
rows are not valid state for that pair:

- `do_lock_stake` creates the lock when none exists and rejects a different
  target hotkey with `LockHotkeyMismatch` when one already exists;
- `move_lock` moves the existing lock to a new target instead of creating a
  second lock; and
- the lock is subnet-wide for the coldkey, while the hotkey identifies its
  current target.

The precompile therefore reflects the runtime design accurately and does not
need a paginated or hotkey-keyed replacement. Keep tests proving that a second
target is rejected and that moving a lock leaves exactly one row.

This exception becomes invalid if any lock creation, transfer, migration, or
repair path permits multiple `Lock` rows for the same `(coldkey, netuid)`.

## `getSumAlphaPrice()`

`getSumAlphaPrice` may scan every subnet. Subnets are a protocol-limited,
scarce resource, and the function's meaningful result is the aggregate over
the complete set. A cursor would change that meaning and move composition to
the caller.

Keep the complete scan, charge for all permitted subnet reads, and test it at
the configured subnet limit. This exception does not apply to collections
whose size grows with accounts, neurons, stakes, commitments, or other
user-created records.

## `getStakingHotkeys(bytes32,uint64,uint16)`

`getStakingHotkeys` reads the existing `StakingHotkeys` vector for one coldkey and returns at most
64 entries from the requested zero-based offset. `StakingHotkeys` stores the complete vector as one
SCALE-encoded value, so each call performs one database read and decodes that value before taking
the requested in-memory slice.

This full-vector decode is accepted because it avoids additional runtime indexes, write-path
maintenance, and a state migration. Keep the output limit and charge the single database read. Do
not describe the call as having CPU, memory, or proof-size work bounded by the page limit.
