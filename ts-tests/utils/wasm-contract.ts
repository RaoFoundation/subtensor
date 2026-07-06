import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import type { TypedApi } from "polkadot-api";
import { Binary } from "polkadot-api";
import { convertPublicKeyToSs58 } from "./address.ts";
import { getBalance } from "./balance.ts";
import { sendTransaction, waitForFinalizedBlocks, waitForTransactionWithRetry } from "./transactions.ts";

export const BITTENSOR_WASM_PATH = "./ink/bittensor.wasm";

export async function sendWasmContractExtrinsic(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
): Promise<void> {
    const tx = api.tx.Contracts.call({
        value: BigInt(0),
        dest: MultiAddress.Id(contractAddress),
        data: Binary.fromBytes(data.asBytes()),
        gas_limit: {
            ref_time: BigInt(10_000_000_000),
            proof_size: BigInt(10_000_000),
        },
        storage_deposit_limit: BigInt(1_000_000_000),
    });
    await waitForTransactionWithRetry(api, tx, coldkey, "contracts_call", 1);
    await waitForFinalizedBlocks(api, 1);
}

/** Submit a contract call without failing when the contract reverts (expected for atomic-failure tests). */
export async function sendWasmContractExtrinsicAllowFailure(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
): Promise<void> {
    const tx = api.tx.Contracts.call({
        value: BigInt(0),
        dest: MultiAddress.Id(contractAddress),
        data: Binary.fromBytes(data.asBytes()),
        gas_limit: {
            ref_time: BigInt(10_000_000_000),
            proof_size: BigInt(10_000_000),
        },
        storage_deposit_limit: BigInt(1_000_000_000),
    });
    await sendTransaction(tx, coldkey);
}

export async function instantiateWasmContract(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    wasmBytecode: Uint8Array,
    constructorData: { asBytes(): Uint8Array }
): Promise<string> {
    const tx = api.tx.Contracts.instantiate_with_code({
        code: Binary.fromBytes(wasmBytecode),
        storage_deposit_limit: BigInt(10_000_000),
        value: BigInt(0),
        gas_limit: {
            ref_time: BigInt(1_000_000_000),
            proof_size: BigInt(1_000_000),
        },
        data: Binary.fromBytes(constructorData.asBytes()),
        salt: Binary.fromHex("0x"),
    });

    const result = await sendTransaction(tx, coldkey);
    if (!result.success) {
        throw new Error(`instantiate_with_code failed: ${result.errorMessage ?? "unknown error"}`);
    }

    const instantiatedEvents = await api.event.Contracts.Instantiated.filter(result.events);
    if (instantiatedEvents.length === 0) {
        throw new Error("No Contracts.Instantiated events found after instantiate_with_code");
    }

    return instantiatedEvents[0].contract;
}

export { convertPublicKeyToSs58, getBalance };
