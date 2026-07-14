"""Chain error descriptions declared (first) by the `LimitOrders` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "ArithmeticOverflow": (
        "Converting a TAO amount to alpha during batched order execution overflowed the "
        "fixed-point range, typically when the pool price is tiny relative to the batch's total "
        "buy TAO. Check the subnet's current alpha price against the batch's aggregate buy "
        "amounts."
    ),
    "ChainIdMismatch": (
        "The order payload's `chain_id` field differs from this chain's configured EVM chain "
        "id, e.g. an order signed for testnet was submitted to mainnet. Compare the order's "
        "`chain_id` with the runtime's `pallet_evm_chain_id` value and re-sign if needed."
    ),
    "DuplicateOrderInBatch": (
        "Two entries in one `execute_batched_orders` call hash to the same order id, meaning "
        "the identical signed payload was included twice, which hard-fails the batch. "
        "Deduplicate by the blake2-256 hash of each SCALE-encoded `VersionedOrder`."
    ),
    "IncorrectPartialFillAmount": (
        "The `partial_fill` amount is zero or exceeds the order's remaining unfilled amount, or "
        "a full execution was submitted against an order already partially filled. Compare "
        "`partial_fill` with `order.amount` minus the filled amount recorded in `Orders`."
    ),
    "LimitOrdersDisabled": (
        "Order execution was attempted while the pallet's global switch is off. Check the "
        "`LimitOrdersEnabled` storage value; root must call `set_pallet_status` with true to "
        "enable the pallet."
    ),
    "OrderAlreadyProcessed": (
        "The order id already has a terminal status: execution found it fulfilled, or "
        "`cancel_order` found any existing status for it. Check the `Orders` storage map under "
        "the blake2-256 hash of the SCALE-encoded `VersionedOrder`."
    ),
    "OrderCancelled": (
        "The order was previously cancelled via `cancel_order` and can never be executed. Check "
        "the `Orders` storage entry for the order id; a `Cancelled` status is terminal, so the "
        "signer must sign and submit a fresh order."
    ),
    "OrderExpired": (
        "The current chain time is past the order's `expiry` field, which is a unix timestamp "
        "in milliseconds, so the order can no longer execute. Compare the `expiry` in the "
        "signed order payload with the chain's current `Timestamp` value."
    ),
    "OrderNetUidMismatch": (
        "An order inside an `execute_batched_orders` call has a `netuid` field different from "
        "the batch's `netuid` parameter, which hard-fails the entire batch. Check each order "
        "payload's `netuid` against the batch argument and split mismatched orders out."
    ),
    "PalletHotkeyNotRegistered": (
        "Root tried to enable the pallet via `set_pallet_status` before its hotkey was "
        "registered to the pallet's intermediary account. Check that the `PalletHotkey` "
        "constant is registered for the pallet account, which genesis or the "
        "`on_runtime_upgrade` migration performs."
    ),
    "PartialFillsNotEnabled": (
        "A `partial_fill` amount was supplied for an order whose signed payload has "
        "`partial_fills_enabled` set to false. Check that field in the order payload; partial "
        "execution requires the signer to have opted in when signing."
    ),
    "PriceConditionNotMet": (
        "The subnet's current alpha price does not satisfy the order's trigger: buys and "
        "stop-losses require price at or below `limit_price`, take-profits at or above it. "
        "Compare `current_alpha_price` for the order's `netuid`, scaled by 1e9, with the "
        "`limit_price` field."
    ),
    "RelayerMissMatch": (
        "The order's `relayer` allowlist is set but the account that submitted the execution "
        "transaction is not in it. Compare the extrinsic's signing account against the "
        "`relayer` list in the signed order payload."
    ),
    "RelayerRequiredForPartialFill": (
        "A `partial_fill` was requested for an order whose `relayer` field is empty; partial "
        "fills are only permitted on orders that restrict who may execute them. Check the order "
        "payload and either set a relayer list or execute the full amount."
    ),
    "RootNetUidNotAllowed": (
        "The order or batch targets the root subnet, netuid 0, which the limit orders pallet "
        "does not serve. Check the `netuid` field of the order payload or the `netuid` "
        "parameter of the batch call and target a non-root subnet."
    ),
    "SwapReturnedZero": (
        "The netted pool swap in `execute_batched_orders` produced zero output for a non-zero "
        "input, meaning the pool lacks liquidity or the derived price limit clamped the swap "
        "entirely. Check the subnet pool's reserves and the batch's tightest slippage-derived "
        "price limit."
    ),
    "ZeroShareInBatch": (
        "An order's pro-rata share of the batch output floored to zero, so the whole batch was "
        "rejected rather than consuming that order's input for no payout. Check the order's "
        "`amount` relative to the batch totals and retry it in a differently composed batch."
    ),
}
