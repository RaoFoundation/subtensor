# pallet-limit-orders

A FRAME pallet for off-chain signed limit orders on Bittensor subnets.

Users sign orders off-chain and submit them to a relayer. The relayer batches
orders targeting the same subnet and submits them via `execute_batched_orders`,
which nets the buy and sell sides, executes a single AMM pool swap for the
residual, and distributes outputs pro-rata to all participants. This minimises
price impact compared to executing each order independently against the pool.

MEV protection is available for free: any caller can wrap `execute_orders` or
`execute_batched_orders` inside `pallet_shield::submit_encrypted` to hide the
batch contents from the mempool until the block is proposed.

---

## Order lifecycle

```
User signs VersionedOrder::V1(Order) off-chain
        │
        ▼
Relayer submits via execute_orders        Relayer submits via execute_batched_orders
        (one-by-one, best-effort)                  (aggregated, atomic)
        │                                           │
        ├─ Invalid / expired /                      ├─ Any order invalid / expired /
        │  price-not-met →                          │  price-not-met / root netuid →
        │  skipped, emits OrderSkipped              │  entire batch fails (DispatchError)
        │  with DispatchError reason                │
        │                                           │
        └─ Valid → executed                         └─ All orders valid → net pool swap
                        │                                   → distribute pro-rata
                        └─ order_id written to Orders as Fulfilled
                           (prevents replay)

User can cancel at any time via cancel_order
        └─ order_id written to Orders as Cancelled
```

---

## Data structures

### `VersionedOrder<AccountId>`

Versioned wrapper around an order payload. Currently has one variant:

| Variant | Description |
|---------|-------------|
| `V1(Order<AccountId>)` | First version of the order schema. |

Versioning lets the pallet accept orders signed against different schemas
simultaneously. When a new variant is added (`V2`, etc.), old `V1` signed orders
remain valid because the `OrderId` and signature both cover the full
`VersionedOrder` encoding (including the version discriminant byte).

### `Order<AccountId>`

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
| `limit_price`   | `u64`       | Price threshold in TAO/alpha raw units. Trigger direction depends on `OrderType` (see table below). |
| `expiry`        | `u64`       | Unix timestamp in milliseconds. Order must not execute after this time. |
| `fee_rate`      | `Perbill`   | Per-order fee as a fraction of the input amount. `Perbill::zero()` = no fee. |
| `fee_recipient` | `AccountId` | Account that receives the fee collected for this order. |
| `relayer`       | `Option<AccountId>` | If `Some`, restricts execution to a single designated relayer account. Any attempt by a different account to execute this order is rejected with `RelayerMissMatch`. `None` = any relayer may execute. |
| `max_slippage`  | `Option<Perbill>`   | Maximum acceptable slippage in parts per billion applied to `limit_price` at swap time. `None` = no slippage protection (execute at market). When `Some(p)`: Buy ceiling = `limit_price + limit_price * p`; Sell floor = `limit_price - limit_price * p`. Both saturate at `u64` bounds. |

### `OrderType`

| Variant      | Action        | Triggers when           | Use case |
|--------------|---------------|-------------------------|----------|
| `LimitBuy`   | Buy alpha      | price ≤ `limit_price`  | Enter a position at or below a target price. |
| `TakeProfit` | Sell alpha     | price ≥ `limit_price`  | Exit a position once price rises to a profit target. |
| `StopLoss`   | Sell alpha     | price ≤ `limit_price`  | Exit a position to limit downside if price falls to a floor. |

### `SignedOrder<AccountId>`

Envelope submitted by the relayer: the `VersionedOrder` payload plus the user's
sr25519 signature over the SCALE encoding of the `VersionedOrder` (including the
version discriminant). Only sr25519 signatures are accepted. Signature
verification uses the inner `order.signer` as the expected public key.

### `OrderStatus`

Terminal state of a processed order, stored under its `OrderId`.

| Variant     | Meaning |
|-------------|---------|
| `Fulfilled` | Order was successfully executed. |
| `Cancelled` | User registered a cancellation intent before execution. |

---

## Storage

### `Orders: StorageMap<H256, OrderStatus>`

Maps an `OrderId` (blake2_256 of the SCALE-encoded `VersionedOrder`) to its
terminal `OrderStatus`. Absence means the order has never been seen and is still
executable (provided it is valid). Presence means it is permanently closed —
neither `Fulfilled` nor `Cancelled` orders can be re-executed.

---

## Config

| Item                  | Type                                              | Description |
|-----------------------|---------------------------------------------------|-------------|
| `SwapInterface`       | `OrderSwapInterface<Self::AccountId>`             | Full swap + balance execution interface. Implemented by `pallet_subtensor::Pallet<T>`. Provides `buy_alpha`, `sell_alpha`, `transfer_tao`, `transfer_staked_alpha`, and `current_alpha_price`. |
| `TimeProvider`        | `UnixTime`                                        | Current wall-clock time for expiry checks. |
| `MaxOrdersPerBatch`   | `Get<u32>` (constant)                             | Maximum number of orders accepted in a single `execute_orders` or `execute_batched_orders` call. Should equal `floor(max_block_weight / per_order_weight)`. |
| `PalletId`            | `Get<PalletId>` (constant)                        | Used to derive the pallet intermediary account (`PalletId::into_account_truncating`). This account temporarily holds pooled TAO and staked alpha during `execute_batched_orders`. |
| `PalletHotkey`        | `Get<Self::AccountId>` (constant)                 | Hotkey the pallet intermediary account stakes to/from during batch execution. Must be a dedicated hotkey registered on every subnet the pallet may operate on. Operators should register it as a non-validator neuron. |
| `WeightInfo`          | `weights::WeightInfo`                             | Benchmarked weight functions for each extrinsic. Use `weights::SubstrateWeight<Runtime>` in production and `()` in tests. |

---

## Extrinsics

### `execute_orders(orders)` — call index 0

**Origin:** any signed account (typically a relayer).

Executes a list of signed limit orders one by one, each interacting with the
AMM pool independently. Orders that fail validation or whose price condition is
not met are silently skipped — a single bad order does not revert the batch.

**Fee handling:** each order's `fee_rate` is deducted from the input amount and
forwarded to that order's `fee_recipient` after execution.

**When to use:** suitable for small batches or when orders target different
subnets. Use `execute_batched_orders` for same-subnet batches to reduce price
impact.

---

### `execute_batched_orders(netuid, orders)` — call index 1

**Origin:** any signed account (typically a relayer).

Aggregates all valid orders targeting `netuid` into a single net pool
interaction:

1. **Validate & classify** — if any order has the wrong netuid, an invalid
   signature, an already-processed id, a past expiry, a price condition not met,
   or targets the root netuid (0), the **entire call fails** with the
   corresponding error. All orders must be valid for execution to proceed. Valid
   orders are split into buy-side (`LimitBuy`) and sell-side (`TakeProfit`,
   `StopLoss`) groups. For buy orders the net TAO (after fee) is pre-computed
   here. Each order's `effective_swap_limit` (derived from `limit_price` and
   `max_slippage`) is computed and stored for use in the pool swap.

2. **Collect assets** — gross TAO is pulled from each buyer's free balance into
   the pallet intermediary account. Gross alpha stake is moved from each seller's
   `(coldkey, hotkey)` position to the pallet intermediary's `(pallet_account,
   pallet_hotkey)` position.

3. **Net pool swap** — buy TAO and sell alpha are converted to a common TAO
   basis at the current spot price and offset against each other. Only the
   residual amount touches the pool in a single swap:
   - Buy-dominant: residual TAO is sent to the pool; pool returns alpha. Price ceiling = `min(effective_swap_limit)` across all buy orders.
   - Sell-dominant: residual alpha is sent to the pool; pool returns TAO. Price floor = `max(effective_swap_limit)` across all sell orders.
   - Perfectly offset: no pool interaction.

4. **Distribute alpha pro-rata** — every buyer receives their share of the total
   available alpha (pool output + seller passthrough alpha). Share is
   proportional to each buyer's net TAO contribution. Integer division floors
   each share; any remainder stays in the pallet intermediary account as dust.

5. **Distribute TAO pro-rata** — every seller receives their share of the total
   available TAO (pool output + buyer passthrough TAO), minus their order's
   fee. Share is proportional to each seller's alpha valued at the current spot
   price. Integer division floors each share; any remainder stays in the pallet
   intermediary account as dust.

6. **Collect fees** — buy-side fees (withheld from each order's TAO input) and
   sell-side fees (withheld from each order's TAO output) are accumulated per
   unique `fee_recipient` and forwarded in a single transfer per recipient.

7. **Emit `GroupExecutionSummary`.**

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

## Events

| Event | Fields | Emitted when |
|-------|--------|--------------|
| `OrderExecuted` | `order_id`, `signer`, `netuid`, `side` | An individual order was successfully executed (by either extrinsic). |
| `OrderSkipped` | `order_id`, `reason` | An order was skipped by `execute_orders` (bad signature, expired, wrong netuid, already processed, price condition not met, or root netuid). `reason` is the `DispatchError` that caused the skip. Not emitted by `execute_batched_orders` — invalid orders there cause the whole call to fail. |
| `OrderCancelled` | `order_id`, `signer` | The signer registered a cancellation via `cancel_order`. |
| `GroupExecutionSummary` | `netuid`, `net_side`, `net_amount`, `actual_out`, `executed_count` | Emitted once per `execute_batched_orders` call summarising the net pool trade. `net_side` is `Buy` if TAO was sent to the pool, `Sell` if alpha was sent. `net_amount` and `actual_out` are zero when the two sides perfectly offset. |

---

## Errors

| Error | Cause |
|-------|-------|
| `InvalidSignature` | Signature does not match the order payload and signer. Also used as a catch-all for failed validation in `execute_orders`. |
| `OrderAlreadyProcessed` | The `OrderId` is already present in `Orders` (either `Fulfilled` or `Cancelled`). |
| `OrderExpired` | `now > order.expiry`. Only returned as a hard error by `execute_batched_orders`; silently skipped in `execute_orders`. |
| `PriceConditionNotMet` | Current spot price is beyond the order's `limit_price`. Only returned as a hard error by `execute_batched_orders`; silently skipped in `execute_orders`. |
| `OrderNetUidMismatch` | An order inside a `execute_batched_orders` call targets a different netuid than the batch parameter. |
| `RootNetUidNotAllowed` | The order or batch targets netuid 0 (root). Root uses a fixed 1:1 stable mechanism with no AMM — limit orders are not meaningful there. |
| `Unauthorized` | Caller of `cancel_order` is not the order's `signer`. |
| `SwapReturnedZero` | The pool swap returned zero output for a non-zero residual input. |
| `RelayerMissMatch` | The caller is not the relayer designated in the order's `relayer` field. Only raised when the field is `Some`. |

---

## Fee model

Fees are specified per-order via `fee_rate: Perbill` and `fee_recipient:
AccountId` fields on the `Order` struct. There is no global protocol fee or
admin key.

All fees are collected in TAO regardless of order side.

| Order type              | Fee deducted from | Timing |
|-------------------------|-------------------|--------|
| `LimitBuy`              | TAO input         | Pre-computed in `validate_and_classify`, before pool swap. |
| `TakeProfit`, `StopLoss`| TAO output        | Deducted in `distribute_tao_pro_rata`, after pool swap. |

Fee formula: `fee = fee_rate * amount` (using `Perbill` multiplication, which
upcasts to u128 internally to avoid overflow).

At the end of each batch, fees are accumulated per unique `fee_recipient` and
forwarded in a single transfer per recipient. If multiple orders share the same
`fee_recipient`, they result in exactly one transfer rather than one per order.

---

## Known limitations

### `max_slippage` is semantically inverted for `StopLoss` orders

`StopLoss` sells are triggered when the spot price *falls* to `limit_price`.
`max_slippage` derives a sell floor as `limit_price - limit_price * slippage`,
which is computed from the (higher) trigger threshold. By the time the order
fires, the actual market price will typically be **below** `limit_price`, so
the derived floor will almost always exceed the real fill price, causing the
swap to be rejected.

**Consequence:** Applying `max_slippage` to a `StopLoss` order will usually
prevent it from executing. In `execute_orders` the order is silently skipped;
in `execute_batched_orders` the entire batch fails.

**Recommendation:** Relayers should set `max_slippage: None` on `StopLoss`
orders. If slippage protection is desired, apply it at the relayer layer by
choosing a conservative `limit_price` rather than relying on `max_slippage`.
