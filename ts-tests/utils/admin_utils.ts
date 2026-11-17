import type { subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import { keyringPairFromUri } from "./account.ts";
import { waitForTransactionWithRetry } from "./transactions.js";

export async function sudoSetStakeThreshold(api: TypedApi<typeof subtensor>, threshold: bigint): Promise<void> {
    const alice = keyringPairFromUri("//Alice");
    const inner = api.tx.AdminUtils.sudo_set_stake_threshold({ min_stake: threshold });
    const tx = api.tx.Sudo.sudo({ call: inner.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_stake_threshold");
}

export async function setTargetRegistrationsPerInterval(
    api: TypedApi<typeof subtensor>,
    netuid: number
): Promise<void> {
    const alice = keyringPairFromUri("//Alice");
    const internalTx = api.tx.AdminUtils.sudo_set_target_registrations_per_interval({
        netuid,
        target_registrations_per_interval: 1000,
    });
    const tx = api.tx.Sudo.sudo({ call: internalTx.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_target_registrations_per_interval");

    const target = await api.query.SubtensorModule.TargetRegistrationsPerInterval.getValue(netuid);
    if (target !== 1000) {
        throw new Error(`Expected TargetRegistrationsPerInterval=1000 for netuid ${netuid}, got ${target}`);
    }
}
