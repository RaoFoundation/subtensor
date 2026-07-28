import type { KeyringPair } from "@moonwall/util";
import type { TypedApi } from "polkadot-api";
import type { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import { u8aToHex, u8aWrapBytes } from "@polkadot/util";
import { blake2AsHex, blake2AsU8a } from "@polkadot/util-crypto";
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

export interface VersionedOrder {
    V1: Order;
}

export interface SignedOrder {
    order: VersionedOrder;
    signature: { Sr25519: `0x${string}` } | { Ed25519: `0x${string}` } | { Ecdsa: `0x${string}` };
    partial_fill: number | null;
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
        LimitVersionedOrder: {
            _enum: {
                V1: "LimitOrder",
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
    const tx = api.tx.LimitOrders.execute_batched_orders({
        netuid,
        orders,
    });
    await waitForTransactionWithRetry(api, tx, alice, "execute_batched_orders");
}
