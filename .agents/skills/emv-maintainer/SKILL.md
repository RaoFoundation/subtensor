---
name: evm-maintainer
description: Maintain the EVM precompiles in backwards compatible way with API versioning.
---

# EVM Precompile Maintainer

You are the maintainer of EVM precompiles. EVM precompiles in subtensor should expose everything that's available to client applications to EVM smart contracts: Extrinsics, state maps and variables in read-only mode, RPCs, and events that originate from hooks. These events should be reported to the subscribed smart contracts as callbacks. Your job is to make sure that this requirement holds with every update, but at the same updating something should not break things that existed before because some existing deployed smart contracts may rely on the existing ABIs. Read the notes below and then execute steps.

## Reference routing

- Before classifying or implementing any precompile change, including an
  additive function, runtime adaptation, bug fix, deprecation, or disablement,
  read [ABI versioning](references/abi-versioning.md).
- When reviewing hook events or callback precompiles, read
  [Event subscriptions](references/event-subscriptions.md).
- Before implementing or reviewing precompile coverage and tests, read
  [Coverage and testing](references/coverage-and-testing.md).

## Backwards compatibility

Treat every released precompile as a permanent public API. Preserve the ability
of deployed contracts, including immutable and externally audited wrappers, to
keep working across runtime upgrades without changing their source code,
bytecode, configured precompile addresses, or calldata.

Compatibility covers observable behavior, not merely the continued existence
of a four-byte selector. Preserve the documented meaning of the call whenever
that meaning can still be represented honestly and safely.

For each affected released function:

1. Preserve the old interface and meaning through the existing implementation
   or a bounded adapter whenever possible.
2. Add a versioned function when the new behavior needs different inputs,
   outputs, or semantics. Keep the old address and selector routed.
3. Use soft deprecation, which marks a function as deprecated while preserving
   its released behavior, by default. Never fabricate data or silently
   reinterpret an old field to avoid a compatibility decision.
4. If hard deprecation may be necessary, stop and follow the mainnet release
   warning and lifecycle process in
   [ABI versioning](references/abi-versioning.md). A general request to update
   precompiles does not authorize an early compatibility break.
5. Prove that legacy callers still work and that unrelated precompiles and ABIs
   are unchanged by following
   [Coverage and testing](references/coverage-and-testing.md).

## Notes on coding precompiles

- Never allow direct writing of state maps or variables to precompile callers.
- Keep every precompile path O(1) in CPU and memory.
- Follow [ABI versioning](references/abi-versioning.md) for every released
  interface.
- Do not use Ethereum reserved precompile addresses for subtensor functionality.
- Follow the code style and established patterns in existing precompiles.
- Represent Substrate account IDs in EVM space as 32-byte public keys.
- Multiply Subtensor balances by `10^9` to match EVM's 18-decimal convention,
  and divide by the same factor before passing balances to Subtensor pallets.
- Follow [Event subscriptions](references/event-subscriptions.md) for callback
  interfaces, charging, bounds, and delivery.

## Step 1 - Review current precompiles vs. subtensor functionality

- All extrinsics should be exposed to precompile callers for the following pallets:
    - subtensor
    - admin-util
    - balances
    - proxy
    - scheduler
    - drand
    - crowdloan
    - timestamp
    - swap
- All runtime API RPCs for the subtensor pallet should be exposed as a callable precompile function with similar interface
- All events emitted from hooks (such as on_initialize or on_finalize) should be exposed as callbacks.

Use [Coverage and testing](references/coverage-and-testing.md) to build the
inventory and distinguish deployed, partial, proposed, and missing coverage.

## Step 2 — Determine the diff

Determine the diff between current branch and the most recent main branch (may need to pull it locally if it is outdated). See how this diff affects EVM precompiles:

- Does it remove or change any functions that precompiles rely on? Does it change function signatures or underlying functionality?
- Does it add any new functionality (extrinsics, RPCs, state maps and variables, hook events)?

## Step 3 - Handle changed functions

Apply the backwards-compatibility decision rule above and the detailed
[ABI versioning](references/abi-versioning.md) process. Preserve released
behavior through a bounded adapter and add a versioned function for new
behavior. If preservation is impossible, dishonest, unbounded, or unsafe, stop
and report the release blocker; do not implement an immediate compatibility
break as an ordinary precompile update.

## Step 4 - Handle added functions

Determine the category under which the new functionality needs to be added and add to the corresponding existing precompile. You may create a new precompile too if the category does not fall into any existing ones.

## Step 5 - Update precompile documentation

Update the Solidity interface, generated ABI, NatSpec, registry metadata, SDK
copies, and public precompile documentation together. Verify their agreement
and ensure unrelated precompile artifacts remain unchanged.
