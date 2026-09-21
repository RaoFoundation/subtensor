import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import type { TypedApi } from "polkadot-api";
import { Binary } from "polkadot-api";
import { convertPublicKeyToSs58 } from "./address.ts";
import { getBalance } from "./balance.ts";
import {
    sendTransaction,
    type TransactionResult,
    waitForFinalizedBlocks,
    waitForTransactionWithRetry,
} from "./transactions.ts";

export const BITTENSOR_WASM_PATH = "./ink/bittensor.wasm";

export type WasmGasLimit = { ref_time: bigint; proof_size: bigint };

/** Never size a call below the constants every message fit under before spec 464. */
const MIN_CALL_GAS_LIMIT: WasmGasLimit = {
    ref_time: BigInt(10_000_000_000),
    proof_size: BigInt(10_000_000),
};

/** 25% over the dry run covers state drift between the estimate and the executing block. */
function withHeadroom(required: bigint, floor: bigint): bigint {
    const sized = required + required / BigInt(4);
    return sized > floor ? sized : floor;
}

/**
 * Size the gas limit for a contract call from a dry run instead of a constant.
 *
 * The chain extension charges the declared weight of the runtime call it dispatches
 * before dispatching it (a stake exit is `remove_stake` plus `staking_hotkeys_walk_bound`,
 * ~93e9 ref_time since spec 464), so any fixed limit goes stale on the next reweigh.
 * `ContractsApi.call` reports `gas_required` as the peak gas the execution needed, which
 * includes that up-front charge, so the limit follows the declared weight automatically.
 */
export async function estimateContractCallGasLimit(
    api: TypedApi<typeof subtensor>,
    callerAddress: string,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
): Promise<WasmGasLimit> {
    const dryRun = await api.apis.ContractsApi.call(
        callerAddress,
        contractAddress,
        BigInt(0),
        undefined,
        undefined,
        Binary.fromBytes(data.asBytes())
    );
    return {
        ref_time: withHeadroom(dryRun.gas_required.ref_time, MIN_CALL_GAS_LIMIT.ref_time),
        proof_size: withHeadroom(dryRun.gas_required.proof_size, MIN_CALL_GAS_LIMIT.proof_size),
    };
}

async function buildContractCallTx(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
) {
    const gasLimit = await estimateContractCallGasLimit(
        api,
        convertPublicKeyToSs58(coldkey.publicKey),
        contractAddress,
        data
    );
    return api.tx.Contracts.call({
        value: BigInt(0),
        dest: MultiAddress.Id(contractAddress),
        data: Binary.fromBytes(data.asBytes()),
        gas_limit: gasLimit,
        storage_deposit_limit: BigInt(1_000_000_000),
    });
}

export async function sendWasmContractExtrinsic(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
): Promise<void> {
    const tx = await buildContractCallTx(api, coldkey, contractAddress, data);
    await waitForTransactionWithRetry(api, tx, coldkey, "contracts_call", 1);
    await waitForFinalizedBlocks(api, 1);
}

/**
 * Like sendWasmContractExtrinsic, but returns the finalized-transaction result
 * so callers can assert on emitted pallet events instead of racing state reads
 * against emission or finality.
 */
export async function sendWasmContractExtrinsicWithEvents(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
): Promise<TransactionResult> {
    const tx = await buildContractCallTx(api, coldkey, contractAddress, data);
    const result = await sendTransaction(tx, coldkey);
    if (!result.success) {
        throw new Error(`contracts_call failed: ${result.errorMessage ?? "unknown error"}`);
    }
    await waitForFinalizedBlocks(api, 1);
    return result;
}

/** Submit a contract call without failing when the contract reverts (expected for atomic-failure tests). */
export async function sendWasmContractExtrinsicAllowFailure(
    api: TypedApi<typeof subtensor>,
    coldkey: KeyringPair,
    contractAddress: string,
    data: { asBytes(): Uint8Array }
): Promise<void> {
    const tx = await buildContractCallTx(api, coldkey, contractAddress, data);
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
