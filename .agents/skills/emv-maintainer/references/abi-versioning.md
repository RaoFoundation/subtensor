# ABI versioning and lifecycle

## Contents

- [Establish the released baseline](#establish-the-released-baseline)
- [Preserve the external contract](#preserve-the-external-contract)
- [Reserve addresses and selectors](#reserve-addresses-and-selectors)
- [Version functions within a domain](#version-functions-within-a-domain)
- [Preserve old behavior through adapters](#preserve-old-behavior-through-adapters)
- [Classify changes](#classify-changes)
- [Apply the lifecycle model](#apply-the-lifecycle-model)
- [Stop an undeployed compatibility break](#stop-an-undeployed-compatibility-break)
- [Report lifecycle and availability](#report-lifecycle-and-availability)
- [Handle reversible disablement](#handle-reversible-disablement)

## Establish the released baseline

Before changing a precompile:

1. Determine which addresses, selectors, Solidity interfaces, and ABI files
   have been deployed or published for production use. Inspect release history
   and the deployed runtime, not only the working tree.
2. Inspect `precompiles/src/lib.rs`, the Rust implementation,
   `precompiles/src/solidity/*.sol`, generated `*.abi` files, tests, public
   documentation, SDK copies, and known integration contracts.
3. Compare the branch with the relevant base and identify every runtime change
   that affects inputs, outputs, state changes, errors, authorization, units,
   value handling, or gas and weight requirements.
4. Treat uncertain production status as released until evidence establishes
   otherwise.
5. Distinguish released interfaces from explicit proposals. Allow an
   unassigned, unpublished proposal to change during design review; freeze its
   address, selectors, and observable behavior once released.

Do not infer compatibility from Rust names. Define the external contract as the
fixed EVM address plus accepted calldata, returned bytes, state effects,
authorization, charging, and success-or-revert behavior.

## Preserve the external contract

Preserve all observable properties of every released call:

- address and selector handling;
- function name, input types, input order, and ABI encoding;
- return types, tuple and struct field order, and ABI encoding;
- documented meaning, units, precision, scaling, rounding, and defaults;
- view, state-changing, payable, and static-call behavior;
- treatment of attached EVM value;
- caller-to-Substrate account mapping and dispatched origin;
- authorization and proxy behavior;
- state transitions and atomicity;
- success-versus-revert behavior and documented error payloads;
- bounded-input and complexity guarantees;
- lifecycle-status selectors and their documented availability guarantees.

Return types do not contribute to a Solidity selector, but changing them under
an existing selector still breaks old callers because they decode the returned
bytes with the old ABI.

Allow internal Rust names, storage layouts, hashers, intermediate types, and
algorithms to change only when the implementation adapts them back to the
released behavior.

Allow runtime weight corrections, but preserve the complexity class and input
bounds. Do not introduce an unannounced increase large enough to make a
previously practical call unusable. Never replace bounded work with an
unbounded scan.

## Reserve addresses and selectors

Keep every released precompile address recognized by the precompile set.
Preserve compatible handling at that address: existing calldata must still
reach behavior that honors its released contract. The internal Rust type or
dispatch structure may change; the observable routing contract may not.

Keep every released selector reserved permanently, including after hard
deprecation. Route a hard-deprecated selector to its descriptive error. Never
allow a different function to claim it.

Assign a genuinely new Bittensor domain the next unused sequential Bittensor
address. The current proposal reserves:

| Address | Domain |
|---|---|
| `0x080f` | Scheduler |
| `0x0810` | Drand |
| `0x0811` | Timestamp |
| `0x0812` | Runtime Configuration |
| `0x0813` | Precompile Registry |

A documented reservation prevents another domain from taking the address but
does not make the precompile callable. When implementing a reserved address,
add exact-value tests for its index and full address, routing tests through the
precompile set, and selector tests for every function at that address.

Before adding a function, calculate its selector from the canonical Solidity
signature and compare it with the complete selector set at the address. Reject
collisions even when the Solidity names differ.

Treat a new function as additive only when:

- its selector does not collide;
- old input and output encodings remain identical;
- unknown-selector and fallback behavior remain unchanged;
- old results and side effects remain unchanged; and
- no unrelated Solidity interface or ABI changes.

## Version functions within a domain

Prefer one fixed address for each coherent domain. Add versions at that address:

```text
functionName
functionNameV2
functionNameV3
```

Keep every earlier version routed. Use a new address only for a genuinely
different domain with an independent responsibility and lifecycle.

Continue supporting legacy addresses created under earlier per-contract
versioning. Do not use them as a precedent for creating a new address whenever
one function changes.

Do not attempt a return-type-only overload. Because return types do not
distinguish selectors, use a versioned name or a genuinely distinct input
signature.

When an audited integration expects a missing chain value or operation, prefer
adding the typed function it expects to the appropriate existing precompile.
Do not require changes to an audited wrapper when the precompile can satisfy
the wrapper's existing interface safely.

## Preserve old behavior through adapters

Adapt released calls to new runtime representations whenever the old result can
still be produced honestly with bounded, proportionate work:

- Reconstruct an old aggregate when one stored value becomes several.
- Return the original tuple when a struct gains fields; expose the extended
  tuple through a new version.
- Update Rust storage access when names, keys, hashers, or map shapes change.
- Supply the exact old default when an extrinsic gains an option; expose the
  option through a new version.
- Derive the documented old result when the runtime replaces its computation.
- Preserve legacy units, precision, scaling, and rounding in the old function;
  expose a corrected convention through a new version.

Do not fabricate data to retain a byte shape. Do not reinterpret an old field
as a different concept. If an adapter cannot preserve the documented meaning,
make an explicit lifecycle decision.

## Classify changes

| Runtime change | Required treatment |
|---|---|
| Storage rename, hasher change, or map restructuring | Update the Rust implementation; preserve ABI and meaning. |
| Equivalent internal computation refactor | Keep the function and verify equivalent observable results. |
| Additional returned information | Keep the old subset; add a version for the richer result. |
| Input or return type/order change | Add a version with a new selector. |
| One concept splits into several | Reconstruct the old aggregate when honest; expose components through a version. |
| Extrinsic gains an option | Preserve the old default; expose the option through a version. |
| Entirely new operation or view | Add a selector to the appropriate domain. |
| Concept disappears without an honest representation | Reserve the selector and evaluate hard deprecation. |
| Bug fix changes observable semantics | Preserve the released behavior and add a corrected version unless retaining it is unsafe. |
| Urgent security or operational risk | Report the risk and consider whether reversible disablement should be recommended. |

For a security-critical behavior that cannot remain callable, stop and report
the compatibility break. Do not silently change or delete the selector.

## Apply the lifecycle model

Keep function lifecycle separate from precompile availability:

| Condition | Required call behavior |
|---|---|
| Active and enabled | Execute normally. |
| Soft-deprecated and enabled | Preserve the documented behavior and encoding. |
| Hard-deprecated and enabled | Keep routing the selector and return a descriptive precompile error. |
| Disabled | Return the precompile-disabled error regardless of function lifecycle. |

Use soft deprecation by default. Preserve the call, mark the Solidity function
with `@deprecated`, and publish replacement metadata without adding
deprecation-only work to every invocation.

Use hard deprecation only when old behavior cannot be represented honestly or
safely, for example because:

- the underlying concept no longer exists and has no representation;
- the semantics changed beyond what the old return type can describe; or
- preservation requires fabricated data, dead state, unbounded work, or an
  unacceptable security risk.

Do not hard-deprecate because a replacement is newer, easier to maintain, or
more complete. First document why an adapter is impossible or disproportionate,
identify affected released functions and known callers, provide a replacement
when possible, and complete the agreed migration process.

## Stop an undeployed compatibility break

If the runtime change that makes old behavior impossible has not reached
mainnet, treat mainnet deployment as blocked by the compatibility break. Do not
interpret a request to update precompiles as authorization to deploy the break
or hard-deprecate affected functions immediately.

Stop and give the developer this prominent warning:

> **Mainnet compatibility warning:** This change would force hard deprecation
> of `<precompile address, function, and selector>` and break contracts that
> still call it. Do not deploy the incompatible runtime change to mainnet until
> `<replacement>` is available, the old function has been soft-deprecated for
> the agreed migration window, and the phase-out criteria have been satisfied.

State why an adapter cannot work, which released functions and known callers
are affected, what replacement is available or required, and which phase-out
steps remain. Continue only with non-breaking preparation such as adding the
replacement, tests, documentation, and lifecycle metadata. Preserve current
mainnet behavior throughout the migration window. Hard-deprecate only in the
later release that completes the planned phase-out.

## Report lifecycle and availability

Use this proposed registry shape as the compatibility target:

```solidity
struct PrecompileStatus {
    bool isDeprecated;
    bool isDisabled;
    address newPrecompile;
    bytes4 newSelector;
    string message;
}
```

Interpret `isDeprecated` as soft or hard function deprecation. Interpret
`isDisabled` as current unavailability through a reversible operational switch.
Use `newPrecompile` and `newSelector` for the recommended replacement; zero
replacement fields mean that none is available. Use `message` for
human-readable status or migration guidance.

Do not infer deprecation from disablement. Do not clear deprecation when a
precompile is re-enabled. Do not describe the registry as callable until its
address and implementation are released.

Keep registry metadata, Solidity NatSpec, public documentation, and call
behavior consistent. Prefer static registry queries over emitting a log on
every deprecated call.

## Handle reversible disablement

Treat disablement as an external, reversible operational action, not a normal
deprecation step. An agent may identify a risk, verify the mechanism, and
recommend that responsible decision-makers consider it. An agent cannot
perform or authorize the action.

Require re-enablement to restore each function's previous active,
soft-deprecated, or hard-deprecated behavior. Never erase lifecycle metadata
when availability changes.

Before recommending disablement, verify that the address routes through
`PrecompileExt::try_execute` and uses the intended `PrecompileEnum` entry.
Check whether multiple addresses share that entry and report the complete
effect of a toggle. Do not claim an address is toggleable merely because the
general mechanism exists.
