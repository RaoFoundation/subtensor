import type { KeyringPair } from "@moonwall/util";
import { xchacha20poly1305 } from "@noble/ciphers/chacha.js";
import type { subtensor } from "@polkadot-api/descriptors";
import { hexToU8a } from "@polkadot/util";
import { xxhashAsU8a } from "@polkadot/util-crypto";
import { randomBytes } from "ethers";
import { MlKem768 } from "mlkem";
import { type TypedApi, Binary } from "polkadot-api";
import { getSignerFromKeypair } from "./account.ts";
import { waitForFinalizedBlocks } from "./transactions.ts";

const keyToBytes = (key: unknown): Uint8Array => {
    if (key instanceof Uint8Array) {
        return key;
    }
    if (typeof key === "object" && key !== null && "asBytes" in key) {
        return (key as Binary).asBytes();
    }
    if (typeof key === "string") {
        return hexToU8a(key);
    }
    throw new Error(`Unexpected MEV shield key type: ${typeof key}`);
};

export const getNextKey = async (api: TypedApi<typeof subtensor>): Promise<Uint8Array | undefined> => {
    // Query at "best" (not default "finalized") because keys rotate every block
    // and finalized lags ~2 blocks behind best with GRANDPA. Using finalized
    // would return a stale key whose hash won't match CurrentKey/NextKey at
    // block-building time, causing InvalidShieldedTxPubKeyHash rejection.
    const key = await api.query.MevShield.NextKey.getValue({ at: "best" });
    if (!key) return undefined;
    return keyToBytes(key);
};

export const checkRuntime = async (api: TypedApi<typeof subtensor>) => {
    const ts1 = await api.query.Timestamp.Now.getValue();

    await waitForFinalizedBlocks(api, 1);

    const ts2 = await api.query.Timestamp.Now.getValue();

    const blockTimeMs = ts2 - ts1;

    const MIN_BLOCK_TIME_MS = 6000;
    // We check at least half of the block time length
    if (blockTimeMs < MIN_BLOCK_TIME_MS) {
        throw new Error(
            `Fast runtime detected (block time ~${blockTimeMs}ms < ${MIN_BLOCK_TIME_MS}ms). Rebuild with normal runtime before running MEV Shield tests.`
        );
    }
};

export const getCurrentKey = async (api: TypedApi<typeof subtensor>): Promise<Uint8Array | undefined> => {
    const key = await api.query.MevShield.CurrentKey.getValue({ at: "best" });
    if (!key) return undefined;
    return keyToBytes(key);
};

export const encryptTransaction = async (plaintext: Uint8Array, publicKey: Uint8Array): Promise<Uint8Array> => {
    const keyHash = xxhashAsU8a(publicKey, 128);

    const mlKem = new MlKem768();
    const [kemCt, sharedSecret] = await mlKem.encap(publicKey);

    const nonce = randomBytes(24);
    const chacha = xchacha20poly1305(sharedSecret, nonce);
    const aeadCt = chacha.encrypt(plaintext);

    const kemLenBytes = new Uint8Array(2);
    new DataView(kemLenBytes.buffer).setUint16(0, kemCt.length, true);

    return new Uint8Array([...keyHash, ...kemLenBytes, ...kemCt, ...nonce, ...aeadCt]);
};

export const submitEncrypted = async (
    api: TypedApi<typeof subtensor>,
    signer: KeyringPair,
    innerTxBytes: Uint8Array,
    publicKey: Uint8Array,
    nonce?: number
) => {
    const ciphertext = await encryptTransaction(innerTxBytes, publicKey);
    return submitEncryptedRaw(api, signer, ciphertext, nonce);
};

export const submitEncryptedRaw = async (
    api: TypedApi<typeof subtensor>,
    signer: KeyringPair,
    ciphertext: Uint8Array,
    nonce?: number
) => {
    const tx = api.tx.MevShield.submit_encrypted({
        ciphertext: Binary.fromBytes(ciphertext),
    });
    return tx.signAndSubmit(getSignerFromKeypair(signer), {
        ...(nonce !== undefined ? { nonce } : {}),
        mortality: { mortal: true, period: 8 },
    });
};
