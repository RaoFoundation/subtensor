import { subtensor } from "@polkadot-api/descriptors";
import { Keyring } from "@polkadot/keyring";
import { ethers } from "ethers";
import type { TypedApi } from "polkadot-api";
import { waitForTransactionWithRetry } from "./transactions.js";

export async function disableWhiteListCheck(api: TypedApi<typeof subtensor>, disabled: boolean): Promise<void> {
    const value = await api.query.EVM.DisableWhitelistCheck.getValue();
    if (value === disabled) {
        return;
    }

    const alice = new Keyring({ type: "sr25519" }).addFromUri("//Alice");
    const internalCall = api.tx.EVM.disable_whitelist({ disabled });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "disable_whitelist", 5);
}

export function createEthersWallet(provider: ethers.JsonRpcProvider): ethers.Wallet {
    const account = ethers.Wallet.createRandom();
    return new ethers.Wallet(account.privateKey, provider);
}

export async function getEthBalance(provider: ethers.Provider, address: string): Promise<bigint> {
    return provider.getBalance(address);
}

/** Read chain ID via RPC without ethers' cached-network checks. */
export async function getEthChainId(provider: ethers.JsonRpcProvider): Promise<bigint> {
    const chainId = await provider.send("eth_chainId", []);
    return BigInt(chainId);
}

export async function forceSetChainID(api: TypedApi<typeof subtensor>, chainId: bigint): Promise<void> {
    const value = await api.query.EVMChainId.ChainId.getValue();
    if (value === chainId) {
        return;
    }

    const alice = new Keyring({ type: "sr25519" }).addFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_evm_chain_id({ chain_id: chainId });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_evm_chain_id", 5);
}
