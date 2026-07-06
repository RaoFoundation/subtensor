import type { KeyringPair } from "@moonwall/util";
import type { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import type { TypedApi } from "polkadot-api";
import { log } from "./logger.js";
import { waitForTransactionWithRetry } from "./transactions.js";

export async function addNewSubnetwork(
    api: TypedApi<typeof subtensor>,
    hotkey: KeyringPair,
    coldkey: KeyringPair
): Promise<number> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const totalNetworks = await api.query.SubtensorModule.TotalNetworks.getValue();

    // Disable network rate limit for testing
    const rateLimit = await api.query.SubtensorModule.NetworkRateLimit.getValue();
    if (rateLimit !== BigInt(0)) {
        const internalCall = api.tx.AdminUtils.sudo_set_network_rate_limit({ rate_limit: BigInt(0) });
        const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
        await waitForTransactionWithRetry(api, tx, alice, "set_network_rate_limit");
    }

    const registerNetworkTx = api.tx.SubtensorModule.register_network({
        hotkey: hotkey.address,
    });
    await waitForTransactionWithRetry(api, registerNetworkTx, coldkey, "register_network");

    return totalNetworks;
}

export async function burnedRegister(
    api: TypedApi<typeof subtensor>,
    netuid: number,
    hotkeyAddress: string,
    coldkey: KeyringPair
): Promise<void> {
    const registered = await api.query.SubtensorModule.Uids.getValue(netuid, hotkeyAddress);
    if (registered !== undefined) {
        log.tx("burned_register", `skipped: hotkey already registered on netuid ${netuid}`);
        return;
    }

    await new Promise((resolve) => setTimeout(resolve, 1000));
    const tx = api.tx.SubtensorModule.burned_register({ hotkey: hotkeyAddress, netuid: netuid });
    await waitForTransactionWithRetry(api, tx, coldkey, "burned_register");
}

export async function startCall(api: TypedApi<typeof subtensor>, netuid: number, coldkey: KeyringPair): Promise<void> {
    const registerBlock = Number(await api.query.SubtensorModule.NetworkRegisteredAt.getValue(netuid));
    let currentBlock = await api.query.System.Number.getValue();
    const duration = Number(await api.constants.SubtensorModule.InitialStartCallDelay);
    const deadline = Date.now() + 120_000;

    while (currentBlock - registerBlock <= duration) {
        if (Date.now() > deadline) {
            throw new Error(
                `startCall timed out for netuid ${netuid} (registerBlock=${registerBlock}, currentBlock=${currentBlock}, duration=${duration})`
            );
        }
        await new Promise((resolve) => setTimeout(resolve, 2000));
        currentBlock = await api.query.System.Number.getValue();
    }

    await new Promise((resolve) => setTimeout(resolve, 2000));

    const tx = api.tx.SubtensorModule.start_call({ netuid: netuid });
    await waitForTransactionWithRetry(api, tx, coldkey, "start_call");

    await new Promise((resolve) => setTimeout(resolve, 1000));
}

export async function rootRegister(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkeyAddress: string
): Promise<void> {
    const tx = api.tx.SubtensorModule.root_register({ hotkey: hotkeyAddress });
    await waitForTransactionWithRetry(api, tx, coldkey, "root_register");
}
