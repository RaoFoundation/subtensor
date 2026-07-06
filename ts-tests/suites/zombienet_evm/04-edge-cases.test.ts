import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import { subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import { ethers } from "ethers";
import type { TypedApi } from "polkadot-api";
import {
    convertH160ToSS58,
    convertPublicKeyToSs58,
    createEthersWallet,
    disableWhiteListCheck,
    forceSetBalance,
    forceSetChainID,
    generateKeyringPair,
    getEthChainId,
    IBALANCETRANSFER_ADDRESS,
    IBalanceTransferABI,
    raoToEth,
    sendTransaction,
    tao,
    waitForFinalizedBlocks,
} from "../../utils";

const INIT_CHAIN_ID = 42;

describeSuite({
    id: "edge-cases",
    title: "EVM edge case tests",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let provider: ethers.JsonRpcProvider;
        let ethWallet: ethers.Wallet;
        let ethWallet2: ethers.Wallet;
        let transferTarget: KeyringPair;
        let nonSudoSigner: KeyringPair;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            provider = context.ethers("EVM").provider as ethers.JsonRpcProvider;

            ethWallet = createEthersWallet(provider);
            ethWallet2 = createEthersWallet(provider);
            transferTarget = generateKeyringPair("sr25519");
            nonSudoSigner = generateKeyringPair("sr25519");

            await forceSetBalance(api, convertH160ToSS58(ethWallet.address));
            await forceSetBalance(api, convertPublicKeyToSs58(nonSudoSigner.publicKey));
            await disableWhiteListCheck(api, true);
            await waitForFinalizedBlocks(api, 1);
        }, 300000);

        async function getChainId(): Promise<bigint> {
            return getEthChainId(provider);
        }

        it({
            id: "T01",
            title: "EVM chain id update is ok",
            test: async () => {
                let chainId = await getChainId();
                expect(chainId).toEqual(BigInt(INIT_CHAIN_ID));

                const newChainId = BigInt(100);
                await forceSetChainID(api, newChainId);
                await waitForFinalizedBlocks(api, 1);

                chainId = await getChainId();
                expect(chainId).toEqual(newChainId);

                await forceSetChainID(api, BigInt(INIT_CHAIN_ID));
                await waitForFinalizedBlocks(api, 1);

                chainId = await getChainId();
                expect(chainId).toEqual(BigInt(INIT_CHAIN_ID));
            },
        });

        it({
            id: "T02",
            title: "EVM chain id is the same, only sudo can change it",
            test: async () => {
                let chainId = await getChainId();
                expect(chainId).toEqual(BigInt(INIT_CHAIN_ID));

                const tx = api.tx.AdminUtils.sudo_set_evm_chain_id({ chain_id: BigInt(100) });
                const result = await sendTransaction(tx, nonSudoSigner);
                expect(result.success).toBe(false);

                chainId = await getChainId();
                expect(chainId).toEqual(BigInt(INIT_CHAIN_ID));
            },
        });

        it({
            id: "T03",
            title: "Can replace simple transfer transaction",
            test: async () => {
                const transferBalance = raoToEth(tao(1));
                const gasPrice = BigInt(10e9);
                const gasLimit = BigInt(1_000_000);
                const nonce = await provider.getTransactionCount(ethWallet.address);

                for (let i = 1; i < 10; i++) {
                    try {
                        await ethWallet.sendTransaction({
                            to: ethWallet2.address,
                            value: transferBalance,
                            nonce,
                            gasPrice: gasPrice * BigInt(i),
                            gasLimit: gasLimit * BigInt(i),
                        });
                    } catch {
                        // Previous transaction may have been mined with the same nonce.
                    }
                    await new Promise((resolve) => setTimeout(resolve, 10));
                }

                await forceSetBalance(api, convertH160ToSS58(ethWallet.address));
                await waitForFinalizedBlocks(api, 1);
            },
        });

        it({
            id: "T04",
            title: "Can replace precompile call transaction",
            test: async () => {
                const contract = new ethers.Contract(IBALANCETRANSFER_ADDRESS, IBalanceTransferABI, ethWallet);
                const transferBalance = raoToEth(tao(1));
                const gasPrice = BigInt(10e9);
                const gasLimit = BigInt(1_000_000);
                const nonce = await provider.getTransactionCount(ethWallet.address);

                for (let i = 1; i < 10; i++) {
                    try {
                        await contract.transfer(transferTarget.publicKey, {
                            value: transferBalance,
                            nonce,
                            gasPrice: gasPrice * BigInt(i),
                            gasLimit: gasLimit * BigInt(i),
                        });
                    } catch {
                        // Previous transaction may have been mined with the same nonce.
                    }
                    await new Promise((resolve) => setTimeout(resolve, 10));
                }

                await forceSetBalance(api, convertH160ToSS58(ethWallet.address));
                await waitForFinalizedBlocks(api, 1);
            },
        });
    },
});
