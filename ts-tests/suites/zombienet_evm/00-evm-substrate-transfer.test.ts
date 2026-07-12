import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import { ethers } from "ethers";
import type { TypedApi } from "polkadot-api";
import { Binary } from "polkadot-api";
import {
    bigintToRao,
    convertH160ToSS58,
    convertPublicKeyToSs58,
    createEthersWallet,
    disableWhiteListCheck,
    ethAddressToH160,
    forceSetBalance,
    generateKeyringPair,
    getBalance,
    getEthBalance,
    GWEI,
    IBALANCETRANSFER_ADDRESS,
    IBalanceTransferABI,
    MAX_TX_FEE,
    raoToEth,
    sendTransaction,
    ss58ToEthAddress,
    ss58ToH160,
    tao,
    waitForFinalizedBlocks,
    waitForTransactionWithRetry,
    WITHDRAW_CONTRACT_ABI,
    WITHDRAW_CONTRACT_BYTECODE,
} from "../../utils";

async function estimateTransactionCost(provider: ethers.Provider, tx: ethers.TransactionRequest): Promise<bigint> {
    const feeData = await provider.getFeeData();
    const estimatedGas = await provider.estimateGas(tx);
    const gasPrice = feeData.gasPrice ?? feeData.maxFeePerGas;
    if (gasPrice == null) {
        return estimatedGas;
    }
    return estimatedGas * gasPrice;
}

function expectWithinTxFee(actual: bigint, expected: bigint): void {
    const diff = actual > expected ? actual - expected : expected - actual;
    expect(diff).toBeLessThan(MAX_TX_FEE);
}

async function transferAndGetFee(
    wallet: ethers.Wallet,
    wallet2: ethers.Wallet,
    provider: ethers.Provider,
    maxFeePerGas: bigint,
    maxPriorityFeePerGas: bigint
): Promise<bigint> {
    const ethBalanceBefore = await getEthBalance(provider, wallet.address);
    const tx = {
        to: wallet2.address,
        value: raoToEth(tao(1)).toString(),
        maxPriorityFeePerGas: maxPriorityFeePerGas.toString(),
        maxFeePerGas: maxFeePerGas.toString(),
        gasLimit: 21000,
    };

    const txResponse = await wallet.sendTransaction(tx);
    const receipt = await txResponse.wait();
    expect(receipt?.status).toEqual(1);

    const ethBalanceAfter = await getEthBalance(provider, wallet.address);
    return ethBalanceBefore - ethBalanceAfter - raoToEth(tao(1));
}

describeSuite({
    id: "evm-substrate-transfer-basic",
    title: "Basic EVM-Substrate Transfer Tests",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let ethWallet: ethers.Wallet;
        let ethWallet2: ethers.Wallet;
        let signer: KeyringPair;
        let provider: ethers.JsonRpcProvider;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);

            provider = context.ethers("EVM").provider as ethers.JsonRpcProvider;
            ethWallet = createEthersWallet(provider);
            ethWallet2 = createEthersWallet(provider);

            signer = generateKeyringPair("sr25519");
            await forceSetBalance(api, convertPublicKeyToSs58(signer.publicKey));
            await forceSetBalance(api, convertH160ToSS58(ethWallet.address));
            await forceSetBalance(api, convertH160ToSS58(ethWallet2.address));
            await disableWhiteListCheck(api, true);
            await waitForFinalizedBlocks(api, 1);
        }, 300000);

        it({
            id: "T01",
            title: "Can transfer token from EVM to EVM",
            test: async () => {
                const senderBalanceBefore = await getEthBalance(provider, ethWallet.address);
                const receiverBalanceBefore = await getEthBalance(provider, ethWallet2.address);

                const transferAmount = raoToEth(tao(1));
                const tx: ethers.TransactionRequest = {
                    to: ethWallet2.address,
                    value: transferAmount,
                };

                const txFee = await estimateTransactionCost(provider, tx);

                const txResponse = await ethWallet.sendTransaction(tx);
                const receipt = await txResponse.wait();
                expect(receipt).toBeDefined();
                expect(receipt!.status).toEqual(1);

                const senderBalanceAfter = await getEthBalance(provider, ethWallet.address);
                const receiverBalanceAfter = await getEthBalance(provider, ethWallet2.address);

                expect(senderBalanceAfter).toEqual(senderBalanceBefore - transferAmount - txFee);
                expect(receiverBalanceAfter).toEqual(receiverBalanceBefore + transferAmount);
            },
        });

        it({
            id: "T02",
            title: "Can transfer token from Substrate to EVM",
            test: async () => {
                const ss58Address = convertH160ToSS58(ethWallet.address);
                const receiverBalance = await getEthBalance(provider, ethWallet.address);
                const transferBalance = tao(1);

                const tx = api.tx.Balances.transfer_keep_alive({
                    value: transferBalance,
                    dest: MultiAddress.Id(ss58Address),
                });
                await waitForTransactionWithRetry(api, tx, signer, "substrate_to_evm");

                const receiverBalanceAfter = await getEthBalance(provider, ethWallet.address);
                expect(receiverBalanceAfter).toEqual(receiverBalance + raoToEth(transferBalance));
            },
        });

        it({
            id: "T03",
            title: "Can transfer token from EVM to Substrate",
            test: async () => {
                const contract = new ethers.Contract(IBALANCETRANSFER_ADDRESS, IBalanceTransferABI, ethWallet);
                const signerSs58 = convertPublicKeyToSs58(signer.publicKey);

                const senderBalance = await getEthBalance(provider, ethWallet.address);
                const receiverBalance = await getBalance(api, signerSs58);
                const transferBalance = raoToEth(tao(1));

                const tx = await contract.transfer(signer.publicKey, { value: transferBalance.toString() });
                const receipt = await tx.wait();
                expect(receipt?.status).toEqual(1);

                await waitForFinalizedBlocks(api, 2);

                const senderBalanceAfter = await getEthBalance(provider, ethWallet.address);
                const receiverBalanceAfter = await getBalance(api, signerSs58);

                expectWithinTxFee(senderBalanceAfter, senderBalance - transferBalance);
                expect(receiverBalance).toEqual(receiverBalanceAfter - tao(1));
            },
        });

        it({
            id: "T04",
            title: "Transfer from EVM to substrate using evm::withdraw",
            test: async () => {
                const ss58Address = convertPublicKeyToSs58(signer.publicKey);
                const senderBalance = await getBalance(api, ss58Address);
                const ethAddress = ss58ToH160(ss58Address);

                const ethTransfer = {
                    to: ss58ToEthAddress(ss58Address),
                    value: raoToEth(tao(2)).toString(),
                };
                const fundReceipt = await (await ethWallet.sendTransaction(ethTransfer)).wait();
                expect(fundReceipt?.status).toEqual(1);

                const tx = api.tx.EVM.withdraw({ address: ethAddress, value: tao(1) });
                const txFee = (await tx.getPaymentInfo(ss58Address)).partial_fee;

                await waitForTransactionWithRetry(api, tx, signer, "evm_withdraw", 5);

                const senderBalanceAfterWithdraw = await getBalance(api, ss58Address);
                expect(senderBalance).toEqual(senderBalanceAfterWithdraw - tao(1) + txFee);
            },
        });

        it({
            id: "T05",
            title: "Transfer from EVM to substrate using evm::call",
            test: async () => {
                const ss58Address = convertPublicKeyToSs58(signer.publicKey);
                const ethAddress = ss58ToH160(ss58Address);

                const ethTransfer = {
                    to: ss58ToEthAddress(ss58Address),
                    value: raoToEth(tao(2)).toString(),
                };
                const fundReceipt = await (await ethWallet.sendTransaction(ethTransfer)).wait();
                expect(fundReceipt?.status).toEqual(1);

                const source = ethAddress;
                const target = ethAddressToH160(ethWallet.address);
                const receiverBalance = await getEthBalance(provider, ethWallet.address);

                const tx = api.tx.EVM.call({
                    source,
                    target,
                    value: [raoToEth(tao(1)), tao(0), tao(0), tao(0)],
                    gas_limit: BigInt(1000000),
                    max_fee_per_gas: [BigInt(10e9), BigInt(0), BigInt(0), BigInt(0)],
                    max_priority_fee_per_gas: undefined,
                    // PAPI encodes this field with the Binary codec despite the Uint8Array annotation.
                    input: Binary.fromText("") as unknown as Uint8Array,
                    nonce: undefined,
                    access_list: [],
                    authorization_list: [],
                });

                await waitForTransactionWithRetry(api, tx, signer, "evm_call");

                const receiverBalanceAfterCall = await getEthBalance(provider, ethWallet.address);
                expect(receiverBalanceAfterCall).toEqual(receiverBalance + raoToEth(tao(1)));
            },
        });

        it({
            id: "T06",
            title: "Forward value in smart contract",
            test: async () => {
                const contractFactory = new ethers.ContractFactory(
                    WITHDRAW_CONTRACT_ABI,
                    WITHDRAW_CONTRACT_BYTECODE,
                    ethWallet
                );
                const contract = await contractFactory.deploy();
                await contract.waitForDeployment();
                await waitForFinalizedBlocks(api, 1);

                const contractAddress = contract.target.toString();
                const code = await provider.getCode(contractAddress);
                expect(code).toBeDefined();
                expect(code.length).toBeGreaterThan(100);

                const ethTransfer = {
                    to: contractAddress,
                    value: raoToEth(tao(2)).toString(),
                };
                const fundReceipt = await (await ethWallet.sendTransaction(ethTransfer)).wait();
                expect(fundReceipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 1);

                const contractBalance = await getEthBalance(provider, contractAddress);
                const callerBalance = await getEthBalance(provider, ethWallet.address);

                const contractForCall = new ethers.Contract(contractAddress, WITHDRAW_CONTRACT_ABI, ethWallet);
                const withdrawTx = await contractForCall.withdraw(raoToEth(tao(1)).toString());
                const withdrawReceipt = await withdrawTx.wait();
                expect(withdrawReceipt?.status).toEqual(1);

                const contractBalanceAfterWithdraw = await getEthBalance(provider, contractAddress);
                const callerBalanceAfterWithdraw = await getEthBalance(provider, ethWallet.address);

                expectWithinTxFee(callerBalanceAfterWithdraw, callerBalance + raoToEth(tao(1)));
                expect(contractBalance).toEqual(contractBalanceAfterWithdraw + raoToEth(tao(1)));
            },
        });

        it({
            id: "T07",
            title: "Transfer full balance",
            test: async () => {
                const ethBalance = await getEthBalance(provider, ethWallet.address);
                const receiverBalance = await getEthBalance(provider, ethWallet2.address);
                const txPrice = await estimateTransactionCost(provider, {
                    to: ethWallet2.address,
                    value: ethBalance.toString(),
                });
                const finalTx = {
                    to: ethWallet2.address,
                    value: (ethBalance - txPrice).toString(),
                };

                let rejected = false;
                try {
                    const txResponse = await ethWallet.sendTransaction(finalTx);
                    await txResponse.wait();
                } catch (error) {
                    rejected = true;
                    if (error instanceof Error) {
                        expect(
                            (error as { code?: string }).code === "INSUFFICIENT_FUNDS" ||
                                error.message.includes("insufficient funds")
                        ).toBe(true);
                    }
                }
                expect(rejected).toBe(true);

                const receiverBalanceAfterTransfer = await getEthBalance(provider, ethWallet2.address);
                expect(receiverBalanceAfterTransfer).toEqual(receiverBalance);
            },
        });

        it({
            id: "T08",
            title: "Transfer more than owned balance should fail",
            test: async () => {
                const ethBalance = await getEthBalance(provider, ethWallet.address);
                const receiverBalance = await getEthBalance(provider, ethWallet2.address);
                const tx = {
                    to: ethWallet2.address,
                    value: (ethBalance + raoToEth(tao(1))).toString(),
                };

                let rejected = false;
                try {
                    const txResponse = await ethWallet.sendTransaction(tx);
                    await txResponse.wait();
                } catch (error) {
                    rejected = true;
                    if (error instanceof Error) {
                        expect(
                            (error as { code?: string }).code === "INSUFFICIENT_FUNDS" ||
                                error.message.includes("insufficient funds")
                        ).toBe(true);
                    }
                }
                expect(rejected).toBe(true);

                const receiverBalanceAfterTransfer = await getEthBalance(provider, ethWallet2.address);
                expect(receiverBalanceAfterTransfer).toEqual(receiverBalance);
            },
        });

        it({
            id: "T09",
            title: "Transfer more than u64::max in substrate equivalent should receive error response",
            test: async () => {
                const receiverBalance = await getEthBalance(provider, ethWallet2.address);
                const oversize = raoToEth(BigInt(2) ** BigInt(64));

                let ethRejected = false;
                try {
                    const txResponse = await ethWallet.sendTransaction({
                        to: ethWallet2.address,
                        value: oversize.toString(),
                    });
                    await txResponse.wait();
                } catch (error) {
                    ethRejected = true;
                    if (error instanceof Error) {
                        expect(
                            (error as { code?: string }).code === "INSUFFICIENT_FUNDS" ||
                                error.message.includes("insufficient funds")
                        ).toBe(true);
                    }
                }
                expect(ethRejected).toBe(true);

                const contract = new ethers.Contract(IBALANCETRANSFER_ADDRESS, IBalanceTransferABI, ethWallet);
                let precompileRejected = false;
                try {
                    const tx = await contract.transfer(signer.publicKey, { value: oversize.toString() });
                    await tx.wait();
                } catch (error) {
                    precompileRejected = true;
                    if (error instanceof Error) {
                        expect(error.message.includes("revert") || error.message.includes("CALL_EXCEPTION")).toBe(true);
                    }
                }
                expect(precompileRejected).toBe(true);

                let balanceTxRejected = false;
                try {
                    const dest = convertH160ToSS58(ethWallet2.address);
                    const tx = api.tx.Balances.transfer_keep_alive({
                        value: bigintToRao(BigInt(2) ** BigInt(64)),
                        dest: MultiAddress.Id(dest),
                    });
                    const result = await sendTransaction(tx, signer);
                    balanceTxRejected = !result.success;
                } catch {
                    balanceTxRejected = true;
                }
                expect(balanceTxRejected).toBe(true);

                let withdrawRejected = false;
                try {
                    const dest = ethAddressToH160(ethWallet2.address);
                    const tx = api.tx.EVM.withdraw({
                        value: bigintToRao(BigInt(2) ** BigInt(64)),
                        address: dest,
                    });
                    const result = await sendTransaction(tx, signer);
                    withdrawRejected = !result.success;
                } catch {
                    withdrawRejected = true;
                }
                expect(withdrawRejected).toBe(true);

                let evmCallRejected = false;
                try {
                    const source = ethAddressToH160(ethWallet.address);
                    const target = ethAddressToH160(ethWallet2.address);
                    const tx = api.tx.EVM.call({
                        source,
                        target,
                        value: [raoToEth(tao(1)), tao(0), tao(0), tao(1)],
                        gas_limit: BigInt(1000000),
                        max_fee_per_gas: [BigInt(10e9), BigInt(0), BigInt(0), BigInt(0)],
                        max_priority_fee_per_gas: undefined,
                        input: Binary.fromText("") as unknown as Uint8Array,
                        nonce: undefined,
                        access_list: [],
                        authorization_list: [],
                    });
                    const result = await sendTransaction(tx, signer);
                    evmCallRejected = !result.success;
                } catch {
                    evmCallRejected = true;
                }
                expect(evmCallRejected).toBe(true);

                const receiverBalanceAfter = await getEthBalance(provider, ethWallet2.address);
                expect(receiverBalanceAfter).toEqual(receiverBalance);
            },
        });

        it({
            id: "T10",
            title: "Gas price should be 10 GWei",
            test: async () => {
                const feeData = await provider.getFeeData();
                expect(feeData.gasPrice).toEqual(BigInt(10000000000));
            },
        });

        it({
            id: "T11",
            title: "max_fee_per_gas and max_priority_fee_per_gas affect transaction fee properly",
            test: async () => {
                const testCases: [number, number, bigint][] = [
                    [10, 0, BigInt(21000 * 10) * BigInt(1e9)],
                    [10, 10, BigInt(21000 * 10) * BigInt(1e9)],
                    [11, 0, BigInt(21000 * 10) * BigInt(1e9)],
                ];

                for (const [maxFeeGwei, maxPriorityGwei, expectedFee] of testCases) {
                    const actualFee = await transferAndGetFee(
                        ethWallet,
                        ethWallet2,
                        provider,
                        GWEI * BigInt(maxFeeGwei),
                        GWEI * BigInt(maxPriorityGwei)
                    );
                    expect(actualFee).toEqual(expectedFee);
                }
            },
        });

        it({
            id: "T12",
            title: "Low max_fee_per_gas gets transaction rejected",
            test: async () => {
                let rejected = false;
                try {
                    await transferAndGetFee(ethWallet, ethWallet2, provider, GWEI * BigInt(9), BigInt(0));
                } catch (error) {
                    rejected = true;
                    if (error instanceof Error) {
                        expect(error.message.includes("gas price less than block base fee")).toBe(true);
                    }
                }
                expect(rejected).toBe(true);
            },
        });

        it({
            id: "T13",
            title: "max_fee_per_gas lower than max_priority_fee_per_gas gets transaction rejected",
            test: async () => {
                let rejected = false;
                try {
                    await transferAndGetFee(ethWallet, ethWallet2, provider, GWEI * BigInt(10), GWEI * BigInt(11));
                } catch (error) {
                    rejected = true;
                    if (error instanceof Error) {
                        expect(error.message.includes("priorityFee cannot be more than maxFee")).toBe(true);
                    }
                }
                expect(rejected).toBe(true);
            },
        });
    },
});
