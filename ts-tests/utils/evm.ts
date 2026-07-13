import type { subtensor } from "@polkadot-api/descriptors";
import { ethers } from "ethers";
import type { TypedApi } from "polkadot-api";
import { keyringPairFromUri } from "./account.ts";
import { waitForTransactionWithRetry } from "./transactions.js";

export async function disableWhiteListCheck(api: TypedApi<typeof subtensor>, disabled: boolean): Promise<void> {
    const value = await api.query.EVM.DisableWhitelistCheck.getValue();
    if (value === disabled) {
        return;
    }

    const alice = keyringPairFromUri("//Alice");
    const internalCall = api.tx.EVM.disable_whitelist({ disabled });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "disable_whitelist", 5);
}

class UncachedNonceWallet extends ethers.Wallet {
    constructor(
        privateKey: string,
        private readonly rpcProvider: ethers.JsonRpcProvider
    ) {
        super(privateKey, rpcProvider);
    }

    /**
     * Bypass ethers' 250ms request cache for nonce reads. Development blocks
     * seal every 100ms, so a cached pending nonce can already be stale when the
     * next transaction is populated and cause a spurious "nonce too low".
     */
    override async getNonce(blockTag: ethers.BlockTag = "latest"): Promise<number> {
        if (blockTag !== "pending") {
            return super.getNonce(blockTag);
        }

        const nonce = await this.rpcProvider.send("eth_getTransactionCount", [this.address, "pending"]);
        return ethers.getNumber(nonce, "nonce");
    }
}

export function createEthersWallet(provider: ethers.JsonRpcProvider): ethers.Wallet {
    const account = ethers.Wallet.createRandom();
    return new UncachedNonceWallet(account.privateKey, provider);
}

/** Read an uncached latest balance directly from the node. */
export async function getEthBalance(provider: ethers.JsonRpcProvider, address: string): Promise<bigint> {
    return BigInt(await provider.send("eth_getBalance", [address, "latest"]));
}

/**
 * Wait for Frontier's latest-state view to expose an exact balance.
 *
 * Raw RPC is intentional: ethers caches some high-level reads briefly, which
 * can return the pre-transaction value when development blocks are very fast.
 */
export async function waitForEthBalance(
    provider: ethers.JsonRpcProvider,
    address: string,
    expected: bigint,
    timeoutMs = 30_000
): Promise<bigint> {
    const deadline = Date.now() + timeoutMs;
    let actual = 0n;

    while (Date.now() < deadline) {
        actual = await getEthBalance(provider, address);
        if (actual === expected) {
            return actual;
        }
        await new Promise((resolve) => setTimeout(resolve, 100));
    }

    throw new Error(`Timed out waiting for ${address} balance ${expected}; last observed ${actual}`);
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

    const alice = keyringPairFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_evm_chain_id({ chain_id: chainId });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_evm_chain_id", 5);
}
