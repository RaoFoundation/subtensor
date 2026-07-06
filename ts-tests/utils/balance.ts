import type { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import type { TypedApi } from "polkadot-api";
import { waitForFinalizedBlocks, waitForTransactionWithRetry } from "./transactions.js";
export const TAO = BigInt(1000000000); // 10^9 RAO per TAO
export const GWEI = BigInt(1000000000);
export const MAX_TX_FEE = BigInt(21000000) * GWEI;

export function tao(value: number): bigint {
    return TAO * BigInt(value);
}

/** Convert RAO to the EVM native balance unit (1 RAO → 1 gwei on-chain). */
export function raoToEth(rao: bigint): bigint {
    return GWEI * rao;
}

export function bigintToRao(value: bigint): bigint {
    return TAO * value;
}

export async function getBalance(api: TypedApi<typeof subtensor>, ss58Address: string): Promise<bigint> {
    const account = await api.query.System.Account.getValue(ss58Address);
    return account.data.free;
}

export async function forceSetBalance(
    api: TypedApi<typeof subtensor>,
    ss58Address: string,
    amount: bigint = tao(1e10)
): Promise<void> {
    const { MultiAddress } = await import("@polkadot-api/descriptors");
    const keyring = new Keyring({ type: "sr25519" });
    const alice = keyring.addFromUri("//Alice");
    const internalCall = api.tx.Balances.force_set_balance({
        who: MultiAddress.Id(ss58Address),
        new_free: amount,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "force_set_balance");
    await waitForFinalizedBlocks(api, 1);
}
