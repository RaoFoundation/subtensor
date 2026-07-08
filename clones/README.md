# Clone Test Agent

This project is a small harness for using Codex to create and run JavaScript regression tests against a local Subtensor mainnet clone.

The goal is to make advanced local blockchain testing repeatable: build or reuse a patched mainnet clone, start a local validator node, optionally upgrade it to the runtime in `subtensor-reference/`, then run focused JS tests through `@polkadot/api`.

## What Happens

The normal workflow is:

1. `scripts/clone-mainnet.sh` verifies or creates a patched mainnet clone chainspec under `../../clones`.
2. `scripts/start-local-clone.sh` starts a local node from that chainspec at `ws://127.0.0.1:9944`.
3. `js-tests/tests/clone-smoke-test.js` confirms the websocket endpoint is usable.
4. `js-tests/scripts/update-runtime-with-alice.js` submits a sudo runtime upgrade from Alice using the wasm in `subtensor-reference/`.
5. Feature tests under `js-tests/tests/` exercise runtime behavior against the upgraded local clone.
6. `scripts/stop-local-clone.sh` stops the node so no background process is left running.

The current focused regression test is `js-tests/tests/test-balancer-operation.js`. It verifies that balancer storage is initialized, initialized balancer weights stay in the `0.45-0.55` range with at least one subnet not exactly `0.5`, balance transfers work, and epoch activity updates subnet reserves within the configured wait window.

## How To Use

### Manual Build Process

This repository expects a Subtensor checkout in `../../subtensor`, plus the read-only symlink `subtensor-reference/` tree in this workspace for runtime reference and wasm upgrades, for example:

```text
development/
  subtensor/
  agents/codex-tester/
```

Build the node binary manually in the `subtensor/` checkout:

```sh
cd ../../subtensor
cargo build --release --workspace --all-targets
```

The runtime upgrade script expects this file:

```text
subtensor-reference/target/release/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm
```

### Prompt Examples

Use prompts like these with Codex:

```text
Write a JS test that verifies balance transfers after a runtime upgrade.
```

```text
Write a test that verifies balancer initialization, non-default balancer weights, balance transfers, and reserve updates after an epoch.
```

```text
Inspect the failed JS test output and node logs, explain the likely runtime issue, and make the smallest test change needed to capture it.
```

## Notes

- `subtensor-reference/` is read-only reference material for tests.
- Local clone data and chainspec files live outside this repo under `../../clones`.
- Keep new JS tests focused and descriptive; avoid broad refactors during runtime investigations.
