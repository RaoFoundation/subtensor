import type { KeyringPair } from "@moonwall/util";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import type { TypedApi } from "polkadot-api";
import { keyringPairFromUri } from "./account.ts";
import { waitForTransactionCompletion } from "./transactions.ts";

// Each file owns a genesis funder and a separate derivation namespace. This
// keeps both setup nonces and test balances independent on the shared network.
const funders = {
    basic: "Alice",
    scaling: "Bob",
    edge: "Charlie",
    timing: "Dave",
    mortality: "Eve",
} as const;

type ShieldSuite = keyof typeof funders;

export function shieldAccount(suite: ShieldSuite, name: string): KeyringPair {
    return keyringPairFromUri(`//${funders[suite]}//Shield//${name}`);
}

export async function fundShieldAccounts(
    api: TypedApi<typeof subtensor>,
    suite: ShieldSuite,
    accounts: KeyringPair[]
): Promise<void> {
    await waitForTransactionCompletion(
        api.tx.Utility.batch_all({
            calls: accounts.map(
                (account) =>
                    api.tx.Balances.transfer_keep_alive({
                        dest: MultiAddress.Id(account.address),
                        value: 100_000_000_000n,
                    }).decodedCall
            ),
        }),
        keyringPairFromUri(`//${funders[suite]}`)
    );
}
