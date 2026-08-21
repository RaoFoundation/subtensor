/**
 * Substrate-side bootstrap for the rails rig, run with tsx from ts-tests
 * (which provides @polkadot/api): disables the EVM deploy whitelist and
 * funds the rig's EVM accounts with TAO via their SS58 mirrors.
 *
 * Usage: pnpm exec tsx ../scripts/rails/bootstrap.ts
 */
import { ApiPromise, WsProvider } from "@polkadot/api";
import { Keyring } from "@polkadot/keyring";
import { blake2AsU8a, encodeAddress } from "@polkadot/util-crypto";
import { hexToU8a, stringToU8a, u8aConcat } from "@polkadot/util";

const WS_URL = process.env.BT_RPC_WS ?? "ws://127.0.0.1:9944";
const SS58_PREFIX = 42;
// 1M TAO per rig account: ample for gas, far below u64 issuance limits.
const FUND_RAO = 1_000_000n * 10n ** 9n;

const EVM_ACCOUNTS: Record<string, string> = {
    deployer: process.env.DEPLOYER_ADDR ?? "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    relayer: process.env.RELAYER_ADDR ?? "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC",
    // The btlocal Hyperlane validator must self-announce its checkpoint
    // location on-chain; without gas here, outbound messages never relay.
    hyperlaneValidator: process.env.VALIDATOR_ADDR ?? "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
};

/** SS58 mirror of an H160: blake2_256("evm:" ++ h160) — Frontier's HashedAddressMapping. */
function h160ToSS58(ethAddress: string): string {
    const prefixed = u8aConcat(stringToU8a("evm:"), hexToU8a(ethAddress));
    return encodeAddress(blake2AsU8a(prefixed, 256), SS58_PREFIX);
}

async function signAndWait(api: ApiPromise, tx: any, signer: any, label: string): Promise<void> {
    await new Promise<void>((resolve, reject) => {
        tx.signAndSend(signer, ({ status, dispatchError }: any) => {
            if (dispatchError) {
                reject(new Error(`${label}: ${dispatchError.toString()}`));
            } else if (status.isInBlock || status.isFinalized) {
                resolve();
            }
        }).catch(reject);
    });
    console.log(`[bootstrap] ${label}: ok`);
}

async function main() {
    const api = await ApiPromise.create({
        provider: new WsProvider(WS_URL),
        noInitWarn: true,
    });
    const alice = new Keyring({ type: "sr25519" }).addFromUri("//Alice");

    // 1. Disable the EVM deploy whitelist so the rig deployer can create
    //    contracts (localnet-only setting).
    const whitelistDisabled = (await api.query.evm.disableWhitelistCheck()).toPrimitive();
    if (whitelistDisabled !== true) {
        await signAndWait(
            api,
            api.tx.sudo.sudo(api.tx.evm.disableWhitelist(true)),
            alice,
            "disable EVM whitelist"
        );
    } else {
        console.log("[bootstrap] EVM whitelist already disabled");
    }

    // 2. Fund the rig EVM accounts through their SS58 mirrors.
    for (const [label, h160] of Object.entries(EVM_ACCOUNTS)) {
        const mirror = h160ToSS58(h160);
        const account: any = (await api.query.system.account(mirror)).toPrimitive();
        const free = BigInt(account?.data?.free ?? 0);
        if (free >= FUND_RAO) {
            console.log(`[bootstrap] ${label} (${h160}) already funded`);
            continue;
        }
        await signAndWait(
            api,
            api.tx.sudo.sudo(api.tx.balances.forceSetBalance(mirror, FUND_RAO)),
            alice,
            `fund ${label} ${h160}`
        );
    }

    await api.disconnect();
}

main().catch((err) => {
    console.error("[bootstrap] FAILED:", err);
    process.exit(1);
});
