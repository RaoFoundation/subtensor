# Precompile coverage and testing

## Contents

- [Define the coverage scope](#define-the-coverage-scope)
- [Build a coverage inventory](#build-a-coverage-inventory)
- [Cover extrinsics](#cover-extrinsics)
- [Cover state with typed views](#cover-state-with-typed-views)
- [Cover runtime APIs and public RPCs](#cover-runtime-apis-and-public-rpcs)
- [Add regression tests first](#add-regression-tests-first)
- [Test observable behavior](#test-observable-behavior)
- [Validate ABIs and routing](#validate-abis-and-routing)
- [Validate cost and bounds](#validate-cost-and-bounds)
- [Run repository checks](#run-repository-checks)
- [Report the result](#report-the-result)

## Define the coverage scope

Take the authoritative pallet and API scope from `SKILL.md`. Do not silently
expand or narrow it based on an older document.

For each in-scope pallet, inspect:

- every dispatchable extrinsic;
- every public state map and value;
- every publicly facing runtime API and RPC;
- changes to types, guards, authorization, units, and error behavior used by
  existing precompiles.

Precompile coverage means that Solidity contracts receive a typed equivalent
of the authorized deterministic client-facing functionality. It does not mean
exposing raw pallet storage, SCALE bytes, or Rust types.

Distinguish deployed coverage from proposed coverage. Do not describe a
documented proposal, unassigned address, or Rust stub as callable.

## Build a coverage inventory

Create or update a working matrix with one row per source item:

| Source | Kind | Public functionality | Precompile domain | Function | Status | Evidence |
|---|---|---|---|---|---|---|
| Pallet and item | Extrinsic, state, runtime API, or RPC | Meaning exposed to clients | Existing or proposed address/domain | Canonical signature | Covered, partial, missing, or excluded | Rust, Solidity, ABI, and test paths |

For every partial, missing, or excluded row, state the exact reason. Do not
equate a similarly named function with coverage; compare parameters, returned
information, authorization, semantics, and failure behavior.

Use the matrix to find both directions of drift:

- runtime functionality with no typed EVM path; and
- precompile behavior whose runtime dependency changed or disappeared.

Group additions by meaning under as few coherent contracts as reasonably
possible. Do not mirror pallet boundaries mechanically and do not create one
precompile per storage item.

## Cover extrinsics

Expose each extrinsic that accepts a non-Root signed origin through a typed
state-changing function unless an explicit scope decision excludes it. Calls
that accept either Root or a signed authority may expose only the signed path.
Classify Root-only and `None`-only calls as not EVM-callable; the existence of a
runtime extrinsic does not authorize a precompile to manufacture its origin.

Preserve:

- dispatched origin and caller mapping;
- authorization and proxy behavior;
- payable versus nonpayable behavior;
- attached-value conversion and handling;
- input validation and bounds;
- dispatch atomicity;
- runtime errors and EVM failure behavior; and
- gas and weight charging, including post-dispatch adjustment.

Do not count a selector as coverage merely because it is routed. Test that a
mapped caller with the required signed authority can succeed and that a caller
without that authority fails without changing state. A selector that always
fails `BadOrigin` is not meaningful coverage.

When an extrinsic changes, compare the old and new behavior rather than only
their Rust signatures. Follow [ABI versioning](abi-versioning.md) when an
existing function is affected.

## Cover state with typed views

Inventory every public state map and value in scope. Expose its meaningful
contents through typed view functions; never provide direct writable access to
storage.

Let a view read one or more storage items when that is required to return the
meaningful value. Keep the mapping from source storage to typed functions
explicit in the coverage inventory so no item disappears behind an abstract
claim of domain coverage.

Group related reads into coherent domain precompiles. Do not expose pallet
prefixes, storage keys, hashers, or SCALE encodings as the contract interface.

For every view, specify and test:

- key and account conversions;
- missing-state behavior;
- result types and tuple order;
- units, precision, scaling, and rounding;
- overflow and narrowing conversions;
- maximum input and output size; and
- the exact database reads charged.

When storage changes internally, update the Rust adapter and prove that released
calldata still returns the released meaning.

## Cover runtime APIs and public RPCs

Inventory the publicly facing runtime APIs and RPCs in scope, including the
Subtensor runtime API surface required by `SKILL.md`.

Expose typed functions with equivalent inputs and meaningful outputs. A
precompile may call the same underlying helpers rather than reproduce an RPC
transport detail. Preserve pagination, bounds, defaults, and absence semantics
that affect callers.

Do not copy a bulk runtime API into Solidity when its work or result can grow
with chain state. Prefer one of these bounded shapes:

- an indexed item view plus a bounded count;
- a cursor and caller-supplied limit capped by a fixed runtime maximum; or
- a fixed-size key batch whose maximum is part of the interface contract.

Return the next cursor or an explicit completion indicator when callers need to
walk the complete collection. Charge for the maximum work actually permitted.
Apply limits before reading or constructing the collection; calling an
unbounded runtime helper and slicing its returned vector is still unbounded.

Do not expose node-only behavior that cannot execute deterministically in the
runtime. When a public RPC composes runtime state, implement the deterministic
runtime-side result and document any transport-only behavior that has no EVM
equivalent.

## Add regression tests first

For a bug fix, add a regression unit test that fails for the reported behavior
before implementing the fix. Confirm the failure is caused by the bug, then
apply the fix and confirm the same test passes.

For an ABI-affecting runtime change, add a compatibility test that sends the
exact legacy calldata and decodes the result using the released ABI. A test
that only calls the new Rust helper or new selector does not prove backwards
compatibility.

Keep precompile unit tests with the implementation's existing
`#[cfg(test)] mod tests` pattern and use `precompiles/src/mock.rs`. Reuse
`selector_u32`, `encode_with_selector`, `execute_returns`,
`execute_returns_raw`, and the established mock-state helpers where suitable.

Name tests after observable behavior and the condition being protected. Avoid
tests that merely duplicate an implementation expression.

## Test observable behavior

Cover every affected path:

- legacy success and return decoding;
- new selector success independently;
- invalid and boundary inputs;
- missing state;
- authorization and proxy origin;
- payable, nonpayable, attached-value, and static-call behavior;
- expected state transitions and rollback on failure;
- runtime dispatch errors and EVM errors;
- account and address conversion;
- TAO and Alpha unit conversion;
- precision, rounding, overflow, and narrowing;
- bounded collections and duplicate inputs;
- lifecycle status, hard-deprecation error, and disable/re-enable behavior when
  applicable; and
- storage adapters against legacy and new state during migrations.

Test both a representative normal case and the boundaries where conversion or
runtime semantics change.

## Validate ABIs and routing

Treat `precompiles/src/solidity/*.sol` and generated `*.abi` files as external
artifacts. Compare them with the relevant released or base-branch versions.

Verify:

1. Every old canonical signature and selector remains present.
2. Old input and output ABI encodings are unchanged.
3. Every new selector matches its canonical Solidity signature.
4. No selector collides with another selector at the address.
5. Only the intended Solidity interface and ABI gain the intended functions.
6. Unrelated precompile Solidity and ABI files are byte-for-byte unchanged.
7. The Rust macro signature, Solidity declaration, generated ABI, NatSpec, SDK
   copies, registry metadata, and public documentation agree.
8. Every released address remains in `Precompiles::used_addresses()`.
9. `Precompiles::execute()` recognizes the address and routes it through the
   intended availability control and compatible implementation.
10. Unknown-address and unknown-selector behavior remains unchanged.
11. Every new Bittensor domain uses the next reserved sequential address, and
    address constants, `used_addresses()`, routing, documentation, Solidity
    interfaces, and address-locking tests agree.

Do not hand-wave generated-file churn. Inspect each changed ABI entry and
remove unrelated regeneration changes.

## Validate cost and bounds

Keep every precompile path bounded in CPU, memory, storage access, and output
size. Record database reads and writes and dispatch weight through the existing
helpers.

Test:

- gas-limit rejection before overweight dispatch;
- post-dispatch charging and refund behavior when affected;
- the maximum accepted collection size;
- rejection just beyond the bound;
- proof-size-sensitive database access where relevant;
- failure paths that could otherwise perform unpaid work.

Do not accept a bounded input if processing it can trigger an unbounded runtime
scan. Document any change large enough to make a previously practical call
unusable even if its asymptotic complexity is unchanged.

## Run repository checks

Run the narrowest relevant unit test while iterating, then run the complete
precompile package tests:

```sh
cargo test -p subtensor-precompiles
```

Check formatting:

```sh
cargo fmt --all --check
```

Run Clippy for the package when practical:

```sh
SKIP_WASM_BUILD=1 cargo clippy \
  -p subtensor-precompiles \
  --all-targets \
  --all-features \
  -- -D warnings
```

Escalate to workspace checks or affected pallet tests when shared runtime
types, dispatchables, mocks, or routing changed. Report any check that could not
run and the specific reason; do not imply success from an unexecuted check.

## Report the result

Summarize:

- source functionality added, changed, or still missing;
- released addresses and selectors affected;
- adapters or new versions introduced;
- lifecycle or mainnet-release warnings;
- files and ABIs changed;
- evidence that unrelated precompiles and ABIs are unchanged;
- regression tests added and their before/after behavior; and
- commands run, results, and any remaining validation gaps.

Do not claim completion while a required coverage row is unexplained or a
legacy caller test is missing.
