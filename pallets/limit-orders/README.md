# pallet-limit-orders

A FRAME pallet for off-chain signed limit orders on Bittensor subnets.

Users sign orders off-chain and submit them to a relayer. The relayer batches
orders targeting the same subnet and submits them via `execute_batched_orders`,
which nets the buy and sell sides, executes a single AMM pool swap for the
residual, and distributes outputs pro-rata to all participants. This minimises
price impact compared to executing each order independently against the pool.

Three things are worth knowing up front:

- **Orders come in two schema versions.** `V1` is a plain order with an absolute
  amount. `V2` adds **linked orders** — an order sized as a fraction of the output
  another order already produced, so a user can sign "sell my subnet-7 alpha, then
  put the TAO that produced into subnet 12" without knowing the proceeds in advance.
- **Three signing forms are accepted**, including a human-readable
  ("clear-signing") form that a hardware wallet can display field-by-field instead
  of showing an opaque hash.
- **The pallet is disabled by default** (`LimitOrdersEnabled` defaults to `false`)
  and must be enabled by root via `set_pallet_status`.

MEV protection is available for free: any caller can wrap `execute_orders` or
`execute_batched_orders` inside `pallet_shield::submit_encrypted` to hide the
batch contents from the mempool until the block is proposed.

---

## Order lifecycle

```
User signs VersionedOrder::V1(Order) or ::V2(OrderV2) off-chain
        │   (raw SCALE, wrapped hash, or readable message — see "Signing forms")
        ▼
Relayer submits via execute_orders          Relayer submits via execute_batched_orders
        (one pool swap per order)                   (aggregated, atomic)
        │                                            │
        ├─ should_fail = false (best effort):        ├─ Any order invalid / expired /
        │  invalid / expired / price-not-met         │  price-not-met / root netuid /
        │  orders are skipped, emitting              │  duplicate →
        │  OrderSkipped with the DispatchError       │  entire batch fails (DispatchError)
        │                                            │
        ├─ should_fail = true (all-or-nothing):     └─ All orders valid → net pool swap
        │  the first failure aborts the call,               → distribute pro-rata
        │  reverting orders already executed
        │
        └─ Valid → executed
                    │
                    ├─ order_id written to Orders (Fulfilled / PartiallyFilled)
                    │  (prevents replay)
                    └─ if has_linked_order: output recorded in LinkedOutputs,
                       drawable by the one linked order that names this order_id

User can cancel at any time via cancel_order
        └─ order_id written to Orders as Cancelled
```

---

## Data structures

### `VersionedOrder<AccountId>`

Versioned wrapper around an order payload.

| Variant | Description |
|---------|-------------|
| `V1(Order<AccountId>)` | First version of the order schema. |
| `V2(OrderV2<AccountId>)` | Adds linked orders: `amount` may be a fraction of another order's recorded output, and an order may declare that its own output be recorded. |

Versioning lets the pallet accept orders signed against different schemas
simultaneously. `V1` signed orders remain valid after `V2` was added, because the
`OrderId` and the signature both cover the full `VersionedOrder` encoding
(including the version discriminant byte). The version tag is also rendered into
the human-readable message, so a `V1` signature can never be replayed as a `V2`
order or vice versa.

Internally every validation and execution path reads `OrderView`, a
version-agnostic projection, rather than branching on the variant. `V1` projects
its `u64` amount to `OrderAmount::Fixed` and its `has_linked_order` to `false`,
which makes `V1` a special case of `V2` rather than a parallel code path.

### `Order<AccountId>` (V1)

The payload that a user signs off-chain, wrapped inside `VersionedOrder`. Never
stored in full on-chain — only the `blake2_256` hash of the `VersionedOrder`
encoding (`OrderId`) is persisted.

| Field           | Type        | Description |
|-----------------|-------------|-------------|
| `signer`        | `AccountId` | Coldkey that authorises the order. For buy types: pays TAO. For sell types: owns the staked alpha. |
| `hotkey`        | `AccountId` | Hotkey to stake to (buy types) or unstake from (sell types). |
| `netuid`        | `NetUid`    | Target subnet. |
| `order_type`    | `OrderType` | One of `LimitBuy`, `TakeProfit`, or `StopLoss` (see table below). |
| `amount`        | `u64`       | Input amount in raw units. TAO for buy types; alpha for sell types. |
| `limit_price`   | `u64`       | Price threshold in ×10⁹ scale (same scale as the `current_alpha_price` RPC): `1_000_000_000` = 1.0 TAO/alpha. Trigger direction depends on `OrderType`. `u64::MAX` = no ceiling; `0` = no floor. |
| `expiry`        | `u64`       | Unix timestamp in milliseconds. Order must not execute after this time. |
| `fee_rate`      | `Perbill`   | Per-order fee as a fraction of the order's TAO amount. `Perbill::zero()` = no fee. |
| `fee_recipient` | `AccountId` | Account that receives the fee collected for this order. |
| `relayer`       | `Option<BoundedVec<AccountId, ConstU32<10>>>` | Accounts authorised to relay this order — up to 10. When `Some`, only an account in the list may submit the execution transaction; anyone else is rejected with `RelayerMissMatch`. `None` = any relayer may execute. Note that `None` and `Some([])` are deliberately distinct, both in encoding and in the readable rendering. |
| `max_slippage`  | `Option<Perbill>`   | Maximum acceptable slippage in parts per billion applied to `limit_price` at swap time. `None` = no slippage protection (execute at market). When `Some(p)`: Buy ceiling = `limit_price + limit_price * p`; Sell floor = `limit_price - limit_price * p`. Both saturate at `u64` bounds. |
| `chain_id`      | `u64`       | EVM-compatible chain ID this order is bound to. Prevents replay of a testnet-signed order on mainnet and vice versa. Must equal `Config::ChainId` or the order is rejected with `ChainIdMismatch`. |
| `partial_fills_enabled` | `bool` | Whether the relayer may fill this order in instalments via `SignedOrder::partial_fill`. Requires `relayer` to be `Some`. |

### `OrderV2<AccountId>` (V2)

Field-for-field identical to `Order` except for two changes, which together
express the entire linked-order mechanism:

| Field | Type | Description |
|-------|------|-------------|
| `amount` | `OrderAmount` | Either an absolute amount (V1 semantics) or a fraction of another order's recorded output. |
| `has_linked_order` | `bool` | When `true`, this order's output is recorded in `LinkedOutputs` so that the one linked order naming it may draw against it. When `false` nothing is recorded and no order can ever link to this one. |

`has_linked_order` is a **signed authorisation, not a hint**: it is the user
declaring that the proceeds of this order may be spent by the linked order they
signed alongside it. It is therefore rendered into the human-readable message, so
it cannot be flipped on an order the user signed without it.

The two halves are independent. An order can be a provider (`has_linked_order =
true`) with a `Fixed` amount, a consumer (`LinkedPercentage`) whose own output is
not recorded, both at once — which is how chains longer than two legs are built —
or neither, which is exactly a V1 order.

### `OrderAmount`

| Variant | Description |
|---------|-------------|
| `Fixed(u64)` | An absolute raw amount: TAO for buys, alpha for sells. Identical in meaning to `Order::amount` in V1. |
| `LinkedPercentage { provider: H256, pct: Perbill }` | Take `pct` of the output recorded for the order whose `OrderId` is `provider`. |

`provider` is the `OrderId` — `blake2_256` over the SCALE-encoded
`VersionedOrder`, i.e. what `derive_order_id` returns and what the user can
compute off-chain before signing.

**Why the provider is named by `order_id`.** Anything weaker leaves a substitution
gap: if the link were positional, or pinned only the provider's subnet, a relayer
holding several of the user's signed sells could fund a consumer out of the
*wrong* one — a forced portfolio rotation that every signature and price bound
would still accept. Naming the id makes the reference a singleton.

`Perbill`'s `Decode` rejects values above `1_000_000_000`, so a decoded fraction
can never exceed 100% of the recorded output.

### `LinkedAsset<AccountId>`

The asset an order produced, and therefore the asset a linked order may consume.

| Variant | Produced by | Spendable by |
|---------|-------------|--------------|
| `Tao` | A sell (`TakeProfit` / `StopLoss`) | Only a `LimitBuy` |
| `Alpha { netuid, hotkey }` | A `LimitBuy` | Only a sell from that same `(netuid, hotkey)` position |

Alpha is only fungible within a single `(netuid, hotkey)` stake position, so the
identity of alpha output carries both.

### `LinkedOutput<AccountId>`

A provider order's recorded output — the denominator a linked order's percentage
resolves through.

| Field | Type | Description |
|-------|------|-------------|
| `signer` | `AccountId` | The provider's signer. The linked order must be signed by this same coldkey. |
| `asset` | `LinkedAsset<AccountId>` | What the provider produced, and therefore what a consumer's *input* side must be. |
| `total` | `u64` | Output the provider produced, **post-fee** — the amount that actually landed with `signer`. |
| `expires_at` | `u64` | Unix ms after which no consumer may draw against the record and anyone may prune it. |

This is **accounting, not custody**. The output was credited to `signer` by the
provider's own execution, exactly as an unlinked order would have credited it; the
record only caps how much of it a linked order is authorised to spend. A consumer
still pays out of the signer's own balance, so spending the proceeds elsewhere
first makes the consumer fail on funds rather than on this cap.

### `OrderType`

| Variant      | Action        | Triggers when           | Use case |
|--------------|---------------|-------------------------|----------|
| `LimitBuy`   | Buy alpha      | price ≤ `limit_price`  | Enter a position at or below a target price. |
| `TakeProfit` | Sell alpha     | price ≥ `limit_price`  | Exit a position once price rises to a profit target. |
| `StopLoss`   | Sell alpha     | price ≤ `limit_price`  | Exit a position to limit downside if price falls to a floor. |

### `SignedOrder<AccountId>`

Envelope submitted by the relayer.

| Field | Type | Description |
|-------|------|-------------|
| `order` | `VersionedOrder<AccountId>` | The signed payload. |
| `signature` | `MultiSignature` | sr25519 or ed25519. ECDSA is **not** accepted. Verified against the inner `order.signer` as the expected public key. |
| `partial_fill` | `Option<u64>` | Relayer-supplied fill amount. **Not part of the signed payload** — it is the relayer's choice within the bounds the signed order allows. `None` = one-shot full execution. |

---

## Signing forms

Three payloads are accepted for the same order. Verification is attempted in this
order — cheapest first — and any one of them succeeding accepts the order:

| Form | Signed bytes | Who produces it |
|------|--------------|-----------------|
| **Raw** | `SCALE(VersionedOrder)` | A software wallet signing arbitrary bytes. Has no `<Bytes>` envelope, so a Ledger can never produce it. |
| **Wrapped hash** | `<Bytes>` ++ `blake2_256(SCALE(VersionedOrder))` ++ `</Bytes>` — i.e. the `OrderId` | A wallet that can only sign short opaque messages. Fixed 47 bytes. |
| **Readable** | `<Bytes>` ++ `utf8(render_order(order))` ++ `</Bytes>`, blake2_256-hashed when longer than `LEDGER_MAX_SIGN_SIZE` | A hardware wallet doing clear-signing. |

### The readable ("clear-signing") message

`render_order` builds a canonical, single-line, all-printable-ASCII rendering. It
is a **pure function of the payload's fields**, so a TS client or a Ledger app can
rebuild the exact same bytes:

```
TAO.com order v2: Limit buy {amount} on subnet {netuid}, limit price {limit_price},
expiry {expiry}, hotkey {ss58}, fee {fee_rate} to {ss58}, relayer {relayer},
max slippage {max_slippage}, chain {chain_id}, partial fills {bool},
signer {ss58}, has-linked-order {bool}
```

(one line, `, ` between fields; `Take-profit` / `Stop-loss` use `trigger price`
instead of `limit price`; V1 renders `v1` and **no** `has-linked-order` tail)

Renderings that are load-bearing rather than cosmetic:

- **`relayer`**: `none` for `None`, `[]` for an empty list, otherwise the SS58
  addresses joined with `+`. Keeping `none` and `[]` distinct is what prevents a
  signature for an "any relayer" order being transplanted onto an order with an
  empty relayer list.
- **`fee_rate`, `max_slippage`, `pct`**: raw parts-per-billion integers, never a
  rendered percentage. `Perbill`'s `Debug` would give a nicer `25%`, but `Debug`
  carries no stability guarantee and this string is consensus-critical.
- **A linked amount**: `{pct} ppb of order 0x{64 lowercase hex} output`. The suffix
  is what keeps the two `OrderAmount` variants injective — no bare `Fixed` amount
  can produce it. The provider id is rendered in full because a truncated
  reference would reintroduce the substitution gap the id exists to close.
- **Accounts**: SS58 at `frame_system::Config::SS58Prefix` (42 on this runtime),
  re-encoded regardless of the prefix the address arrived in.

### The Ledger hashing rule

A Ledger blake2_256-hashes any `signRaw` payload longer than
`LEDGER_MAX_SIGN_SIZE` (**256 bytes**, `MAX_SIGN_SIZE` in the Zondax Polkadot app)
before signing it, so the signature commits to the hash of the payload rather
than to the payload bytes. `verify_readable` follows the same rule: over the
limit it verifies `blake2_256(payload)`, at or below it verifies the payload
directly. The branch is deterministic on payload length, so there is no
malleability between the two.

In practice the readable message is always oversized — three SS58 addresses alone
are 144 characters — so the hashed branch is the live one. This is **not** blind
signing: the device's printable-ASCII check and pagination operate on the received
buffer and the hashing happens later, in the signing step only, so the user still
sees the full text.

> A software `signRaw` (polkadot.js extension, `keyring.sign()`) does **not** hash.
> A software-produced signature over an oversized payload is not valid on this path.

The rule is pinned by a recorded hardware vector in
`src/tests/ledger_vector.rs` (Nano S+, Polkadot Generic v100.0.25, path
`m/44'/354'/0'/0'/0'`), with a probe matrix ruling out the alternatives.

### `OrderStatus`

State of a processed order, stored under its `OrderId`.

| Variant | Meaning |
|---------|---------|
| `Fulfilled` | Order was fully executed. Terminal. |
| `PartiallyFilled(u64)` | Order was filled in instalments; the value is the cumulative amount filled so far. Not terminal — the remainder may still be filled with an explicit `partial_fill`. |
| `Cancelled` | User registered a cancellation intent before execution. Terminal. |

---

## Storage

### `Orders: StorageMap<H256, OrderStatus>`

Maps an `OrderId` (blake2_256 of the SCALE-encoded `VersionedOrder`) to its
`OrderStatus`. Absence means the order has never been seen and is still executable
(provided it is valid). `Fulfilled` and `Cancelled` are permanently closed;
`PartiallyFilled` may still be completed.

### `LinkedOutputs: StorageMap<H256, LinkedOutput<AccountId>>`

Output recorded by orders that declared `has_linked_order`, keyed by the
provider's `OrderId`. Written when such an order executes, and removed either by
the single linked order that draws against it, or by `prune_linked_output`.

### `LimitOrdersEnabled: StorageValue<bool>`

Master switch, **defaulting to `false`**. While disabled, `execute_orders`,
`execute_batched_orders`, and `cancel_order` all fail with `LimitOrdersDisabled`.
`prune_linked_output` deliberately keeps working, so a record can always be
reclaimed. Set by root via `set_pallet_status`.

### `HasMigrationRun: StorageMap<BoundedVec<u8>, bool>`

Migration bookkeeping.

---

## Config

| Item                  | Type                                              | Description |
|-----------------------|---------------------------------------------------|-------------|
| `SwapInterface`       | `OrderSwapInterface<Self::AccountId>`             | Full swap + balance execution interface. Implemented by `pallet_subtensor::Pallet<T>`. Provides `buy_alpha`, `sell_alpha`, `transfer_tao`, `transfer_staked_alpha`, and `current_alpha_price`. |
| `TimeProvider`        | `UnixTime`                                        | Current wall-clock time for expiry checks and provider-record TTLs. |
| `MaxOrdersPerBatch`   | `Get<u32>` (constant)                             | Maximum number of orders accepted in a single `execute_orders` or `execute_batched_orders` call. Should equal `floor(max_block_weight / per_order_weight)`. |
| `PalletId`            | `Get<PalletId>` (constant)                        | Used to derive the pallet intermediary account (`PalletId::into_account_truncating`). This account temporarily holds pooled TAO and staked alpha during `execute_batched_orders`. |
| `PalletHotkey`        | `Get<Self::AccountId>` (constant)                 | Hotkey the pallet intermediary account stakes to/from during batch execution. Must be a dedicated hotkey registered on every subnet the pallet may operate on. Operators should register it as a non-validator neuron. |
| `ChainId`             | `Get<u64>` (constant)                             | The chain's EVM-compatible chain ID. An order whose `chain_id` differs is rejected with `ChainIdMismatch`, which is what prevents cross-chain replay. |
| `LinkedOutputTtl`     | `Get<u64>` (constant)                             | How long, in milliseconds, a provider's recorded output stays drawable before anyone may prune it. **180 days on this runtime**: a linked take-profit is a standing instruction that may legitimately wait months for its trigger, and an expiry that fired first would strand the user holding the provider's proceeds with an unexecutable second leg. |
| `WeightInfo`          | `weights::WeightInfo`                             | Benchmarked weight functions for each extrinsic. Use `weights::SubstrateWeight<Runtime>` in production and `()` in tests. |

---

## Extrinsics

### `execute_orders(orders, should_fail)` — call index 0

**Origin:** any signed account (typically a relayer).

Executes a list of signed limit orders one by one, each interacting with the AMM
pool independently. `should_fail` chooses the failure policy:

- **`false` (best-effort):** an order that fails for any reason is skipped and
  emits `OrderSkipped` with the `DispatchError`. A single bad order does not
  revert the others.
- **`true` (all-or-nothing):** the first failure returns the underlying error,
  reverting any orders already executed in this call.

**Fee handling:** each order's `fee_rate` is deducted from the TAO input (buys) or
the TAO output (sells) and forwarded to that order's `fee_recipient`.

**Linked orders chain naturally here.** Each order runs to completion before the
next is validated, so a provider and the linked order that draws on it can be
submitted in the same call.

**When to use:** small batches, orders targeting different subnets, or any chain
of linked orders. Use `execute_batched_orders` for same-subnet batches to reduce
price impact.

---

### `execute_batched_orders(netuid, orders)` — call index 1

**Origin:** any signed account (typically a relayer).

Aggregates all valid orders targeting `netuid` into a single net pool interaction:

1. **Validate & classify** — if any order has the wrong netuid, an invalid
   signature, an already-processed id, a duplicate id within the batch, a past
   expiry, a price condition not met, a mismatched chain id, or targets the root
   netuid (0), the **entire call fails** with the corresponding error. Valid
   orders are split into buy-side (`LimitBuy`) and sell-side (`TakeProfit`,
   `StopLoss`) groups. A linked order is sized here, against its provider's
   recorded output, and the record is consumed here too. Each order's
   `effective_swap_limit` (derived from `limit_price` and `max_slippage`) is
   computed and stored for use in the pool swap.

2. **Collect assets** — gross TAO is pulled from each buyer's free balance into
   the pallet intermediary account. Gross alpha stake is moved from each seller's
   `(coldkey, hotkey)` position to the pallet intermediary's `(pallet_account,
   pallet_hotkey)` position.

3. **Net pool swap** — buy TAO and sell alpha are converted to a common TAO basis
   at the current spot price and offset against each other. Only the residual
   amount touches the pool in a single swap:
   - Buy-dominant: residual TAO is sent to the pool; pool returns alpha. Price ceiling = `min(effective_swap_limit)` across all buy orders.
   - Sell-dominant: residual alpha is sent to the pool; pool returns TAO. Price floor = `max(effective_swap_limit)` across all sell orders.
   - Perfectly offset: no pool interaction.

4. **Distribute alpha pro-rata** — every buyer receives their share of the total
   available alpha (pool output + seller passthrough alpha). Share is proportional
   to each buyer's net TAO contribution. Integer division floors each share; any
   remainder stays in the pallet intermediary account as dust.

5. **Distribute TAO pro-rata** — every seller receives their share of the total
   available TAO (pool output + buyer passthrough TAO), minus their order's fee.
   Share is proportional to each seller's alpha valued at the current spot price.
   Integer division floors each share; any remainder stays in the pallet
   intermediary account as dust.

6. **Collect fees** — buy-side fees (withheld from each order's TAO input) and
   sell-side fees (withheld from each order's TAO output) are accumulated per
   unique `fee_recipient` and forwarded in a single transfer per recipient.

7. **Emit `GroupExecutionSummary`.**

> **A provider and its own consumer cannot share one batched call.** Every amount
> is resolved and consumed in step 1, before the single netted swap in step 3 that
> would produce the provider's output even runs. Such a batch is rejected
> `NoLinkedOutput` — structurally, not incidentally. Split the two legs across two
> calls, or use `execute_orders`.

> **Note:** rounding dust (alpha and TAO) accumulates in the pallet intermediary
> account between batches. If an emission epoch fires while dust is present, the
> pallet earns emissions it never distributes.

---

### `cancel_order(order)` — call index 2

**Origin:** the order's `signer` (coldkey).

Registers a cancellation intent by writing the `OrderId` into `Orders` as
`Cancelled`. Once cancelled an order can never be executed. The full
`VersionedOrder` payload is required so the pallet can derive the `OrderId`.

---

### `set_pallet_status(enabled)` — call index 3

**Origin:** root.

Enables or disables the pallet. Enabling requires the pallet intermediary
account's `PalletHotkey` to be registered, otherwise the call fails with
`PalletHotkeyNotRegistered` — batch execution would break without it.

---

### `prune_linked_output(order_id)` — call index 4

**Origin:** any signed account.

Removes a provider's recorded output from `LinkedOutputs`. Two callers are
allowed:

- the record's **`signer`, at any time** — withdrawing the authorisation they gave
  their linked order;
- **anyone**, once `expires_at` has passed — which is what keeps the map bounded,
  since a provider whose linked order never fires would otherwise leave a
  permanent entry.

Anyone else, before expiry, is rejected with `LinkedOutputNotPrunable`.

**Pruning moves no funds.** The output was already credited to `signer` by the
provider's execution; only the authorisation to spend it through a linked order
goes away. Works even while the pallet is disabled, so a record is always
reclaimable.

---

## Linked orders

A linked order is sized as a fraction of the output another order *already
produced*, rather than as an absolute amount the user must know at signing time.

```
1. User signs a provider:  TakeProfit, Fixed(100 alpha), has_linked_order = true
                           → order_id = blake2_256(SCALE(VersionedOrder))
2. User signs a consumer:  LimitBuy, LinkedPercentage { provider: <that id>, pct: 100% }
3. Provider executes  → sells 100 alpha, receives T TAO post-fee
                      → LinkedOutputs[provider_id] = { signer, Tao, total: T, expires_at }
                      → emits LinkedOutputRecorded
4. Consumer executes  → resolves to pct * T, spends it buying alpha
                      → record REMOVED, emits LinkedOutputConsumed { amount, undrawn }
```

### A record is drawn exactly once

The first linked order to draw takes `pct` of the recorded output and the record
is **removed**, whatever `pct` was. So a provider funds one linked order, not a
basket: `pct` means "spend this much of the proceeds", and the unspent `1 - pct`
simply stays with the signer as ordinary balance.

That is what makes the conservation invariant free. There is no drawn-so-far
counter to keep, no ordering between competing consumers to reason about, and no
way for two linked orders naming one provider to collectively draw more than it
produced — the second finds no record and fails `NoLinkedOutput`.

Fan-out, if it is ever wanted, is a strictly additive change: reintroduce a
drawn-so-far counter and delete the record only when it reaches `total`. Nothing
in the payload would have to change.

### What a consumer must match

| Check | Error on failure |
|-------|------------------|
| A record exists for `provider` | `NoLinkedOutput` |
| The consumer's signer equals the record's `signer` | `LinkedOutputSignerMismatch` |
| The consumer's **input** asset equals the record's `asset` | `LinkedOutputAssetMismatch` |
| `now <= expires_at` | `LinkedOutputExpired` |
| `pct * total` does not floor to zero | `LinkedAmountResolvedToZero` |

The record is read (not removed) during validation and consumed only **after** the
trade lands. So a linked order that validates but fails to swap — most commonly
because the signer spent the proceeds elsewhere — leaves the record intact and the
order retryable with the same signature.

### Partial fills are excluded, on both sides

| Situation | Error |
|-----------|-------|
| A `partial_fill` submitted against an order with a linked amount | `PartialFillNotSupportedForLinkedAmount` |
| A `partial_fill` submitted against an order with `has_linked_order` | `PartialFillNotSupportedForProvider` |

A consumer's total is derived rather than signed, so there is no stable
`sum(fills) <= amount` right-hand side to check against. A provider filled in
instalments would produce output in instalments, making the recorded `total` its
linked order divides depend on how the relayer chose to slice the fills.

Note this is a rejection of the submitted fill, not of the signed flags: the same
provider payload with `partial_fills_enabled = true` executes fine as long as no
`partial_fill` is supplied.

---

## Events

| Event | Fields | Emitted when |
|-------|--------|--------------|
| `OrderExecuted` | `order_id`, `signer`, `netuid`, `order_type`, `amount_in`, `amount_out` | An individual order was successfully executed (by either extrinsic). Both amounts are post-fee on their respective sides. |
| `OrderSkipped` | `order_id`, `reason` | An order was skipped by `execute_orders` with `should_fail = false`. `reason` is the `DispatchError` that caused the skip. Never emitted by `execute_batched_orders` — invalid orders there fail the whole call. |
| `OrderCancelled` | `order_id`, `signer` | The signer registered a cancellation via `cancel_order`. |
| `GroupExecutionSummary` | `netuid`, `net_side`, `net_amount`, `actual_out`, `executed_count` | Once per `execute_batched_orders` call, summarising the net pool trade. `net_side` is `Buy` if TAO was sent to the pool, `Sell` if alpha was. `net_amount` and `actual_out` are zero when the two sides perfectly offset. |
| `LimitOrdersPalletStatusChanged` | `enabled` | Root enabled or disabled the pallet. |
| `LinkedOutputRecorded` | `order_id`, `signer`, `asset`, `total`, `expires_at` | An order that declared `has_linked_order` recorded its output, making it drawable. |
| `LinkedOutputConsumed` | `provider`, `consumer`, `amount`, `undrawn` | A linked order drew against a provider's record, consuming it. `undrawn` is `total - amount`, which stays with the signer as ordinary balance. |
| `LinkedOutputPruned` | `order_id`, `total` | A record was removed without ever being drawn from. `total` is what was recorded and never claimed; it stays with the signer. |

---

## Errors

### Validation and execution

| Error | Cause |
|-------|-------|
| `InvalidSignature` | No accepted signing form verifies against the payload and `order.signer`, or the signature is ECDSA. |
| `OrderAlreadyProcessed` | The `OrderId` is already present in `Orders` as `Fulfilled` or `Cancelled`. Note that `OrderId` has no nonce, so two identical payloads collide — vary a field to place the same trade twice. |
| `OrderCancelled` | The `OrderId` is present in `Orders` as `Cancelled`. |
| `OrderExpired` | `now > order.expiry`. |
| `PriceConditionNotMet` | Current spot price is beyond the order's `limit_price` for its `OrderType`. |
| `OrderNetUidMismatch` | An order inside an `execute_batched_orders` call targets a different netuid than the batch parameter. |
| `RootNetUidNotAllowed` | The order or batch targets netuid 0 (root). Root uses a fixed 1:1 stable mechanism with no AMM — limit orders are not meaningful there. |
| `ChainIdMismatch` | `order.chain_id != Config::ChainId`. Prevents cross-chain replay. |
| `Unauthorized` | Caller of `cancel_order` is not the order's `signer`. |
| `LimitOrdersDisabled` | `LimitOrdersEnabled` is `false`. |
| `RelayerMissMatch` | The caller is not in the order's `relayer` list. Only raised when the field is `Some`. |
| `SwapReturnedZero` | The pool swap returned zero output for a non-zero residual input. |
| `ArithmeticOverflow` | An intermediate calculation overflowed. |
| `PalletHotkeyNotRegistered` | `set_pallet_status(true)` was called while the pallet intermediary's `PalletHotkey` is not registered. |

### Batching

| Error | Cause |
|-------|-------|
| `DuplicateOrderInBatch` | The same `OrderId` appears twice in one `execute_batched_orders` call. |
| `ZeroShareInBatch` | An order's pro-rata share floored to zero during distribution. |

### Partial fills

| Error | Cause |
|-------|-------|
| `PartialFillsNotEnabled` | A `partial_fill` was submitted for an order whose `partial_fills_enabled` is `false`. |
| `IncorrectPartialFillAmount` | The fill is zero, exceeds the remaining amount, or a `partial_fill = None` execution was attempted against an order already `PartiallyFilled`. |
| `RelayerRequiredForPartialFill` | A `partial_fill` was submitted for an order with `relayer: None`. |
| `PartialFillNotSupportedForLinkedAmount` | A `partial_fill` was submitted against an order with a `LinkedPercentage` amount. |
| `PartialFillNotSupportedForProvider` | A `partial_fill` was submitted against an order with `has_linked_order`. |

### Linked orders

| Error | Cause |
|-------|-------|
| `NoLinkedOutput` | The named provider has no record: it has not executed yet, never declared `has_linked_order`, was already drawn from, or was pruned. Also what a provider-and-consumer-in-one-batched-call hits. |
| `LinkedOutputSignerMismatch` | The linked order's signer differs from the provider's. |
| `LinkedOutputAssetMismatch` | The provider's output asset is not what the linked order spends. |
| `LinkedOutputExpired` | `now > record.expires_at`. Checked on draw as well as on prune, so drawability does not depend on when someone got around to calling `prune_linked_output`. |
| `LinkedAmountResolvedToZero` | `pct * total` floored to zero. |
| `LinkedOutputNotPrunable` | `prune_linked_output` was called by an account that is not the record's signer, before `expires_at`. |

---

## Fee model

Fees are specified per-order via `fee_rate: Perbill` and `fee_recipient:
AccountId` fields on the order. There is no global protocol fee or admin key.

All fees are collected in TAO regardless of order side.

| Order type              | Fee deducted from | Timing |
|-------------------------|-------------------|--------|
| `LimitBuy`              | TAO input         | Before the pool swap. |
| `TakeProfit`, `StopLoss`| TAO output        | After the pool swap. |

Fee formula: `fee = fee_rate * amount` (using `Perbill` multiplication, which
upcasts to u128 internally to avoid overflow).

In `execute_batched_orders`, fees are accumulated per unique `fee_recipient` and
forwarded in a single transfer per recipient. If multiple orders share the same
`fee_recipient`, they result in exactly one transfer rather than one per order.

**Provider records hold post-fee output.** `LinkedOutput::total` is what actually
landed with the signer — the alpha a buy received (its fee came out of the TAO
input) or the TAO a sell kept after its fee — so it is exactly what a linked order
may draw against.

---

## Known limitations

### `max_slippage` is semantically inverted for `StopLoss` orders

`StopLoss` sells are triggered when the spot price *falls* to `limit_price`.
`max_slippage` derives a sell floor as `limit_price - limit_price * slippage`,
which is computed from the (higher) trigger threshold. By the time the order
fires, the actual market price will typically be **below** `limit_price`, so the
derived floor will almost always exceed the real fill price, causing the swap to
be rejected.

**Consequence:** Applying `max_slippage` to a `StopLoss` order will usually
prevent it from executing. In `execute_orders` the order is skipped (best-effort)
or fails the call (`should_fail = true`); in `execute_batched_orders` the entire
batch fails.

**Recommendation:** Relayers should set `max_slippage: None` on `StopLoss` orders.
If slippage protection is desired, apply it at the relayer layer by choosing a
conservative `limit_price` rather than relying on `max_slippage`.

### A small linked draw can be valid but unexecutable

`LinkedAmountResolvedToZero` only rejects a draw that floors to *zero*. The
staking minimum enforced downstream (`DefaultMinStake` in `pallet_subtensor`) is
much higher, and it surfaces as `AmountTooLow` from inside the swap. So a user who
signs "10% of my sell" against a small provider can hold a perfectly valid link
that no relayer can execute.

**Recommendation:** clients building linked payloads should check
`pct * expected_output >= DefaultMinStake` at signing time rather than leaving the
user to discover it at execution.

### Rounding dust accrues to the pallet account

See the note under `execute_batched_orders`.
