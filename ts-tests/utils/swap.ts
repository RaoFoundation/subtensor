import { waitForTransactionWithRetry } from "./transactions.js";
import type { KeyringPair } from "@moonwall/util";
import type { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";

export async function swapHotkey(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    oldHotkey: string,
    newHotkey: string,
    netuid?: number
): Promise<void> {
    const tx = api.tx.SubtensorModule.swap_hotkey({
        hotkey: oldHotkey,
        new_hotkey: newHotkey,
        netuid: netuid ?? undefined,
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "swap_hotkey");
}
