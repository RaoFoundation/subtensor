import { ApiPromise, WsProvider } from "@polkadot/api";
import { blake2AsU8a, encodeAddress } from "@polkadot/util-crypto";
import { hexToU8a, stringToU8a, u8aConcat } from "@polkadot/util";

async function main() {
    const api = await ApiPromise.create({
        provider: new WsProvider("ws://127.0.0.1:9944"),
        noInitWarn: true,
    });
    const mirror = encodeAddress(
        blake2AsU8a(
            u8aConcat(stringToU8a("evm:"), hexToU8a("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")),
            256
        ),
        42
    );
    console.log("mirror", mirror);
    console.log(JSON.stringify((await api.query.system.account(mirror)).toJSON()));
    console.log("totalIssuance", (await api.query.balances.totalIssuance()).toString());
    await api.disconnect();
}

main();
