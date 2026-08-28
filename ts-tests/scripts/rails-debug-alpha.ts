// Debug: how does the Alpha NMap decode for a live stake?
import { ApiPromise, WsProvider } from "@polkadot/api";
import { Keyring } from "@polkadot/keyring";
import { alicePair, createStartedSubnet, signAndWait } from "../utils/rails.js";

async function main() {
    const api = await ApiPromise.create({
        provider: new WsProvider("ws://127.0.0.1:9944"),
        noInitWarn: true,
    });
    const alice = alicePair();
    const hot = new Keyring({ type: "sr25519" }).addFromUri(`//DebugHot/${Date.now()}`);

    const netuid = await createStartedSubnet(api, hot);
    console.log("netuid:", netuid);
    await signAndWait(api, api.tx.subtensorModule.addStake(hot.address, netuid, 10_000_000_000n), alice, "add_stake");
    console.log("staked 10 TAO");

    const entry = await api.query.subtensorModule.alpha(hot.address, alice.address, netuid);
    console.log("alpha(hot, cold, netuid) json:", JSON.stringify(entry.toJSON()));
    console.log("alpha(hot, cold, netuid) str:", entry.toString());
    const th = await api.query.subtensorModule.totalHotkeyAlpha(hot.address, netuid);
    console.log("totalHotkeyAlpha:", th.toString());
    const entries = await api.query.subtensorModule.alpha.entries();
    console.log("total Alpha entries:", entries.length);
    for (const [k, v] of entries.slice(-3)) {
        console.log("  key:", k.args.map(String).join(", "), "value:", JSON.stringify(v.toJSON()));
    }
    process.exit(0);
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
