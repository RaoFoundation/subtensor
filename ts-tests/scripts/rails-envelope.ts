/**
 * Encode a v1 GatewayEnvelope *prefix* to wire hex (mirror of
 * subtensor_runtime_common::rails::GatewayEnvelope::to_wire, minus the
 * trailing u64 nonce — the RailsPortal appends its own sequential nonce on
 * dispatch).
 *
 * Usage (run with tsx from ts-tests):
 *   tsx scripts/rails-envelope.ts credit <usdAssetId> <amount> <destSS58orHexPubkey>
 *   tsx scripts/rails-envelope.ts stake  <usdAssetId> <amount> <dest> <netuid> <hotkey> [minAlpha]
 *
 * Prints the 0x-prefixed envelope prefix hex on stdout.
 */
import { decodeAddress } from "@polkadot/util-crypto";
import { hexToU8a, isHex, u8aConcat, u8aToHex } from "@polkadot/util";

const VERSION_V1 = 1;

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

function accountId(dest: string): Uint8Array {
    if (isHex(dest) && dest.length === 66) {
        return hexToU8a(dest);
    }
    return decodeAddress(dest);
}

function main() {
    const [mode, usdAssetId, amount, dest, ...rest] = process.argv.slice(2);
    if (!mode || !usdAssetId || !amount || !dest) {
        console.error("usage: rails-envelope.ts credit|stake <usdAssetId> <amount> <dest> [...]");
        process.exit(1);
    }

    // AssetId::Usd(u32) = variant index 3 + u32 LE.
    const asset = u8aConcat(new Uint8Array([3]), u32le(Number(usdAssetId)));

    let action: Uint8Array;
    switch (mode) {
        case "credit":
            // GatewayAction::CreditTUsd = variant 0.
            action = new Uint8Array([0]);
            break;
        case "stake": {
            // GatewayAction::Stake { netuid, hotkey, min_alpha }
            //   = variant 2 + u16 LE + AccountId32 + u64 LE.
            const [netuid, hotkey, minAlpha] = rest;
            if (!netuid || !hotkey) {
                console.error("stake mode needs <netuid> <hotkey> [minAlpha]");
                process.exit(1);
            }
            action = u8aConcat(
                new Uint8Array([2]),
                u16le(Number(netuid)),
                accountId(hotkey),
                u64le(BigInt(minAlpha ?? "0"))
            );
            break;
        }
        default:
            console.error(`unknown mode ${mode}`);
            process.exit(1);
    }

    const prefix = u8aConcat(
        new Uint8Array([VERSION_V1]),
        asset,
        u64le(BigInt(amount)),
        accountId(dest),
        action
    );
    console.log(u8aToHex(prefix));
}

main();
