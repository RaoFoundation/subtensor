import { waitForTransactionWithRetry } from "./transactions.js";
import type { TypedApi } from "polkadot-api";
import type { subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@moonwall/util";
import { Keyring } from "@polkadot/keyring";

export async function setAutoParentDelegationEnabled(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    hotkey: string,
    enabled: boolean
): Promise<void> {
    const tx = api.tx.SubtensorModule.set_auto_parent_delegation_enabled({ hotkey, enabled });
    await waitForTransactionWithRetry(api, tx, coldkey, "set_auto_parent_delegation_enabled");
}

export async function getChildren(
    api: TypedApi<typeof subtensor>,
    hotkeyAddress: string,
    netuid: number
): Promise<Array<{ proportion: bigint; child: string }>> {
    const raw = await api.query.SubtensorModule.ChildKeys.getValue(hotkeyAddress, netuid);
    return (raw ?? []).map(([proportion, child]: [bigint, string]) => ({ proportion, child }));
}

export async function sudoSetPendingChildKeyCooldown(api: TypedApi<typeof subtensor>, cooldown: bigint): Promise<void> {
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const inner = api.tx.SubtensorModule.set_pending_childkey_cooldown({ cooldown });
    const tx = api.tx.Sudo.sudo({ call: inner.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_pending_childkey_cooldown");
}
