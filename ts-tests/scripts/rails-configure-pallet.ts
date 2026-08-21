/**
 * Configure pallet-usd-psm on the localnet after the rails contracts are
 * deployed (run with tsx from ts-tests, like bootstrap.ts):
 *   - register the deployed Gateway contract as the pallet's gateway,
 *   - register the canonical USD ERC-20 as PSM asset 0,
 *   - initialize the protocol-owned tUSD/TAO pool from Alice's balance,
 *   - wire the outbound leg: hub mailbox + hub sender gas,
 *   - for every catalog token: create a started subnet (emissions running),
 *     set its on-chain identity (name/description/logo mirrored from the
 *     mainnet subnet), and wire the escrow hotkey + share-mint route,
 *   - wire the USDC release route back to the portal.
 *
 * Env: GATEWAY_ADDR, CANONICAL_USD_ADDR, BT_MAILBOX_ADDR, PORTAL_ADDR (H160
 * hex), BASE_DOMAIN, BT_RPC_WS, TOKENS_JSON (catalog entries with deployed
 * `address` fields), RESOLVED_OUT (path for the resolved catalog).
 * Idempotent: every step checks current state first; a subnet is reused when
 * an outbound route already points at its token.
 *
 * Prints on stdout for deploy-contracts.sh: `HUB_SENDER 0x..`, `NETUID n`
 * (first catalog subnet), and writes RESOLVED_OUT with one entry per token:
 * netuid, address, symbol, plus name/description/logo/url read back from the
 * chain's SubnetIdentitiesV3 (the chain is the source of truth the demo page
 * displays).
 */
import { writeFileSync } from "node:fs";
import { ApiPromise, WsProvider } from "@polkadot/api";
import { blake2AsU8a } from "@polkadot/util-crypto";
import { stringToU8a, u8aToHex } from "@polkadot/util";
import {
    addr32,
    alicePair,
    createStartedSubnet,
    h160ToSS58,
    signAndWait,
} from "../utils/rails";

const WS_URL = process.env.BT_RPC_WS ?? "ws://127.0.0.1:9944";
const GATEWAY = required("GATEWAY_ADDR");
const CANONICAL_USD = required("CANONICAL_USD_ADDR");
const BT_MAILBOX = required("BT_MAILBOX_ADDR");
const PORTAL = required("PORTAL_ADDR");
const BASE_DOMAIN = Number(process.env.BASE_DOMAIN ?? "8453");
const RESOLVED_OUT = process.env.RESOLVED_OUT ?? "";

interface CatalogToken {
    address: string;
    name: string;
    symbol: string;
    description: string;
    logo: string;
    url: string;
}
const TOKENS: CatalogToken[] = JSON.parse(required("TOKENS_JSON"));

const USD_ASSET_ID = 0;
// PSM inflow window: 1M USD cap, ~100 USD/block refill (9 decimals).
const CAP_LIMIT = 1_000_000n * 10n ** 9n;
const REFILL_PER_BLOCK = 100n * 10n ** 9n;
const HAIRCUT_BPS = 0;
// Protocol-owned pool seed: 10k TAO against 40k tUSD (1 TAO = 4 USD locally).
const POOL_TAO = 10_000n * 10n ** 9n;
const POOL_TUSD = 40_000n * 10n ** 9n;
const POOL_FEE_BPS = 30;
// Index heartbeat: push the share index to the spoke every ~20 blocks so the
// MetaMask balance visibly ticks without any buy/sell traffic.
const HEARTBEAT_BLOCKS = 20;

function required(name: string): string {
    const value = process.env[name];
    if (!value) {
        throw new Error(`missing env ${name}`);
    }
    return name === "TOKENS_JSON" ? value : value.toLowerCase();
}

/** The runtime's keyless outbound identity: blake2_256("rails/hub-evm")[..20]. */
function hubEvmAddress(): string {
    return u8aToHex(blake2AsU8a(stringToU8a("rails/hub-evm"), 256).slice(0, 20));
}

async function sudoAndWait(api: ApiPromise, call: any, signer: any, label: string): Promise<void> {
    await signAndWait(api, api.tx.sudo.sudo(call), signer, label);
    console.log(`[configure] ${label}: ok`);
}

/** Map token address (as bytes32 route) -> netuid for already-wired routes. */
async function existingRoutes(api: ApiPromise): Promise<Map<string, number>> {
    const routes = new Map<string, number>();
    const entries = await api.query.usdPsm.remoteRoutes.entries(BASE_DOMAIN);
    for (const [key, value] of entries) {
        const route = (value.toPrimitive() as string | null)?.toLowerCase();
        if (route) {
            routes.set(route, Number(key.args[1].toPrimitive()));
        }
    }
    return routes;
}

async function main() {
    const api = await ApiPromise.create({
        provider: new WsProvider(WS_URL),
        noInitWarn: true,
    });
    const alice = alicePair();

    const gateway = (await api.query.usdPsm.gateway()).toPrimitive() as string | null;
    if (gateway?.toLowerCase() === GATEWAY) {
        console.log("[configure] gateway already set");
    } else {
        await sudoAndWait(api, api.tx.usdPsm.setGateway(GATEWAY), alice, `set gateway ${GATEWAY}`);
    }

    const asset: any = (await api.query.usdPsm.psmAssets(USD_ASSET_ID)).toPrimitive();
    if (asset && asset.erc20?.toLowerCase() === CANONICAL_USD) {
        console.log("[configure] canonical USD asset already registered");
    } else {
        await sudoAndWait(
            api,
            api.tx.usdPsm.registerUsdAsset(
                USD_ASSET_ID,
                CANONICAL_USD,
                CAP_LIMIT,
                REFILL_PER_BLOCK,
                HAIRCUT_BPS
            ),
            alice,
            `register USD asset 0 -> ${CANONICAL_USD}`
        );
    }

    const taoReserve = BigInt(
        ((await api.query.usdPsm.poolTaoReserve()).toPrimitive() as number | string) ?? 0
    );
    if (taoReserve > 0n) {
        console.log("[configure] pool already initialized");
    } else {
        await sudoAndWait(
            api,
            api.tx.usdPsm.initPool(alice.address, POOL_TAO, POOL_TUSD, POOL_FEE_BPS),
            alice,
            `init pool ${POOL_TAO} rao / ${POOL_TUSD} tUSD`
        );
    }

    // Outbound leg: the runtime dispatches share mints, index heartbeats and
    // USDC releases through this mailbox.
    const hubMailbox = (await api.query.usdPsm.hubMailbox()).toPrimitive() as string | null;
    if (hubMailbox?.toLowerCase() === BT_MAILBOX) {
        console.log("[configure] hub mailbox already set");
    } else {
        await sudoAndWait(
            api,
            api.tx.usdPsm.setHubMailbox(BT_MAILBOX),
            alice,
            `set hub mailbox ${BT_MAILBOX}`
        );
    }

    // One started subnet per catalog token, owned by Alice, with Alice's
    // hotkey as the escrow validator. Identities mirror the real mainnet
    // subnets so the demo page can pull names/descriptions/logos from the
    // chain. Reused across re-runs via the outbound-route mapping.
    const routes = await existingRoutes(api);
    const resolved: Array<{
        netuid: number;
        address: string;
        symbol: string;
        name: string;
        description: string;
        logo: string;
        url: string;
    }> = [];
    for (const token of TOKENS) {
        const token32 = addr32(token.address.toLowerCase());
        let netuid = routes.get(token32);
        if (netuid !== undefined) {
            console.log(`[configure] ${token.symbol} already wired (netuid ${netuid})`);
        } else {
            netuid = await createStartedSubnet(api, alice);
            console.log(`[configure] created started subnet netuid ${netuid} for ${token.symbol}`);
            await sudoAndWait(
                api,
                api.tx.usdPsm.setOutboundRoute(BASE_DOMAIN, netuid, token32),
                alice,
                `route netuid ${netuid} shares -> domain ${BASE_DOMAIN} ${token.symbol} ${token.address}`
            );
        }

        // On-chain identity: Alice owns the subnet, so she signs directly.
        const identity: any = (
            await api.query.subtensorModule.subnetIdentitiesV3(netuid)
        ).toHuman();
        if (identity?.subnetName === token.name) {
            console.log(`[configure] identity for netuid ${netuid} already set`);
        } else {
            await signAndWait(
                api,
                api.tx.subtensorModule.setSubnetIdentity(
                    netuid,
                    token.name,
                    "", // github_repo
                    "", // subnet_contact
                    token.url,
                    "", // discord
                    token.description,
                    token.logo,
                    "" // additional
                ),
                alice,
                `identity netuid ${netuid} = ${token.name}`
            );
            console.log(`[configure] identity netuid ${netuid} = ${token.name}: ok`);
        }

        // Escrow hotkey: buys stake into (hotkey=Alice, coldkey=hub escrow)
        // so the escrow earns emissions like any nominator.
        const escrowHotkey = (await api.query.usdPsm.escrowHotkeys(netuid)).toPrimitive() as
            | string
            | null;
        if (escrowHotkey === alice.address) {
            console.log(`[configure] escrow hotkey netuid ${netuid} already set`);
        } else {
            await sudoAndWait(
                api,
                api.tx.usdPsm.setEscrowHotkey(netuid, alice.address),
                alice,
                `escrow hotkey netuid ${netuid} -> Alice`
            );
        }

        // Read the identity back: what the chain stores is what ships.
        const onChain: any = (
            await api.query.subtensorModule.subnetIdentitiesV3(netuid)
        ).toHuman();
        resolved.push({
            netuid,
            address: token.address,
            symbol: token.symbol,
            name: onChain?.subnetName ?? token.name,
            description: onChain?.description ?? token.description,
            logo: onChain?.logoUrl ?? token.logo,
            url: onChain?.subnetUrl ?? token.url,
        });
    }

    // USDC release route: sells pay out through the portal on fake Base.
    const portal32 = addr32(PORTAL);
    const usdRoute = (await api.query.usdPsm.usdReleaseRoutes(BASE_DOMAIN)).toPrimitive() as
        | string
        | null;
    if (usdRoute?.toLowerCase() === portal32) {
        console.log("[configure] USD release route already set");
    } else {
        await sudoAndWait(
            api,
            api.tx.usdPsm.setUsdRoute(BASE_DOMAIN, portal32),
            alice,
            `USD release route domain ${BASE_DOMAIN} -> portal ${PORTAL}`
        );
    }

    const heartbeat = Number((await api.query.usdPsm.heartbeatInterval()).toPrimitive());
    if (heartbeat === HEARTBEAT_BLOCKS) {
        console.log("[configure] heartbeat interval already set");
    } else {
        await sudoAndWait(
            api,
            api.tx.usdPsm.setHeartbeatInterval(HEARTBEAT_BLOCKS),
            alice,
            `heartbeat every ${HEARTBEAT_BLOCKS} blocks`
        );
    }

    // Gas for the hub's keyless EVM identity: outbound dispatches are real
    // EVM transactions and pay fees from this account.
    const hubSender = hubEvmAddress();
    const hubMirror = h160ToSS58(hubSender);
    const HUB_GAS_RAO = 1_000_000n * 10n ** 9n;
    const hubAccount: any = (await api.query.system.account(hubMirror)).toPrimitive();
    if (BigInt(hubAccount?.data?.free ?? 0) >= HUB_GAS_RAO) {
        console.log(`[configure] hub sender ${hubSender} already funded`);
    } else {
        await sudoAndWait(
            api,
            api.tx.balances.forceSetBalance(hubMirror, HUB_GAS_RAO),
            alice,
            `fund hub sender ${hubSender}`
        );
    }

    if (RESOLVED_OUT) {
        writeFileSync(RESOLVED_OUT, JSON.stringify(resolved, null, 2));
        console.log(`[configure] wrote resolved catalog -> ${RESOLVED_OUT}`);
    }
    // Consumed by deploy-contracts.sh to wire the Base side.
    console.log(`HUB_SENDER ${hubSender}`);
    console.log(`NETUID ${resolved[0].netuid}`);

    await api.disconnect();
}

main().catch((err) => {
    console.error("[configure] FAILED:", err);
    process.exit(1);
});
