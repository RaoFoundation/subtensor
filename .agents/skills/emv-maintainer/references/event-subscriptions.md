# Event subscription precompiles

## Contents

- [Use typed domain precompiles](#use-typed-domain-precompiles)
- [Inventory reportable events](#inventory-reportable-events)
- [Use a common subscription interface](#use-a-common-subscription-interface)
- [Fund callback delivery](#fund-callback-delivery)
- [Keep event production bounded](#keep-event-production-bounded)
- [Define stable callback ABIs](#define-stable-callback-abis)
- [Normalize variable-length events](#normalize-variable-length-events)
- [Specify delivery semantics](#specify-delivery-semantics)
- [Protect execution](#protect-execution)
- [Test subscription behavior](#test-subscription-behavior)

## Use typed domain precompiles

Expose events from `SubtensorModule` and `AdminUtils` as typed Solidity
callbacks. Do not expose raw `RuntimeEvent`, pallet enum discriminants, or
SCALE-encoded payloads.

Group callbacks by meaning under a small number of independently addressed
precompiles. Use the current proposed domains as the design baseline:

- staking and economic flows;
- neurons, identities, relationships, and key rotation;
- weights and commit-reveal;
- subnet lifecycle, epochs, emissions, leases, and voting-power tracking;
- runtime and subnet configuration.

Consult the corresponding pages under
`docs/guides/evm/precompiles/*-events.mdx` for the current proposed inventory.
Treat names, signatures, mask bits, and addresses as provisional until
released. After release, apply the ABI rules in
[ABI versioning](abi-versioning.md).

Create another address only when an event family has a genuinely separate
domain and lifecycle. Do not create one precompile per pallet event.

## Inventory reportable events

Inspect both event enum definitions and every emission site. An enum variant
without an active emission site is not a live callback. Record it as a coverage
gap or future possibility, not as currently delivered behavior.

For each emitted event:

1. Record the source pallet, variant, fields, and emission sites.
2. Identify whether it originates from an extrinsic, scheduled operation, or
   runtime hook.
3. Assign it to a meaningful event-precompile domain.
4. Define stable EVM field types and conversions.
5. Determine whether the source payload is bounded.
6. Define a recovery view when callbacks alone are not authoritative.
7. Add an event-mask bit without changing any released assignment.

Prioritize hook-origin events because an interested contract cannot obtain them
from its own transaction receipt. Use the same subscription model for relevant
transaction and scheduled-operation events when this provides coherent domain
coverage.

When a new source event starts being emitted, add a new typed callback and mask
bit. Do not change an existing callback to absorb different semantics.

## Use a common subscription interface

Use the same control shape for every event domain unless a documented reason
requires an additive version:

```solidity
struct EventFilter {
    uint256 eventMask;
    uint16 netuid;
    bytes32 accountId;
    bool matchAnyNetuid;
    bool matchAnyAccount;
}

struct Subscription {
    bool active;
    EventFilter filter;
    uint64 callbackGasLimit;
    uint64 nextSequence;
}

function subscribe(
    EventFilter calldata filter,
    uint64 callbackGasLimit
) external;

function unsubscribe() external;

function getSubscription(
    address subscriber
) external view returns (Subscription memory);

function minimumCallbackBalance(
    uint64 callbackGasLimit
) external view returns (uint256);
```

Always make the caller the subscriber. Do not allow one address to subscribe or
unsubscribe another address.

Store at most one fixed-size subscription per contract and event domain. Use a
fixed event mask plus at most one netuid and one account filter. Do not store or
iterate an arbitrary list of filters.

Validate the mask, callback gas limit, filter flags, and minimum balance before
creating or replacing a subscription. Make subscription replacement atomic.

## Fund callback delivery

Charge callback attempts to the subscribing contract's own TAO balance. Require
enough balance at subscription time to fund the documented minimum number of
attempts at the selected callback gas limit.

Charge a reverting callback for the work it consumed. Never let callback
failure revert the runtime operation that produced the source event.

Automatically remove a subscription when its balance cannot fund the next
attempt. Define charging, rounding, and TAO-to-EVM unit conversion precisely.
Do not provide free delivery paths that allow subscription spam.

Keep the minimum-balance calculation available as a typed view so a contract
can determine whether a subscription is fundable before submitting it.

## Keep event production bounded

Do not synchronously iterate all subscribers when an event is emitted. Append a
fixed-size typed report to a bounded queue in O(1), then process a bounded
amount of delivery work in later blocks.

Advance delivery through bounded cursors. Cap:

- queue capacity;
- work per block;
- callback gas;
- report size;
- subscription size; and
- the number of delivery attempts performed by one bounded work item.

Do not copy or ABI-encode an unbounded vector while producing a report. If a
source event is variable-length, normalize it incrementally as described below.

Define what happens when the queue reaches capacity. Never permit unbounded
runtime storage or memory growth.

## Define stable callback ABIs

Give every event a stable, event-specific receiver selector. Include
`uint64 sequence` and `uint64 sourceBlock` in every callback before the
event-specific fields.

Use stable EVM representations:

- Substrate account IDs and hashes: `bytes32`;
- EVM accounts: `address`;
- netuids and UIDs: `uint16` when the runtime domain fits;
- TAO and Alpha amounts: 18-decimal `uint256` values using the documented
  `10^9` conversion factor;
- fixed-point values: an explicitly documented integer representation.

Choose bounded representations for strings, identities, and other structured
values before release. Do not expose a Rust or SCALE representation as the ABI.

After release:

- reserve the precompile address and control selectors;
- reserve every event-mask bit;
- preserve callback names, parameters, order, types, and meaning;
- preserve filter, charging, sequencing, and delivery guarantees; and
- add a versioned callback when richer data is required.

Do not add speculative fields to a callback merely because a future runtime
might produce them. Add another selector when the semantics become concrete.

## Normalize variable-length events

Convert every variable-length source event into bounded callbacks. Emit a
summary when useful, followed by one item callback per entry. Give related
callbacks the same source sequence and include item index and item count.

For UID-indexed emission arrays, interpret the array index as the UID and
deliver one `(uid, amount)` callback per entry. For example, `[10, 20, 30]`
represents UIDs `0`, `1`, and `2`; do not treat an entry as an arbitrary UID
value.

Apply the same approach to children lists, weight hashes, completed-netuid
batches, and similar collections. Use a stable typed representation for
per-item failures instead of SCALE-encoded `DispatchError`.

Produce normalized items incrementally at the source. Do not first copy the
complete vector into a queued report.

## Specify delivery semantics

Treat callbacks as asynchronous, best-effort notifications. Do not promise that
a callback executes in the source event's block.

Use a monotonically increasing source sequence and source block so receivers
can order reports and detect gaps. Define whether normalized items share one
source sequence and how item indices identify completeness.

If bounded queue overwrite or another allowed failure drops a report, make the
gap observable through sequencing. Require authoritative recovery through the
corresponding typed view where contract logic needs exact current state.

Document ordering across event domains only if the implementation guarantees
it. Require receivers to make callbacks idempotent and tolerate retries,
reordering outside documented guarantees, and sequence gaps.

## Protect execution

Apply reentrancy protection around delivery. Do not allow a callback to
recursively create unbounded callback work.

Keep the source runtime operation independent of callback execution. Bound
callback gas and isolate callback failure. Validate that subscriber-controlled
code cannot stall block processing, retain an unpaid subscription, or make
another subscriber's delivery unbounded.

Account for database reads, writes, queue operations, EVM execution, and failed
attempts. Use saturating arithmetic where appropriate and reject values that
cannot be converted safely.

## Test subscription behavior

Test at least:

- self-subscription and self-unsubscription;
- attempts to manage another address;
- invalid masks, filters, and gas limits;
- insufficient initial balance;
- successful charging and delivery;
- reverting and out-of-gas callbacks;
- automatic unsubscription when payment fails;
- event filtering by mask, netuid, and account;
- monotonic sequencing and source-block reporting;
- queue capacity and observable gaps;
- bounded per-block work with many subscribers;
- reentrancy and recursive-work resistance;
- one-item normalization and item ordering;
- unit and account conversions;
- released callback selectors and event-mask assignments; and
- additive introduction of a new callback without changing old callbacks.

Use [Coverage and testing](coverage-and-testing.md) for the general precompile
regression and ABI-diff requirements.
