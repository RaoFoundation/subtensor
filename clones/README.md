# Mainnet clone regression harness

TypeScript regression tests (run via `tsx`) against a **local clone of mainnet state**, sudo-upgraded
to the runtime built from this monorepo. CI runs the smoke test plus
`test:clone-regressions` in `check-clone-upgrade.yml` after every PR runtime
upgrade.

## What happens

1. `scripts/clone-mainnet.sh` creates or refreshes a patched mainnet clone
   chainspec under `clones/` (gitignored).
2. `scripts/start-local-clone.sh` starts a local node at `ws://127.0.0.1:9944`.
3. `js-tests/tests/clone-smoke-test.ts` confirms the websocket endpoint is usable.
4. `js-tests/scripts/update-runtime-with-alice.ts` sudo-upgrades the clone using
   the wasm from `target/release/wbuild/node-subtensor-runtime/` (built from the
   monorepo root).
5. `npm run test:clone-regressions` runs focused regressions against the
   upgraded clone.
6. `scripts/stop-local-clone.sh` stops the node.

Full contributor walkthrough:
[docs/internals/mainnet-clone.mdx](../docs/internals/mainnet-clone.mdx).

## Local usage

From the monorepo root:

```bash
cargo build --release -p node-subtensor
./clones/scripts/clone-mainnet.sh
./clones/scripts/start-local-clone.sh   # leave running; use another terminal

cd clones/js-tests
npm ci
npm run runtime:update:alice
npm test                              # smoke
npm run test:clone-regressions        # full local clone suite

# from repo root when done:
./clones/scripts/stop-local-clone.sh
```

The runtime upgrade script reads wasm from:

```text
target/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm
```

Override with `RUNTIME_WASM_PATH` if needed.

## Test scripts

| Script | Purpose |
|--------|---------|
| `npm test` | Connectivity smoke test |
| `npm run test:clone-regressions` | Curated regressions for the upgraded local clone (CI) |
| `npm run test:<name>` | Individual tests; see `package.json` for the full list |

Tests defaulting to `ws://127.0.0.1:9944` are clone-local. Scripts prefixed
with `testnet-` or `test:balancer-devnet-*` target live networks and are
manual-only.

## Notes

- Clone data and chainspec files live under `clones/` (gitignored).
- Keep new tests focused; prefer shared helpers in `js-tests/lib/` over
  copy-pasting chain plumbing.
- Tests are TypeScript executed with `tsx`; `npm run typecheck` runs
  `tsc --noEmit` over the suite.
