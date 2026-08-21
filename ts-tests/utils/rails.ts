/**
 * Helpers for the rails e2e suite (suites/rails): rig manifest access,
 * ethers handles to both local chains, GatewayEnvelope wire encoding, and
 * substrate-side setup (subnet creation, delivery polling).
 *
 * Everything reads the rig manifest written by `just rails-up`, so the suite
 * has zero hardcoded addresses.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { ApiPromise } from "@polkadot/api";
import { Keyring } from "@polkadot/keyring";
import type { KeyringPair } from "@polkadot/keyring/types";
import { hexToU8a, isHex, stringToU8a, u8aConcat, u8aToHex } from "@polkadot/util";
import { blake2AsU8a, decodeAddress, encodeAddress } from "@polkadot/util-crypto";
import { ethers } from "ethers";
import artifacts from "./rails-artifacts.json";

const THIS_DIR = dirname(fileURLToPath(import.meta.url));
const MANIFEST_PATH = join(THIS_DIR, "..", "..", ".rails", "manifest.json");

// Anvil developer key #0: the rig deployer/owner on both chains.
export const DEPLOYER_PK = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

export interface RailsManifest {
    version: number;
    chains: {
        btlocal: {
            chainId: number;
            domain: number;
            rpcHttp: string;
            rpcWs: string;
            mailbox: string;
            canonicalUsd: string;
            gateway: string;
            hubSender: string;
            netuid: number;
            usdRailsPrecompile: string;
            psmEscrow: string;
        };
        baselocal: {
            chainId: number;
            domain: number;
            rpcHttp: string;
            mailbox: string;
            mockUsdc: string;
            portal: string;
            /** Alias for the first catalog token (kept for existing suites). */
            chutes: string;
            /** One rebasing share token per demo subnet, identity read from chain. */
            tokens: Array<{
                netuid: number;
                address: string;
                symbol: string;
                name: string;
                description: string;
                logo: string;
                url: string;
            }>;
        };
    };
    accounts: { deployer: string; hyperlaneValidator: string; relayer: string };
}

export function loadManifest(): RailsManifest {
    return JSON.parse(readFileSync(MANIFEST_PATH, "utf8")) as RailsManifest;
}

/** Live ethers handles to every rig contract, wallet-connected as deployer. */
export interface RailsRig {
    manifest: RailsManifest;
    btProvider: ethers.JsonRpcProvider;
    baseProvider: ethers.JsonRpcProvider;
    btDeployer: ethers.Wallet;
    baseDeployer: ethers.Wallet;
    mockUsdc: ethers.Contract;
    portal: ethers.Contract;
    chutes: ethers.Contract;
    gateway: ethers.Contract;
    canonicalUsd: ethers.Contract;
}

export function connectRig(): RailsRig {
    const manifest = loadManifest();
    const btProvider = new ethers.JsonRpcProvider(manifest.chains.btlocal.rpcHttp, undefined, {
        cacheTimeout: -1,
    });
    const baseProvider = new ethers.JsonRpcProvider(manifest.chains.baselocal.rpcHttp, undefined, {
        cacheTimeout: -1,
    });
    const btDeployer = new ethers.Wallet(DEPLOYER_PK, btProvider);
    const baseDeployer = new ethers.Wallet(DEPLOYER_PK, baseProvider);
    return {
        manifest,
        btProvider,
        baseProvider,
        btDeployer,
        baseDeployer,
        mockUsdc: new ethers.Contract(manifest.chains.baselocal.mockUsdc, artifacts.MockUSDC.abi, baseDeployer),
        portal: new ethers.Contract(manifest.chains.baselocal.portal, artifacts.RailsPortal.abi, baseDeployer),
        chutes: new ethers.Contract(manifest.chains.baselocal.chutes, artifacts.Chutes.abi, baseDeployer),
        gateway: new ethers.Contract(manifest.chains.btlocal.gateway, artifacts.Gateway.abi, btDeployer),
        canonicalUsd: new ethers.Contract(
            manifest.chains.btlocal.canonicalUsd,
            artifacts.CanonicalShareToken.abi,
            btDeployer
        ),
    };
}

// ---------------------------------------------------------------- addresses

/** SS58 mirror of an H160: blake2_256("evm:" ++ h160) — Frontier's HashedAddressMapping. */
export function h160ToSS58(ethAddress: string): string {
    const prefixed = u8aConcat(stringToU8a("evm:"), hexToU8a(ethAddress));
    return encodeAddress(blake2AsU8a(prefixed, 256), 42);
}

/** Left-pad an H160 to the bytes32 form used as Hyperlane recipient/sender. */
export function addr32(ethAddress: string): string {
    return `0x${ethAddress.replace(/^0x/, "").toLowerCase().padStart(64, "0")}`;
}

// ------------------------------------------------------- envelope encoding

function u32le(n: number): Uint8Array {
    const b = new Uint8Array(4);
    new DataView(b.buffer).setUint32(0, n, true);
    return b;
}

function u64le(n: bigint): Uint8Array {
    const b = new Uint8Array(8);
    new DataView(b.buffer).setBigUint64(0, n, true);
    return b;
}

function u16le(n: number): Uint8Array {
    const b = new Uint8Array(2);
    new DataView(b.buffer).setUint16(0, n, true);
    return b;
}

function accountId32(dest: string): Uint8Array {
    if (isHex(dest) && dest.length === 66) {
        return hexToU8a(dest);
    }
    return decodeAddress(dest);
}

const ENVELOPE_VERSION_V1 = 1;

export type EnvelopeAction =
    | { kind: "credit" }
    | { kind: "stake"; netuid: number; hotkey: string; minAlpha: bigint }
    | { kind: "buyShares"; netuid: number; recipient: string; minAlpha: bigint; domain: number }
    | {
          kind: "sellShares";
          netuid: number;
          recipient: string;
          usdAssetId: number;
          minUsd: bigint;
          domain: number;
      };

export interface EnvelopeInput {
    /** PSM asset id for USD-carrying envelopes; ignored for sellShares (alpha asset). */
    usdAssetId?: number;
    amount: bigint;
    /** SS58 address or 0x pubkey hex of the runtime destination account. */
    dest: string;
    action: EnvelopeAction;
    nonce: bigint;
}

function h160Bytes(ethAddress: string): Uint8Array {
    return hexToU8a(ethAddress);
}

/**
 * Encode a v1 GatewayEnvelope to wire hex — mirror of
 * `subtensor_runtime_common::rails::GatewayEnvelope::to_wire`.
 */
export function encodeEnvelope(input: EnvelopeInput): string {
    // AssetId: Alpha(netuid) = index 2 + u16 LE; Usd(id) = index 3 + u32 LE.
    const asset =
        input.action.kind === "sellShares"
            ? u8aConcat(new Uint8Array([2]), u16le(input.action.netuid))
            : u8aConcat(new Uint8Array([3]), u32le(input.usdAssetId ?? 0));

    let action: Uint8Array;
    switch (input.action.kind) {
        case "credit":
            action = new Uint8Array([0]);
            break;
        case "stake":
            action = u8aConcat(
                new Uint8Array([2]),
                u16le(input.action.netuid),
                accountId32(input.action.hotkey),
                u64le(input.action.minAlpha)
            );
            break;
        case "buyShares":
            action = u8aConcat(
                new Uint8Array([4]),
                u16le(input.action.netuid),
                h160Bytes(input.action.recipient),
                u64le(input.action.minAlpha),
                u32le(input.action.domain)
            );
            break;
        case "sellShares":
            action = u8aConcat(
                new Uint8Array([5]),
                u16le(input.action.netuid),
                h160Bytes(input.action.recipient),
                u32le(input.action.usdAssetId),
                u64le(input.action.minUsd),
                u32le(input.action.domain)
            );
            break;
        default: {
            const exhaustive: never = input.action;
            throw new Error(`unknown action ${JSON.stringify(exhaustive)}`);
        }
    }

    return u8aToHex(
        u8aConcat(
            new Uint8Array([ENVELOPE_VERSION_V1]),
            asset,
            u64le(input.amount),
            accountId32(input.dest),
            action,
            u64le(input.nonce)
        )
    );
}

// ----------------------------------------------------------------- polling

export async function waitUntil<T>(
    probe: () => Promise<T | undefined>,
    label: string,
    timeoutMs = 120_000,
    intervalMs = 1_000
): Promise<T> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
        const value = await probe();
        if (value !== undefined) {
            return value;
        }
        if (Date.now() > deadline) {
            throw new Error(`timed out after ${timeoutMs}ms waiting for ${label}`);
        }
        await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
}

/** Poll pallet-usd-psm for an inbound receipt of `nonce`. */
export async function waitForReceipt(
    api: ApiPromise,
    nonce: bigint,
    timeoutMs = 120_000
): Promise<{ block: number; fallback: string | null }> {
    return waitUntil(
        async () => {
            const raw = (await api.query.usdPsm.processedNonces(nonce)).toJSON() as {
                block: number;
                fallback: string | null;
            } | null;
            return raw ?? undefined;
        },
        `inbound receipt for nonce ${nonce}`,
        timeoutMs
    );
}

// ----------------------------------------------------------- substrate side

export async function signAndWait(api: ApiPromise, tx: any, signer: KeyringPair, label: string): Promise<void> {
    await new Promise<void>((resolve, reject) => {
        tx.signAndSend(signer, ({ status, dispatchError }: any) => {
            if (dispatchError) {
                if (dispatchError.isModule) {
                    const meta = api.registry.findMetaError(dispatchError.asModule);
                    reject(new Error(`${label}: ${meta.section}.${meta.name}`));
                } else {
                    reject(new Error(`${label}: ${dispatchError.toString()}`));
                }
            } else if (status.isInBlock || status.isFinalized) {
                resolve();
            }
        }).catch(reject);
    });
}

export function alicePair(): KeyringPair {
    return new Keyring({ type: "sr25519" }).addFromUri("//Alice");
}

/**
 * Alpha staked by (hotkey, coldkey) on `netuid`, in rao-scale alpha, read
 * through the StakeInfo runtime API (the raw storage is a lazily-migrated
 * share pool and not stable to query directly).
 */
export async function alphaStakeOf(api: ApiPromise, hotkey: string, coldkey: string, netuid: number): Promise<bigint> {
    const info = (
        await (api.call as any).stakeInfoRuntimeApi.getStakeInfoForHotkeyColdkeyNetuid(hotkey, coldkey, netuid)
    ).toJSON() as { stake?: number | string } | null;
    return BigInt(info?.stake ?? 0);
}

/**
 * Create a fresh subnet owned by Alice with `hotkey` registered and started
 * (emissions running). Returns its netuid.
 */
export async function createStartedSubnet(api: ApiPromise, hotkey: KeyringPair): Promise<number> {
    const alice = alicePair();

    const rateLimit = Number((await api.query.subtensorModule.networkRateLimit()).toPrimitive());
    if (rateLimit !== 0) {
        await signAndWait(
            api,
            api.tx.sudo.sudo(api.tx.adminUtils.sudoSetNetworkRateLimit(0)),
            alice,
            "zero network rate limit"
        );
    }

    // The network lock cost doubles with every registration and only decays
    // over the reduction interval; repeated suite runs would otherwise price
    // Alice out. Pin it to a fixed minimum with instant decay. The minimum
    // must not be too small: registration seeds the subnet's AMM pool with
    // exactly `network_min_lock` TAO, and a 1-TAO-deep pool rejects ordinary
    // stakes with InsufficientLiquidity.
    const MIN_LOCK_RAO = 100_000_000_000n; // 100 TAO
    const reduction = Number((await api.query.subtensorModule.networkLockReductionInterval()).toPrimitive());
    const minLock = BigInt((await api.query.subtensorModule.networkMinLockCost()).toPrimitive() as string | number);
    if (reduction > 1 || minLock !== MIN_LOCK_RAO) {
        await signAndWait(
            api,
            api.tx.sudo.sudo(api.tx.adminUtils.sudoSetLockReductionInterval(1)),
            alice,
            "shrink lock reduction interval"
        );
        await signAndWait(
            api,
            api.tx.sudo.sudo(api.tx.adminUtils.sudoSetNetworkMinLockCost(MIN_LOCK_RAO)),
            alice,
            "set min network lock cost"
        );
    }

    const before = (await api.query.subtensorModule.totalNetworks()).toPrimitive() as number;
    await signAndWait(api, api.tx.subtensorModule.registerNetwork(hotkey.address), alice, "register_network");
    const netuid = before;

    // start_call is gated by a post-registration delay measured in blocks.
    const registeredAt = (await api.query.subtensorModule.networkRegisteredAt(netuid)).toPrimitive() as number;
    const delay = Number(api.consts.subtensorModule.initialStartCallDelay?.toPrimitive() ?? 5);
    await waitUntil(
        async () => {
            const now = (await api.query.system.number()).toPrimitive() as number;
            return now - registeredAt > delay ? true : undefined;
        },
        `start_call delay for netuid ${netuid}`,
        180_000
    );
    await signAndWait(api, api.tx.subtensorModule.startCall(netuid), alice, "start_call");
    return netuid;
}
