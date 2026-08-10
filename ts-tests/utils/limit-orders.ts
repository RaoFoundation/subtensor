import type { KeyringPair } from "@moonwall/util";
import type { TypedApi } from "polkadot-api";
import type { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import { stringToU8a, u8aToHex, u8aWrapBytes } from "@polkadot/util";
import { blake2AsHex, blake2AsU8a, decodeAddress, encodeAddress } from "@polkadot/util-crypto";
import { waitForTransactionWithRetry } from "./transactions.js";
import { MultiAddress } from "@polkadot-api/descriptors";

// ── Types ─────────────────────────────────────────────────────────────────────

export type OrderType = "LimitBuy" | "TakeProfit" | "StopLoss";

export interface OrderParams {
    signer: KeyringPair;
    hotkey: string;
    netuid: number;
    orderType: OrderType;
    amount: bigint;
    limitPrice: bigint;
    expiry: bigint;
    feeRate: number; // Perbill (parts per billion), e.g. 10_000_000 = 1%
    feeRecipient: string;
    chainId?: bigint; // defaults to 42n (the dev node's EVM chain ID)
    relayer?: string[] | null; // Optional: if set, only these accounts may relay the order
    maxSlippage?: number | null; // Optional: Perbill (ppb). When set, effective swap limit = limit_price ± limit_price * maxSlippage / 1e9
    partialFillsEnabled?: boolean; // Optional: if true, order can be partially filled (requires relayer)
}

export interface Order {
    signer: string;
    hotkey: string;
    netuid: number;
    order_type: OrderType;
    amount: bigint;
    limit_price: bigint;
    expiry: bigint;
    fee_rate: number;
    fee_recipient: string;
    relayer: string[] | null;
    max_slippage: number | null;
    chain_id: bigint;
    partial_fills_enabled: boolean;
}

/**
 * v2 order amount: either an absolute raw amount (v1 semantics) or a fraction of
 * the output another order recorded — a "linked" order.
 *
 * `pct` is a Perbill (parts per billion), so 250_000_000 is 25%.  `provider` is the
 * `OrderId` of the order being drawn from, i.e. `orderId(api, providerOrder)`.
 */
export type OrderAmount = { Fixed: bigint } | { LinkedPercentage: { provider: `0x${string}`; pct: number } };

/**
 * The v2 order payload.  Field-for-field identical to {@link Order} except that
 * `amount` is an {@link OrderAmount} and `has_linked_order` is new.
 *
 * FIELD ORDER MATTERS: it must match `OrderV2` in `pallets/limit-orders/src/v2.rs`,
 * because the signature is over the SCALE encoding built from the registry type.
 */
export interface OrderV2 {
    signer: string;
    hotkey: string;
    netuid: number;
    order_type: OrderType;
    amount: OrderAmount;
    limit_price: bigint;
    expiry: bigint;
    fee_rate: number;
    fee_recipient: string;
    relayer: string[] | null;
    max_slippage: number | null;
    chain_id: bigint;
    partial_fills_enabled: boolean;
    /** Record this order's output so the linked order that names it can draw on it. */
    has_linked_order: boolean;
}

export interface OrderV2Params extends Omit<OrderParams, "amount"> {
    /** Either `{ Fixed: n }` or `{ LinkedPercentage: { provider, pct } }`. */
    amount: OrderAmount;
    hasLinkedOrder?: boolean;
}

export type VersionedOrder = { V1: Order } | { V2: OrderV2 };

export interface SignedOrder {
    order: VersionedOrder;
    signature: { Sr25519: `0x${string}` } | { Ed25519: `0x${string}` } | { Ecdsa: `0x${string}` };
    partial_fill: number | null;
}

/**
 * Narrow a `VersionedOrder` to its v1 payload.
 *
 * Throws on a version mismatch rather than returning `undefined`: every call site
 * built the order itself and so knows its version, and an `undefined` return would
 * only be dealt with by a non-null assertion.
 */
export function asV1(order: VersionedOrder): Order {
    if (!("V1" in order)) {
        throw new Error("expected a V1 order");
    }
    return order.V1;
}

/** Narrow a `VersionedOrder` to its v2 payload.  Throws on a version mismatch. */
export function asV2(order: VersionedOrder): OrderV2 {
    if (!("V2" in order)) {
        throw new Error("expected a V2 order");
    }
    return order.V2;
}

/** The `LinkedAsset` a provider record is stamped with, as decoded from storage. */
export type LinkedAsset = "Tao" | { netuid: number; hotkey: string };

/** A decoded `LinkedOutputs` entry. */
export interface LinkedOutput {
    signer: string;
    asset: LinkedAsset;
    total: bigint;
    expires_at: bigint;
}

// ── Constants ─────────────────────────────────────────────────────────────────

export const PERBILL_ONE_PERCENT = 10_000_000;
export const FAR_FUTURE = BigInt("18446744073709551615"); // u64::MAX
export const EXPIRED = BigInt(1); // 1ms — always in the past

// ── Order building & signing ──────────────────────────────────────────────────

/**
 * Build the `VersionedOrder` (V1) struct from the supplied params.  Shared by
 * `buildSignedOrder` (raw signing) and `buildWrappedSignedOrder` (Ledger /
 * signRaw `<Bytes>`-wrapped signing) so the field mapping stays identical.
 */
function buildVersionedOrder(params: OrderParams): VersionedOrder {
    const inner: Order = {
        signer: params.signer.address,
        hotkey: params.hotkey,
        netuid: params.netuid,
        order_type: params.orderType,
        amount: params.amount,
        limit_price: params.limitPrice,
        expiry: params.expiry,
        fee_rate: params.feeRate,
        fee_recipient: params.feeRecipient,
        relayer: params.relayer ?? null,
        max_slippage: params.maxSlippage ?? null,
        chain_id: params.chainId ?? 42n,
        partial_fills_enabled: params.partialFillsEnabled ?? false,
    };

    return { V1: inner };
}

/**
 * Build the `VersionedOrder` (V2) struct from the supplied params.  Shared by the
 * three v2 signing helpers so the field mapping stays identical.
 */
function buildVersionedOrderV2(params: OrderV2Params): VersionedOrder {
    const inner: OrderV2 = {
        signer: params.signer.address,
        hotkey: params.hotkey,
        netuid: params.netuid,
        order_type: params.orderType,
        amount: params.amount,
        limit_price: params.limitPrice,
        expiry: params.expiry,
        fee_rate: params.feeRate,
        fee_recipient: params.feeRecipient,
        relayer: params.relayer ?? null,
        max_slippage: params.maxSlippage ?? null,
        chain_id: params.chainId ?? 42n,
        partial_fills_enabled: params.partialFillsEnabled ?? false,
        has_linked_order: params.hasLinkedOrder ?? false,
    };

    return { V2: inner };
}

/**
 * Build a SignedOrder ready for submission to execute_orders /
 * execute_batched_orders.  The Order struct is SCALE-encoded via the
 * polkadot.js registry and then signed with the signer's sr25519 key.
 */
export function buildSignedOrder(api: any, params: OrderParams): SignedOrder {
    const versionedOrder = buildVersionedOrder(params);

    // SCALE-encode the VersionedOrder so the signature covers the version tag.
    const encoded = api.registry.createType("LimitVersionedOrder", versionedOrder);
    const sig = params.signer.sign(encoded.toU8a());

    return {
        order: versionedOrder,
        signature: { Sr25519: u8aToHex(sig) as `0x${string}` },
        partial_fill: null,
    };
}

/**
 * v2 counterpart of {@link buildSignedOrder}: signature directly over the
 * SCALE-encoded `VersionedOrder::V2`.
 */
export function buildSignedOrderV2(api: any, params: OrderV2Params): SignedOrder {
    const versionedOrder = buildVersionedOrderV2(params);

    const encoded = api.registry.createType("LimitVersionedOrder", versionedOrder);
    const sig = params.signer.sign(encoded.toU8a());

    return {
        order: versionedOrder,
        signature:
            params.signer.type === "ed25519"
                ? { Ed25519: u8aToHex(sig) as `0x${string}` }
                : { Sr25519: u8aToHex(sig) as `0x${string}` },
        partial_fill: null,
    };
}

/**
 * v2 counterpart of {@link buildWrappedSignedOrder}: signature over
 * `<Bytes>` ++ blake2_256(SCALE(VersionedOrder::V2)) ++ `</Bytes>`.
 */
export function buildWrappedSignedOrderV2(api: any, params: OrderV2Params): SignedOrder {
    const versionedOrder = buildVersionedOrderV2(params);

    const encoded = api.registry.createType("LimitVersionedOrder", versionedOrder);
    const wrapped = u8aWrapBytes(blake2AsU8a(encoded.toU8a(), 256));
    const sig = params.signer.sign(wrapped);

    return {
        order: versionedOrder,
        signature:
            params.signer.type === "ed25519"
                ? { Ed25519: u8aToHex(sig) as `0x${string}` }
                : { Sr25519: u8aToHex(sig) as `0x${string}` },
        partial_fill: null,
    };
}

/**
 * Build a SignedOrder whose signature is over the `<Bytes>`-wrapped order hash
 * (the Ledger / `signRaw` form).  This exercises the runtime's alternative
 * verification path:
 *
 *     signature.verify(b"<Bytes>" ++ blake2_256(SCALE(VersionedOrder)) ++ b"</Bytes>", signer)
 *
 * The signed payload is the raw 32-byte blake2-256 hash of the SCALE-encoded
 * VersionedOrder, wrapped by `u8aWrapBytes` (which prepends `<Bytes>` and
 * appends `</Bytes>`).  This is byte-for-byte what the runtime reconstructs
 * from `order_id.as_bytes()`, so the hash must be wrapped raw — never
 * hex-encoded before wrapping.
 *
 * The signature scheme tag (`Sr25519` vs `Ed25519`) follows the signer's
 * keypair type, so the same helper works for both schemes.
 */
export function buildWrappedSignedOrder(api: any, params: OrderParams): SignedOrder {
    const versionedOrder = buildVersionedOrder(params);

    // SCALE-encode the VersionedOrder, then hash it (this is the OrderId).
    const encoded = api.registry.createType("LimitVersionedOrder", versionedOrder);
    const hash = blake2AsU8a(encoded.toU8a(), 256);

    // Wrap the raw 32-byte hash in the signRaw envelope: <Bytes>..hash..</Bytes>.
    const wrapped = u8aWrapBytes(hash);
    const sig = params.signer.sign(wrapped);

    // Tag the signature variant from the keypair type.
    const signature =
        params.signer.type === "ed25519"
            ? { Ed25519: u8aToHex(sig) as `0x${string}` }
            : { Sr25519: u8aToHex(sig) as `0x${string}` };

    return {
        order: versionedOrder,
        signature,
        partial_fill: null,
    };
}

// ── Human-readable ("clear-signing" / Ledger) message ──────────────────────────

/**
 * SS58 prefix under which all account fields are rendered in the canonical
 * human-readable message.  MUST match the pallet's `SS58_PREFIX` constant (42).
 */
export const READABLE_SS58_PREFIX = 42;

/**
 * Ledger's raw-signing size limit — `MAX_SIGN_SIZE` in the Zondax Polkadot app.
 * MUST match the pallet's `LEDGER_MAX_SIGN_SIZE`.
 *
 * A `signRaw` payload longer than this is blake2_256-hashed on-device before the
 * signature is produced, so for an oversized payload the signature commits to the
 * hash rather than to the payload bytes, and the runtime verifies it that way.
 * The device still displays the full message — the hashing happens in the signing
 * step only.
 */
export const LEDGER_MAX_SIGN_SIZE = 256;

/**
 * Re-encode an account address as SS58 at prefix 42.  Accepts any input the
 * `@polkadot/util-crypto` `decodeAddress` understands (SS58 of any prefix, hex,
 * or raw bytes) and always re-encodes so the output prefix is deterministic —
 * matching the runtime's `render_account`, which always renders at prefix 42.
 */
function renderAccount(addr: string): string {
    return encodeAddress(decodeAddress(addr), READABLE_SS58_PREFIX);
}

/**
 * Format the canonical human-readable ("clear-signing") message for an order.
 *
 * This is a PURE function of the order's V1 fields and MUST match the runtime's
 * `Pallet::render_order` byte-for-byte — the runtime rebuilds this exact string
 * and verifies the signature over `<Bytes>` ++ utf8(message) ++ `</Bytes>`.  Any
 * drift here silently breaks signature verification.
 *
 * Canonical form (single line, `, ` between fields):
 *
 *   TAO.com order v1: {LABEL} {amount} on subnet {netuid}, {PRICE_WORD} {limit_price},
 *   expiry {expiry}, hotkey {hotkey}, fee {fee_rate} to {fee_recipient},
 *   relayer {relayer}, max slippage {max_slippage}, chain {chain_id},
 *   partial fills {partial}, signer {signer}
 */
export function formatOrderMessage(order: Order): string {
    const label =
        order.order_type === "LimitBuy" ? "Limit buy" : order.order_type === "TakeProfit" ? "Take-profit" : "Stop-loss";

    const priceWord = order.order_type === "LimitBuy" ? "limit price" : "trigger price";

    const maxSlippage = order.max_slippage === null ? "none" : order.max_slippage.toString();

    let relayer: string;
    if (order.relayer === null) {
        relayer = "none";
    } else if (order.relayer.length === 0) {
        relayer = "[]";
    } else {
        relayer = order.relayer.map(renderAccount).join("+");
    }

    return (
        `TAO.com order v1: ${label} ${order.amount.toString()} on subnet ${order.netuid.toString()}, ` +
        `${priceWord} ${order.limit_price.toString()}, expiry ${order.expiry.toString()}, ` +
        `hotkey ${renderAccount(order.hotkey)}, ` +
        `fee ${order.fee_rate.toString()} to ${renderAccount(order.fee_recipient)}, ` +
        `relayer ${relayer}, ` +
        `max slippage ${maxSlippage}, chain ${order.chain_id.toString()}, ` +
        `partial fills ${order.partial_fills_enabled ? "true" : "false"}, ` +
        `signer ${renderAccount(order.signer)}`
    );
}

/**
 * Render an {@link OrderAmount} for the canonical human-readable message.
 *
 * MUST match `OrderAmount::render` in `pallets/limit-orders/src/v2.rs`:
 *
 *   - `Fixed(n)`              → `n`                                  (bare digits)
 *   - `LinkedPercentage{p,x}` → `{x} ppb of order 0x{64 hex} output`
 *
 * The fraction stays in raw parts-per-billion rather than a rendered percentage:
 * integer-to-decimal is trivially reproducible here and in the Ledger app, whereas a
 * decimal-percent algorithm is not, and the string is consensus-critical.  The `ppb`
 * suffix is also what keeps the two variants injective — no bare amount can produce
 * it, so a fixed-amount signature can never be replayed as a linked one.
 *
 * The provider id is rendered as 64 LOWERCASE hex characters with a `0x` prefix,
 * matching Rust's `HexDisplay`.
 */
export function formatOrderAmount(amount: OrderAmount): string {
    if ("Fixed" in amount) {
        return amount.Fixed.toString();
    }
    const { provider, pct } = amount.LinkedPercentage;
    const hex = provider.replace(/^0x/, "").toLowerCase();
    return `${pct.toString()} ppb of order 0x${hex} output`;
}

/**
 * Format the canonical human-readable ("clear-signing") message for a v2 order.
 *
 * MUST match the runtime's `Pallet::render_order` byte-for-byte.  Identical to the
 * v1 form except for three things:
 *
 *   1. the version tag is `v2`, which alone stops a v1 signature being replayed as
 *      a v2 order (and vice versa);
 *   2. the amount slot goes through {@link formatOrderAmount}, so a linked order
 *      renders its provider and fraction instead of a bare number;
 *   3. a `, has-linked-order {true|false}` tail is appended.  v1 renders NO tail —
 *      that is why {@link formatOrderMessage} is a separate function rather than a
 *      parameterised one, since every v1 message must stay byte-identical to what
 *      it was before v2 existed.
 *
 * The tail is load-bearing, not decorative: `has_linked_order` authorises recording
 * the order's output for a linked order to spend.  Were it absent from the message,
 * a signature over a readable non-provider order would transplant onto the same
 * order with the flag set, and the recomputed message would still match.
 */
export function formatOrderMessageV2(order: OrderV2): string {
    const label =
        order.order_type === "LimitBuy" ? "Limit buy" : order.order_type === "TakeProfit" ? "Take-profit" : "Stop-loss";

    const priceWord = order.order_type === "LimitBuy" ? "limit price" : "trigger price";

    const maxSlippage = order.max_slippage === null ? "none" : order.max_slippage.toString();

    let relayer: string;
    if (order.relayer === null) {
        relayer = "none";
    } else if (order.relayer.length === 0) {
        relayer = "[]";
    } else {
        relayer = order.relayer.map(renderAccount).join("+");
    }

    return (
        `TAO.com order v2: ${label} ${formatOrderAmount(order.amount)} on subnet ${order.netuid.toString()}, ` +
        `${priceWord} ${order.limit_price.toString()}, expiry ${order.expiry.toString()}, ` +
        `hotkey ${renderAccount(order.hotkey)}, ` +
        `fee ${order.fee_rate.toString()} to ${renderAccount(order.fee_recipient)}, ` +
        `relayer ${relayer}, ` +
        `max slippage ${maxSlippage}, chain ${order.chain_id.toString()}, ` +
        `partial fills ${order.partial_fills_enabled ? "true" : "false"}, ` +
        `signer ${renderAccount(order.signer)}, ` +
        `has-linked-order ${order.has_linked_order ? "true" : "false"}`
    );
}

/**
 * v2 counterpart of {@link buildReadableSignedOrder}: signature over the
 * `<Bytes>`-wrapped canonical readable message, blake2_256-hashed first when it
 * exceeds the device's raw-signing limit (which it always does in practice).
 */
export function buildReadableSignedOrderV2(api: any, params: OrderV2Params): SignedOrder {
    const versionedOrder = buildVersionedOrderV2(params);
    const v2 = asV2(versionedOrder);

    const wrapped = u8aWrapBytes(stringToU8a(formatOrderMessageV2(v2)));
    const signedBytes = wrapped.length > LEDGER_MAX_SIGN_SIZE ? blake2AsU8a(wrapped, 256) : wrapped;
    const sig = params.signer.sign(signedBytes);

    return {
        order: versionedOrder,
        signature:
            params.signer.type === "ed25519"
                ? { Ed25519: u8aToHex(sig) as `0x${string}` }
                : { Sr25519: u8aToHex(sig) as `0x${string}` },
        partial_fill: null,
    };
}

/**
 * Build a SignedOrder whose signature is over the `<Bytes>`-wrapped canonical
 * human-readable message (the "clear-signing" / Ledger form that a hardware
 * wallet can display field-by-field).  This exercises the runtime's
 * `verify_readable` path:
 *
 *     signature.verify(b"<Bytes>" ++ utf8(render_order(order)) ++ b"</Bytes>", signer)
 *
 * IMPORTANT: the message is converted to BYTES with `stringToU8a` and then
 * wrapped with `u8aWrapBytes`, so the signed payload is exactly
 * `<Bytes>` ++ utf8(message) ++ `</Bytes>` — matching the runtime's
 * `[b"<Bytes>", &render_order, b"</Bytes>"].concat()`.  Wrapping the raw string
 * instead of the bytes would corrupt the payload.
 *
 * The bytes actually signed then follow the device's rule: a payload longer than
 * `LEDGER_MAX_SIGN_SIZE` is blake2_256-hashed first, because that is what a Ledger
 * signs and therefore what the runtime verifies.  The readable message is always
 * oversized (three SS58 addresses alone are 144 characters), so this emulates a
 * hardware signer.  Note that `signRaw` in a *software* wallet (polkadot.js
 * extension) does NOT hash — such a signature is not valid on this path.
 *
 * The signature scheme tag (`Sr25519` vs `Ed25519`) follows the signer's
 * keypair type, so the same helper works for both schemes.
 */
export function buildReadableSignedOrder(api: any, params: OrderParams): SignedOrder {
    const versionedOrder = buildVersionedOrder(params);

    // Render the canonical message, convert to UTF-8 bytes, then wrap.
    // `buildVersionedOrder` always produces a V1, so the narrowing cannot fail.
    const message = formatOrderMessage(asV1(versionedOrder));
    const wrapped = u8aWrapBytes(stringToU8a(message));
    const signedBytes = wrapped.length > LEDGER_MAX_SIGN_SIZE ? blake2AsU8a(wrapped, 256) : wrapped;
    const sig = params.signer.sign(signedBytes);

    // Tag the signature variant from the keypair type.
    const signature =
        params.signer.type === "ed25519"
            ? { Ed25519: u8aToHex(sig) as `0x${string}` }
            : { Sr25519: u8aToHex(sig) as `0x${string}` };

    return {
        order: versionedOrder,
        signature,
        partial_fill: null,
    };
}

/**
 * Compute the on-chain OrderId (blake2_256 of SCALE-encoded VersionedOrder).
 * Mirrors `Pallet::derive_order_id` in Rust.
 */
export function orderId(api: any, order: VersionedOrder): `0x${string}` {
    const encoded = api.registry.createType("LimitVersionedOrder", order);
    return blake2AsHex(encoded.toU8a(), 256) as `0x${string}`;
}

// ── Registry ──────────────────────────────────────────────────────────────────

/**
 * Register the custom SCALE types used by pallet-limit-orders with the
 * polkadot.js ApiPromise registry.  Call this once after obtaining the api.
 */
export function registerLimitOrderTypes(api: any): void {
    api.registry.register({
        LimitOrderType: {
            _enum: ["LimitBuy", "TakeProfit", "StopLoss"],
        },
        LimitOrder: {
            signer: "AccountId",
            hotkey: "AccountId",
            netuid: "u16",
            order_type: "LimitOrderType",
            amount: "u64",
            limit_price: "u64",
            expiry: "u64",
            fee_rate: "u32", // Perbill
            fee_recipient: "AccountId",
            relayer: "Option<Vec<AccountId>>",
            max_slippage: "Option<u32>",
            chain_id: "u64",
            partial_fills_enabled: "bool",
        },
        // `OrderAmount::LinkedPercentage` is a struct variant, so it is modelled as a
        // nested struct — the SCALE bytes are identical (variant index, then fields in
        // declaration order).
        LimitLinkedPercentage: {
            provider: "H256",
            pct: "u32", // Perbill
        },
        LimitOrderAmount: {
            _enum: {
                Fixed: "u64",
                LinkedPercentage: "LimitLinkedPercentage",
            },
        },
        // Field order must match `OrderV2` in pallets/limit-orders/src/v2.rs — the
        // signature is over this encoding, so a reordering silently breaks it.
        LimitOrderV2: {
            signer: "AccountId",
            hotkey: "AccountId",
            netuid: "u16",
            order_type: "LimitOrderType",
            amount: "LimitOrderAmount",
            limit_price: "u64",
            expiry: "u64",
            fee_rate: "u32", // Perbill
            fee_recipient: "AccountId",
            relayer: "Option<Vec<AccountId>>",
            max_slippage: "Option<u32>",
            chain_id: "u64",
            partial_fills_enabled: "bool",
            has_linked_order: "bool",
        },
        // Variant indices are part of the signed payload: V1 stays 0 so every
        // already-signed v1 order keeps verifying, and V2 takes 1.
        LimitVersionedOrder: {
            _enum: {
                V1: "LimitOrder",
                V2: "LimitOrderV2",
            },
        },
        LimitSignedOrder: {
            order: "LimitVersionedOrder",
            signature: "MultiSignature",
            partial_fill: "Option<u64>",
        },
        LimitOrderStatus: {
            _enum: {
                Fulfilled: null,
                PartiallyFilled: "u64",
                Cancelled: null,
            },
        },
    });
}

// ── Chain helpers ─────────────────────────────────────────────────────────────

/** Read current SubnetTAO and SubnetAlphaIn to derive spot price (TAO per alpha). */
export async function getAlphaPrice(api: TypedApi<typeof subtensor>, netuid: number): Promise<bigint> {
    const taoReserve = await api.query.SubtensorModule.SubnetTAO.getValue(netuid);
    const alphaIn = await api.query.SubtensorModule.SubnetAlphaIn.getValue(netuid);
    if (alphaIn === 0n) return 0n;
    return taoReserve / alphaIn; // integer approximation
}

/** Enable the subtoken for a subnet (required for swaps to work). */
export async function enableSubtoken(api: TypedApi<typeof subtensor>, netuid: number): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_subtoken_enabled({
        netuid,
        subtoken_enabled: true,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_subtoken_enabled");
}

/** Sudo-enable or disable the limit-orders pallet. */
export async function setPalletStatus(api: TypedApi<typeof subtensor>, enabled: boolean): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const tx = api.tx.Sudo.sudo({
        call: api.tx.LimitOrders.set_pallet_status({ enabled }).decodedCall,
    });
    await waitForTransactionWithRetry(api, tx, alice, "set_pallet_status");
}

/** Read the on-chain OrderStatus for a given order id (hex). */
export async function getOrderStatus(
    polkadotJs: any,
    id: `0x${string}`
): Promise<"Fulfilled" | "PartiallyFilled" | "Cancelled" | undefined> {
    const result = await polkadotJs.query.limitOrders.orders(id);
    if (result.isNone) return undefined;
    return result.unwrap().type as "Fulfilled" | "PartiallyFilled" | "Cancelled";
}

/** Read the on-chain OrderStatus and return the PartiallyFilled amount, or null. */
export async function getPartiallyFilledAmount(polkadotJs: any, id: `0x${string}`): Promise<bigint | null> {
    const result = await polkadotJs.query.limitOrders.orders(id);
    if (result.isNone) return null;
    const status = result.unwrap();
    if (status.type !== "PartiallyFilled") return null;
    return BigInt(status.asPartiallyFilled.toString());
}

/**
 * Read a provider's recorded output from `LinkedOutputs`, or `undefined` when there
 * is none — which is the case when the order never declared `has_linked_order`, has
 * not executed yet, was already drawn from, or was pruned.
 *
 * The stored type comes from pallet metadata, so no registry entry is needed here;
 * only the *signing* payload needs the hand-registered types above.
 */
export async function getLinkedOutput(polkadotJs: any, id: `0x${string}`): Promise<LinkedOutput | undefined> {
    const result = await polkadotJs.query.limitOrders.linkedOutputs(id);
    if (result.isNone) return undefined;
    const record = result.unwrap();
    const asset = record.asset;
    return {
        signer: record.signer.toString(),
        asset: asset.isTao
            ? "Tao"
            : {
                  netuid: asset.asAlpha.netuid.toNumber(),
                  hotkey: asset.asAlpha.hotkey.toString(),
              },
        total: BigInt(record.total.toString()),
        expires_at: BigInt(record.expiresAt.toString()),
    };
}

/**
 * As {@link getLinkedOutput}, but throws when there is no record.  Use this when the
 * test needs to READ the record's fields; use `getLinkedOutput` when it only needs to
 * assert presence or absence.
 */
export async function expectLinkedOutput(polkadotJs: any, id: `0x${string}`): Promise<LinkedOutput> {
    const record = await getLinkedOutput(polkadotJs, id);
    if (record === undefined) {
        throw new Error(`no LinkedOutputs entry for ${id}`);
    }
    return record;
}

/** The pallet's configured `LinkedOutputTtl`, in milliseconds. */
export function linkedOutputTtl(polkadotJs: any): bigint {
    return BigInt(polkadotJs.consts.limitOrders.linkedOutputTtl.toString());
}

/** Filter system events by method name. */
export function filterEvents(events: any, method: string): any[] {
    return (events as any[]).filter((e: any) => e.event.method === method);
}

/** Read the EVM chain ID from pallet_evm_chain_id storage. */
export async function fetchChainId(api: any): Promise<bigint> {
    const result = await api.query.evmChainId.chainId();
    return BigInt(result.toString());
}

/**
 * Compute the expected `net_amount` field of `GroupExecutionSummary` for a
 * mixed buy/sell batch, mirroring the pallet's netting logic.
 *
 * The runtime API returns `floor(price_actual * 1e9)` as a u64, so our
 * bigint replication differs from the on-chain U96F32 result by at most a
 * few RAO — use `toBeCloseTo` or a small tolerance window when asserting.
 *
 * @param polkadotJs  polkadot-js ApiPromise
 * @param netuid      subnet id
 * @param buySideTao  total net TAO from buy orders (after fees, in RAO)
 * @param sellSideAlpha  total net alpha from sell orders (in RAO)
 * @param side        which side dominates ("Buy" | "Sell")
 */
export async function computeNetAmount(
    polkadotJs: any,
    netuid: number,
    buySideTao: bigint,
    sellSideAlpha: bigint,
    side: "Buy" | "Sell"
): Promise<bigint> {
    // price_scaled = floor(price_actual * 1e9)  [RAO per alpha * 1e9 / 1e9 = dimensionless]
    const priceRaw = await polkadotJs.call.swapRuntimeApi.currentAlphaPrice(netuid);
    const price = BigInt(priceRaw.toString());
    const SCALE = 1_000_000_000n;

    if (side === "Buy") {
        // net_amount (TAO) = buy_tao - alpha_to_tao(sell_alpha, price)
        //   alpha_to_tao ≈ floor(price * sell_alpha / 1e9)
        const sellTaoEquiv = (price * sellSideAlpha) / SCALE;
        return buySideTao - sellTaoEquiv;
    } else {
        // net_amount (alpha) = sell_alpha - tao_to_alpha(buy_tao, price)
        //   tao_to_alpha ≈ floor(buy_tao * 1e9 / price)
        const buyAlphaEquiv = (buySideTao * SCALE) / price;
        return sellSideAlpha - buyAlphaEquiv;
    }
}

export async function executeBatchedOrders(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    orders: SignedOrder[]
): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    // The generated PAPI descriptors in `.papi/descriptors` predate `VersionedOrder::V2`,
    // so their `order` type only admits V1 and a v2 payload will not typecheck here.
    // Regenerate with `pnpm generate-types` against a node carrying this runtime to drop
    // the cast. The ApiPromise-based `devExecuteOrders` path needs no such cast because it
    // encodes via the hand-registered types in `registerLimitOrderTypes`.
    const tx = api.tx.LimitOrders.execute_batched_orders({
        netuid,
        orders: orders as never,
    });
    await waitForTransactionWithRetry(api, tx, alice, "execute_batched_orders");
}
