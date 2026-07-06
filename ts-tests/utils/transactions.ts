import type { KeyringPair } from "@moonwall/util";
import type { subtensor } from "@polkadot-api/descriptors";
import { sleep } from "@zombienet/utils";
import type { Transaction, TypedApi } from "polkadot-api";
import { getPolkadotSigner } from "polkadot-api/signer";
import { log } from "./logger.js";
import { waitForBlocks } from "./staking.ts";

export async function waitForTransactionWithRetry(
    api: TypedApi<typeof subtensor>,
    tx: Transaction<Record<string, unknown>, string, string, void>,
    signer: KeyringPair,
    label: string,
    maxRetries = 1
): Promise<void> {
    let retries = 0;

    while (retries < maxRetries) {
        try {
            await waitForTransactionCompletion(tx, signer);
            return;
        } catch (error) {
            log.tx(label, `attempt ${retries + 1} failed: ${error}`);
            retries += 1;
            if (retries >= maxRetries) {
                throw new Error(`[${label}] failed after ${maxRetries} retries`);
            }
            await waitForBlocks(api, 1);
        }
    }
}

export async function waitForTransactionCompletion(
    tx: Transaction<Record<string, unknown>, string, string, void>,
    keypair: KeyringPair,
    timeout: number = 3 * 60 * 1000
): Promise<{ txHash: string; blockHash: string }> {
    const result = await sendTransaction(tx, keypair, timeout);
    if (!result.success) {
        throw new Error(result.errorMessage || "Transaction failed");
    }
    if (!result.txHash || !result.blockHash) {
        throw new Error("Missing txHash or blockHash in successful transaction");
    }
    return {
        txHash: result.txHash,
        blockHash: result.blockHash,
    };
}

export type TransactionResult = {
    success: boolean;
    events: any[];
    txHash?: string;
    blockHash?: string;
    errorMessage?: string;
};

/**
 * Send a transaction and return a result object instead of throwing on ExtrinsicFailed.
 * Use this for tests that expect failure.
 */
export async function sendTransaction(
    tx: Transaction<Record<string, unknown>, string, string, void>,
    signer: KeyringPair,
    timeout: number = 3 * 60 * 1000
): Promise<TransactionResult> {
    const callerStack = new Error().stack;
    const polkadotSigner = getPolkadotSigner(signer.publicKey, "Sr25519", signer.sign);

    return new Promise((resolve) => {
        const timer = setTimeout(() => {
            subscription.unsubscribe();
            console.log("Transaction timed out");
            console.error("callerStack", callerStack);
            resolve({ success: false, events: [], errorMessage: "Transaction timed out" });
        }, timeout);

        const subscription = tx.signSubmitAndWatch(polkadotSigner).subscribe({
            next(event) {
                if (event.type === "finalized") {
                    clearTimeout(timer);
                    subscription.unsubscribe();

                    if (event.dispatchError) {
                        resolve({
                            success: false,
                            events: event.events,
                            txHash: event.txHash,
                            blockHash: event.block.hash,
                            errorMessage: JSON.stringify(event.dispatchError),
                        });
                    } else {
                        resolve({
                            success: true,
                            events: event.events,
                            txHash: event.txHash,
                            blockHash: event.block.hash,
                        });
                    }
                }
            },
            error(err) {
                clearTimeout(timer);
                subscription.unsubscribe();
                console.error("callerStack", callerStack);
                const message = err instanceof Error ? err.message : String(err);
                resolve({ success: false, events: [], errorMessage: message });
            },
        });
    });
}

const SECOND = 1000;

/** Polls the chain until `count` new finalized blocks have been produced. */
export async function waitForFinalizedBlocks(
    api: TypedApi<typeof subtensor>,
    count: number,
    pollInterval = 1 * SECOND,
    timeout = 120 * SECOND
): Promise<void> {
    const startBlock = await api.query.System.Number.getValue({ at: "finalized" });
    const target = startBlock + count;
    const deadline = Date.now() + timeout;

    while (Date.now() < deadline) {
        await sleep(pollInterval);

        const currentBlock = await api.query.System.Number.getValue({ at: "finalized" });

        if (currentBlock >= target) return;
    }

    throw new Error(`Timed out waiting for ${count} finalized blocks (from #${startBlock}, target #${target})`);
}
