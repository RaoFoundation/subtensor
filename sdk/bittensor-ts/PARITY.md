---
title: "TypeScript SDK feature parity"
description: "Current source coverage, frontend and backend APIs, and validation boundaries."
---

# Feature coverage

The reference is the Rust client, Python SDK, and Python CLI in this checkout.
CLI interactions are exposed as application APIs; the package does not install
a command-line program. Numeric money uses exact rao and unit-aware `Balance`
values. Formatting, prompts, and terminal dashboards remain application concerns.

| Reference surface | TypeScript application API | Platforms |
| --- | --- | --- |
| 105 registered Python reads | `Client.read`, typed namespaces, pinned `Snapshot`, `Metagraph` | Node and browser |
| 86 semantic Python intents | Typed `buildIntent`, `intentBuilders`, `plan`, `execute`, tools and policies | Node and browser |
| Current chain metadata | 411 storage, 136 constants, 125 runtime APIs, 316 call descriptors; dynamic SCALE/runtime access | Node and browser |
| Transactions and composition | Transfers, stake and root baskets, claims, proxy, multisig, batch, sudo, registration, weights, identities, collateral, locks, leases, crowdloans, and subnet administration | Node and browser |
| Detached and external signing | Strict unsigned JSON import/export, extension signers, Vault adapters, metadata proofs and signature verification | Node and browser |
| Shielded transactions | Short mortal eras, paired nonces, separate carrier fee accounting, actual inner receipts | Node and browser |
| Wallet and secret storage | Filesystem wallets, keyfile import/export, encrypted Ethereum keystores, Ledger HID | Node; browser signers use injected extensions or explicit adapters |
| Extension bridge | Token-bound loopback WebSocket server and extension signer | Node server, browser extension |
| EVM workflows | Canonical 21 precompiles, address mapping, association, funding, stake, contract deployment and calls | Node and browser; filesystem artifacts in Node |
| Timelock and proof of work | Encryption/reveal helpers, weight commits, bounded registration orchestration | Node and browser; CPU/OpenCL miners in Node |
| Operator workflows | Upgrade inspection, asset verification and reproduction recipe, restartable Null pruning, transfer history, endpoint probes | Node and browser through explicit adapters |
| Local application state | Configuration, address/proxy/multisig books, subnet label caching | File, browser storage, memory, or custom stores |
| Diagnostics | 421 module errors, 18 semantic error codes, pool rejections, nested dispatch failures | Node and browser |

`scripts/check-native-parity.cjs` checks the Rust export surfaces, including
explicit mappings for Rust client operations implemented by TypeScript adapters.
`npm run check:catalogs` regenerates reference-derived catalogs in memory and
checks for drift. `parity-manifest.json` records upstream source hashes so future
changes require review, including runtime fee and claim-admission inputs; a
matching hash or name count alone is not behavioral
evidence. Refresh it with `npm run parity:record` after updating implementations
and their tests.

# Validation scope

The SDK suite covers intent preparation, trusted constructor policy, typed inputs,
reads and snapshots, large numeric values, wallets, signing, bridge sessions,
receipt handling, EVM encoding, upgrades, preferences and pruning. Real Chromium
tests load the actual WASM through a custom loader, the explicit browser entry,
and the default package import. Live tests use a locally built development node
and development keys, and are separate from the fixture tests.

Unit fixtures cover all registered intent planners; they do not establish live
execution of every operation. Ledger hardware, OpenCL hardware, hosted Vault
deployments, production upgrades and mainnet submissions are not exercised by the
ordinary test suite. Compressed runtime assets retain their exact bytes and hash;
upgrade inspection explicitly reports when embedded version extraction is skipped.

Low-level `IntentCall` construction preserves Rust compatibility. Use the semantic
workflow APIs for state-dependent admission, mandatory shielding and fee-payer
checks. Imported unsigned transactions and raw calls carry no semantic approval.
