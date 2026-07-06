import { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";

export async function getProxies(api: TypedApi<typeof subtensor>, address: string): Promise<string[]> {
    const entries = await api.query.Proxy.Proxies.getEntries();
    const result: string[] = [];
    for (const entry of entries) {
        const proxyAddress = entry.keyArgs[0];
        const values = entry.value;
        const proxies = values[0];
        for (const proxy of proxies) {
            if (proxy.delegate === address) {
                result.push(proxyAddress);
            }
        }
    }
    return result;
}
