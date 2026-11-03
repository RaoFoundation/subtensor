import {
    createKeyringPairFromMnemonic as createSdkKeyringPairFromMnemonic,
    createKeyringPairFromUri as createSdkKeyringPairFromUri,
    generateKeyringPair as generateSdkKeyringPair,
} from "@bittensor/sdk";
import type { KeyringPair } from "@moonwall/util";
import type { subtensor } from "@polkadot-api/descriptors";
import type { PolkadotSigner, TypedApi } from "polkadot-api";
import { getPolkadotSigner } from "polkadot-api/signer";

export const getAccountNonce = async (api: TypedApi<typeof subtensor>, address: string): Promise<number> => {
    const account = await api.query.System.Account.getValue(address, { at: "best" });
    return account.nonce;
};

export function getSignerFromKeypair(keypair: KeyringPair): PolkadotSigner {
    const scheme = keypair.type === "ed25519" ? "Ed25519" : "Sr25519";
    return getPolkadotSigner(keypair.publicKey, scheme, (payload) => keypair.sign(payload));
}

/** Create a Moonwall/Polkadot.js-compatible pair backed entirely by the Rust SDK. */
export function keyringPairFromUri(uri: string, type: "sr25519" | "ed25519" = "sr25519"): KeyringPair {
    return createSdkKeyringPairFromUri(uri, type);
}

/** Create a compatible pair from a mnemonic without invoking JavaScript crypto. */
export function keyringPairFromMnemonic(mnemonic: string, type: "sr25519" | "ed25519" = "sr25519"): KeyringPair {
    return createSdkKeyringPairFromMnemonic(mnemonic, type);
}

/** Generate and derive an e2e account inside the Rust SDK. */
export function generateKeyringPair(type: "sr25519" | "ed25519" = "sr25519"): KeyringPair {
    return generateSdkKeyringPair(type);
}
