# Wave 2 gate

- metadata fingerprint: OK (`sha256:7cc9b85a0a732d1976b5558cea02ea0c8ad696bf333099dd2dbfc8ad61e18f35`)
- giant test splits landed: weights, staking, migration, locks, children, coinbase, epoch, networks, swap_hotkey_with_subnet, math
- docs-only frozen surfaces: storage, dispatches, events, errors, migrations
- source areas: coinbase, epoch, staking, subnets, swap (identity), rpc_info, utils, guards, extensions, benchmarks, rpc
- per-shard tests: passed in shard worktrees before merge
- full workspace nextest / try-runtime: deferred until disk allows; re-run before PR to main
