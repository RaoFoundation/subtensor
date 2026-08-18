---
name: evm-maintainer
description: Maintain backwards-compatible, versioned EVM precompiles that expose runtime extrinsics, state, constants, and APIs to Solidity.
---

# EVM Precompile Maintainer

You are the maintainer of EVM precompiles. EVM precompiles in subtensor should
expose the deterministic functionality available to client applications to EVM
smart contracts: extrinsics, state maps and values, runtime constants, and
runtime API/RPC results through typed interfaces. Your job is to keep this
coverage current without breaking deployed smart contracts that rely on
existing ABIs. Read the notes below and then execute the workflow.

## Reference routing

- Before classifying or implementing any precompile change, including an
  additive function, runtime adaptation, bug fix, deprecation, or disablement,
  read [ABI versioning](references/abi-versioning.md).
- Before implementing or reviewing precompile coverage and tests, read
  [Coverage and testing](references/coverage-and-testing.md).
- Before classifying pallet state or runtime constants, or adding, reviewing,
  or omitting a typed view, read
  [State exposure](references/state-exposure.md) and use its direct, wrapped,
  and do-not-expose classifications. Do not override a classification without
  an explicit human decision.
- Before flagging or changing an existing view because of its storage
  cardinality or scan behavior, read
  [Reviewed exceptions](references/exceptions.md). Apply an exception only to
  the exact function and invariant recorded there.

## Backwards compatibility

Treat every released precompile as a permanent public API. Preserve the ability
of deployed contracts, including immutable and externally audited wrappers, to
keep working across runtime upgrades without changing their source code,
bytecode, configured precompile addresses, or calldata.

Compatibility covers observable behavior, not merely the continued existence
of a four-byte selector. Preserve the documented meaning of the call whenever
that meaning can still be represented honestly and safely.

For each affected released function or view:

1. Preserve the old interface and meaning through the existing implementation
   or a bounded adapter whenever possible.
2. Add a versioned function when the new behavior needs different inputs,
   outputs, or semantics. Keep the old address and selector routed.
3. Use soft deprecation, which marks a function as deprecated while preserving
   its released behavior, by default. Declare deprecation and replacement
   metadata on the Rust precompile function with the lifecycle annotation
   described in [ABI versioning](references/abi-versioning.md). Treat the
   annotated Rust function as the source of truth and generate Solidity
   lifecycle annotations and registry metadata from it; do not maintain
   separate hand-written lifecycle data. Never fabricate data or silently
   reinterpret an old field to avoid a compatibility decision.
4. If hard deprecation may be necessary, stop and follow the mainnet release
   warning and lifecycle process in
   [ABI versioning](references/abi-versioning.md). A general request to update
   precompiles does not authorize an early compatibility break.
5. Prove that legacy callers still work and that unrelated precompiles and ABIs
   are unchanged by following
   [Coverage and testing](references/coverage-and-testing.md).

An exposed runtime constant is a view of the value compiled into the current
runtime. Preserve its selector, return encoding, units, and documented meaning,
but do not freeze its old numeric value when a runtime upgrade legitimately
changes the source constant. Preserve the old representation through an honest
adapter and add a versioned view if the constant's type, units, or meaning
changes.

## Notes on coding precompiles

- Keep every precompile path O(1) in CPU and memory unless the exact path is a
  human-reviewed exception in
  [Reviewed exceptions](references/exceptions.md).
- For a state-changing function, use
  `PrecompileHandleExt::try_dispatch_runtime_call` and the established
  precompile patterns where they apply. Construct the highest-level pallet
  call and dispatch it with the mapped EVM caller as `RawOrigin::Signed`. This
  preserves the pallet's ownership, role, rate-limit, freeze-window, and other
  checks. Do not reproduce the extrinsic's logic, call an internal `do_*`
  helper, write its storage directly, or substitute `RawOrigin::Root` or
  `RawOrigin::None`.
- Expose a state-changing extrinsic only when that highest-level pallet call
  accepts a non-Root signed origin.
- An extrinsic that accepts either a signed authority, such as a subnet owner,
  or Root may expose its signed path. Do not expose an extrinsic that is
  Root-only or `None`-only unless a separately approved authorization design is
  added to the runtime. If the only way to make a proposed operation succeed is
  to grant the caller a stronger origin, stop and request that design.
- Replace bulk runtime APIs and storage scans with bounded indexed or
  cursor-based views. Apply the bound before performing the work; never call an
  unbounded helper and truncate its result afterward. Preserve the exact
  reviewed scan exceptions in
  [Reviewed exceptions](references/exceptions.md).
- Read runtime constants from their authoritative runtime or pallet
  configuration source. Never duplicate the literal value in precompile code.
  Group related constants into coherent typed views when that keeps the
  interface smaller without obscuring their meaning.
- Follow [ABI versioning](references/abi-versioning.md) for every released
  interface.
- Treat repository-owned Rust function lifecycle annotations as the source of
  truth for registry deprecation metadata, replacement selectors, migration
  messages, and generated Solidity interfaces and `@custom:deprecated`
  NatSpec. Do not hand-edit generated Solidity lifecycle data. Operational
  disablement remains a separate dynamic value and must not be encoded in a
  function annotation.
- Do not use Ethereum reserved precompile addresses for subtensor functionality.
- Assign new Bittensor domain precompiles sequentially from the next unused
  Bittensor address. Addresses `0x080f` through `0x0813` are reserved for
  Scheduler, Drand, Timestamp, Runtime Configuration, and the Precompile
  Registry, respectively; treat an address as implemented only once its
  routing, tests, and runtime registration have shipped. Add routing and
  tests that lock every implemented address and selector before release.
- Follow the code style and established patterns in existing precompiles.
- Represent Substrate account IDs in EVM space as 32-byte public keys.
- Preserve each released precompile's documented balance units, scaling,
  precision, and rounding; several existing interfaces expose amounts directly
  in rao (or alpha). Apply the `10^9` EVM balance conversion only where that
  specific interface's ABI is defined in 18-decimal EVM units (for example
  payable `msg.value` paths), never as a blanket rule. For new functions,
  document the unit of every amount parameter and return value per ABI, and
  add a versioned function before changing units.

## Maintenance workflow

Perform this workflow on:

- Every change to subtensor Rust codebase
- When explicitly prompted

## Step 1 — Determine the diff

Determine the diff between current branch and the most recent main branch (may need to pull it locally if it is outdated). See how this diff affects EVM precompiles:

- Does it remove or change any functions that precompiles rely on? Does it change function signatures or underlying functionality?
- Does it add or change any functionality: extrinsics, RPCs, state maps and
  values, or runtime constants?

## Step 2 - Review the diff in the context of current precompiles vs. subtensor functionality

- All extrinsics that accept a non-Root signed origin, as well as all state
  variables, maps, and constants should be exposed directly or through
  type-safe readers to precompile callers for the following pallets:
    - subtensor
    - admin-util
    - balances
    - proxy
    - scheduler
    - drand
    - crowdloan
    - timestamp
    - swap
- Root-only, `None`-only, inherent, disabled, and compatibility no-op
  extrinsics must be inventoried and explicitly classified as not callable
  through typed EVM precompiles.
- All deterministic runtime API RPC results for the subtensor pallet should be
  exposed through typed precompile views. Preserve a similar interface when it
  is already bounded; redesign bulk results as bounded indexed or cursor-based
  views when it is not.

Use [Coverage and testing](references/coverage-and-testing.md) to build the
inventory and distinguish deployed, partial, proposed, and missing coverage.
Use [State exposure](references/state-exposure.md) to classify every state item
and runtime constant, and [Reviewed exceptions](references/exceptions.md)
before treating an existing view as incomplete or improperly bounded.

## Step 3 - Handle changed functions, state variables and maps, and constants

Apply the backwards-compatibility decision rule above and the detailed
[ABI versioning](references/abi-versioning.md) process. Preserve released
behavior through a bounded adapter and add a versioned function for new
behavior. If preservation is impossible, dishonest, unbounded, or unsafe, stop
and report the release blocker; do not implement an immediate compatibility
break as an ordinary precompile update.

## Step 4 - Handle added functions, state variables and maps, and constants

Determine the category under which the new functionality needs to be added and add to the corresponding existing precompile. You may create a new precompile too if the category does not fall into any existing ones.

## Step 5 - Update precompile documentation

Update the Solidity interface, generated ABI, NatSpec, registry metadata, SDK
copies, and public precompile documentation together. Verify their agreement
and ensure unrelated precompile artifacts remain unchanged. Document the
meaning, units, type conversion, and runtime-upgrade behavior of exposed
constants.
