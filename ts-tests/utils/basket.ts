import type { subtensor } from "@polkadot-api/descriptors";
import { Binary, type TypedApi } from "polkadot-api";
import { expect } from "vitest";
import { keyringPairFromUri } from "./account.ts";
import { sudoSetTempo } from "./staking.js";
import { waitForTransactionWithRetry } from "./transactions.js";

/** Settle a fixture's dividends after its artificial pool-funding price jump. */
export async function settleBasketDividends(
    api: TypedApi<typeof subtensor>,
    hotkey: string,
    netuids: number[]
): Promise<void> {
    // Stop fresh epoch credits. The slow EMA setters do not accelerate the
    // independent 600-block fast EMA, which is already running before start_call.
    for (const netuid of netuids) {
        await sudoSetTempo(api, netuid, 65_535);
    }

    // Reset only the fixture's fast anchors. Their normal next-block initializer
    // samples the now-funded pools; the production price-band guard stays enabled.
    const keys = await Promise.all(
        netuids.map(async (netuid) =>
            Binary.fromHex(await api.query.SubtensorModule.SubnetFastMovingPrice.getKey(netuid))
        )
    );
    const reset = api.tx.System.kill_storage({ keys });
    const alice = keyringPairFromUri("//Alice");
    await waitForTransactionWithRetry(
        api,
        api.tx.Sudo.sudo({ call: reset.decodedCall }),
        alice,
        "reset_fixture_fast_prices"
    );

    await expect
        .poll(() => api.query.SubtensorModule.PendingBasketDeposits.getEntries(hotkey), {
            timeout: 10_000,
            interval: 100,
            message: "The block drain must settle the fixture's queued dividends",
        })
        .toHaveLength(0);
}
