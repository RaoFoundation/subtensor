import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import { MultiAddress, subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import { u8aToHex } from "@polkadot/util";
import { decodeAddress } from "@polkadot/util-crypto";
import { ethers } from "ethers";
import type { TypedApi } from "polkadot-api";
import {
    addNewSubnetwork,
    ALPHA_POOL_CONTRACT_ABI,
    ALPHA_POOL_CONTRACT_BYTECODE,
    BRIDGE_TOKEN_CONTRACT_ABI,
    BRIDGE_TOKEN_CONTRACT_BYTECODE,
    burnedRegister,
    convertH160ToPublicKey,
    convertH160ToSS58,
    convertPublicKeyToSs58,
    createEthersWallet,
    disableWhiteListCheck,
    forceSetBalance,
    generateKeyringPair,
    getBalance,
    getProxies,
    getStake,
    IPROXY_ADDRESS,
    IProxyABI,
    ISTAKING_V2_ADDRESS,
    IStakingV2ABI,
    raoToEth,
    STAKE_WRAP_ABI,
    STAKE_WRAP_BYTECODE,
    startCall,
    sudoSetLockReductionInterval,
    tao,
    waitForFinalizedBlocks,
} from "../../utils";

const DEPLOYED_BYTECODE_PREFIX = "0x60806040523480156";

async function expectDeployedContract(provider: ethers.Provider, contractAddress: string): Promise<void> {
    const code = await provider.getCode(contractAddress);
    expect(code).toBeDefined();
    expect(code.length).toBeGreaterThan(100);
    expect(code.includes(DEPLOYED_BYTECODE_PREFIX)).toBe(true);
}

export async function getTransferCallCode(
    api: TypedApi<typeof subtensor>,
    receiver: KeyringPair,
    transferAmount: number
): Promise<number[]> {
    const unsignedTx = api.tx.Balances.transfer_keep_alive({
        dest: MultiAddress.Id(convertPublicKeyToSs58(receiver.publicKey)),
        value: BigInt(transferAmount),
    });
    const encodedCallDataBytes = await unsignedTx.getEncodedData();
    return [...encodedCallDataBytes.asBytes()];
}

describeSuite({
    id: "contract-deploy-call",
    title: "Contract deploy and precompile call tests",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let provider: ethers.JsonRpcProvider;
        let ethWallet: ethers.Wallet;
        let stakeWallet: ethers.Wallet;
        let proxyWallet1: ethers.Wallet;
        let proxyWallet2: ethers.Wallet;
        let proxyWallet3: ethers.Wallet;
        let proxyWallet4: ethers.Wallet;
        let pureProxyReceiver: KeyringPair;
        let delegateProxyReceiver: KeyringPair;
        let hotkey: KeyringPair;
        let coldkey: KeyringPair;
        let netuid: number;
        let subnetReady = false;
        let proxyWalletsReady = false;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            provider = context.ethers("EVM").provider as ethers.JsonRpcProvider;
            ethWallet = createEthersWallet(provider);
            await forceSetBalance(api, convertH160ToSS58(ethWallet.address));
            await disableWhiteListCheck(api, true);
            await waitForFinalizedBlocks(api, 1);
        }, 300000);

        async function ensureSubnetReady(): Promise<void> {
            if (subnetReady) {
                return;
            }

            hotkey = generateKeyringPair("sr25519");
            coldkey = generateKeyringPair("sr25519");

            await sudoSetLockReductionInterval(api, 1);
            await forceSetBalance(api, convertPublicKeyToSs58(hotkey.publicKey));
            await forceSetBalance(api, convertPublicKeyToSs58(coldkey.publicKey));

            netuid = await addNewSubnetwork(api, hotkey, coldkey);
            await startCall(api, netuid, coldkey);
            await burnedRegister(api, netuid, convertH160ToSS58(ethWallet.address), coldkey);
            await waitForFinalizedBlocks(api, 1);
            subnetReady = true;
        }

        async function ensureProxyWalletsReady(): Promise<void> {
            if (proxyWalletsReady) {
                return;
            }

            stakeWallet = createEthersWallet(provider);
            proxyWallet1 = createEthersWallet(provider);
            proxyWallet2 = createEthersWallet(provider);
            proxyWallet3 = createEthersWallet(provider);
            proxyWallet4 = createEthersWallet(provider);
            pureProxyReceiver = generateKeyringPair("sr25519");
            delegateProxyReceiver = generateKeyringPair("sr25519");

            for (const wallet of [stakeWallet, proxyWallet1, proxyWallet2, proxyWallet3, proxyWallet4]) {
                await forceSetBalance(api, convertH160ToSS58(wallet.address));
            }
            await waitForFinalizedBlocks(api, 1);
            proxyWalletsReady = true;
        }

        async function deployAndFundStakeWrap(wallet: ethers.Wallet): Promise<ethers.Contract> {
            const contractFactory = new ethers.ContractFactory(STAKE_WRAP_ABI, STAKE_WRAP_BYTECODE, wallet);
            const contract = await contractFactory.deploy();
            await contract.waitForDeployment();

            const txResponse = await wallet.sendTransaction({
                to: contract.target.toString(),
                value: raoToEth(tao(10000)),
            });
            await txResponse.wait();
            await waitForFinalizedBlocks(api, 1);

            return new ethers.Contract(contract.target.toString(), STAKE_WRAP_ABI, wallet);
        }

        async function waitForPureProxyCount(delegateSs58: string, expectedCount: number): Promise<string[]> {
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const proxies = await getProxies(api, delegateSs58);
                if (proxies.length === expectedCount) {
                    return proxies;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const proxies = await getProxies(api, delegateSs58);
            expect(proxies.length).toEqual(expectedCount);
            return proxies;
        }

        async function waitForProxyDelegates(realSs58: string, expectedCount: number): Promise<string[]> {
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const proxies = await api.query.Proxy.Proxies.getValue(realSs58);
                const delegates = proxies[0].map((proxy) => proxy.delegate);
                if (delegates.length === expectedCount) {
                    return delegates;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const proxies = await api.query.Proxy.Proxies.getValue(realSs58);
            const delegates = proxies[0].map((proxy) => proxy.delegate);
            expect(delegates.length).toEqual(expectedCount);
            return delegates;
        }

        async function waitForBalanceIncrease(
            ss58Address: string,
            balanceBefore: bigint,
            increase: bigint
        ): Promise<bigint> {
            const expected = balanceBefore + increase;
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const balance = await getBalance(api, ss58Address);
                if (balance === expected) {
                    return balance;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const balance = await getBalance(api, ss58Address);
            expect(balance).toEqual(expected);
            return balance;
        }

        it({
            id: "T01",
            title: "Can deploy bridge token smart contract",
            test: async () => {
                const contractFactory = new ethers.ContractFactory(
                    BRIDGE_TOKEN_CONTRACT_ABI,
                    BRIDGE_TOKEN_CONTRACT_BYTECODE,
                    ethWallet
                );
                const contract = await contractFactory.deploy("name", "symbol", ethWallet.address);
                await contract.waitForDeployment();

                expect(contract.target).toBeDefined();
                await expectDeployedContract(provider, contract.target.toString());
                await waitForFinalizedBlocks(api, 1);
            },
        });

        it({
            id: "T02",
            title: "Can deploy bridge token contract with gas limit",
            test: async () => {
                const contractFactory = new ethers.ContractFactory(
                    BRIDGE_TOKEN_CONTRACT_ABI,
                    BRIDGE_TOKEN_CONTRACT_BYTECODE,
                    ethWallet
                );
                const contract = await contractFactory.deploy("name", "symbol", ethWallet.address, {
                    gasLimit: 12_345_678,
                });
                await contract.waitForDeployment();

                expect(contract.target).toBeDefined();
                await expectDeployedContract(provider, contract.target.toString());
                await waitForFinalizedBlocks(api, 1);
            },
        });

        it({
            id: "T03",
            title: "Can add stake V2",
            test: async () => {
                await ensureSubnetReady();
                const hotkeySs58 = convertPublicKeyToSs58(hotkey.publicKey);
                const walletSs58 = convertH160ToSS58(ethWallet.address);
                const stakeBalance = tao(20);

                const stakeBefore = await getStake(api, hotkeySs58, walletSs58, netuid);
                const stakingPrecompile = new ethers.Contract(ISTAKING_V2_ADDRESS, IStakingV2ABI, ethWallet);
                const tx = await stakingPrecompile.addStake(hotkey.publicKey, stakeBalance.toString(), netuid);
                const receipt = await tx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 2);

                const stakeFromContract = BigInt(
                    await stakingPrecompile.getStake(
                        hotkey.publicKey,
                        convertH160ToPublicKey(ethWallet.address),
                        netuid
                    )
                );
                const stakeAfter = await getStake(api, hotkeySs58, walletSs58, netuid);

                expect(stakeFromContract).toBeGreaterThan(stakeBefore);
                expect(stakeAfter).toBeGreaterThan(stakeBefore);
            },
        });

        it({
            id: "T04",
            title: "Can deploy alpha pool smart contract",
            test: async () => {
                await ensureSubnetReady();
                const hotkeySs58 = convertPublicKeyToSs58(hotkey.publicKey);
                const walletSs58 = convertH160ToSS58(ethWallet.address);
                const stakingPrecompile = new ethers.Contract(ISTAKING_V2_ADDRESS, IStakingV2ABI, ethWallet);

                const stakeBeforeDeposit = await getStake(api, hotkeySs58, walletSs58, netuid);

                const contractFactory = new ethers.ContractFactory(
                    ALPHA_POOL_CONTRACT_ABI,
                    ALPHA_POOL_CONTRACT_BYTECODE,
                    ethWallet
                );
                const contract = await contractFactory.deploy(hotkey.publicKey);
                await contract.waitForDeployment();
                expect(contract.target).toBeDefined();

                const contractAddress = contract.target.toString();
                const contractPublicKey = convertH160ToPublicKey(contractAddress);
                await forceSetBalance(api, convertPublicKeyToSs58(contractPublicKey));
                await expectDeployedContract(provider, contractAddress);

                const contractForCall = new ethers.Contract(contractAddress, ALPHA_POOL_CONTRACT_ABI, ethWallet);
                const setContractColdkeyTx = await contractForCall.setContractColdkey(contractPublicKey);
                const setColdkeyReceipt = await setContractColdkeyTx.wait();
                expect(setColdkeyReceipt?.status).toEqual(1);

                expect(await contractForCall.contract_coldkey()).toEqual(u8aToHex(contractPublicKey));
                expect(await contractForCall.contract_hotkey()).toEqual(u8aToHex(hotkey.publicKey));

                const alphaInPool = await contractForCall.getContractStake(netuid);
                expect(alphaInPool).toEqual(BigInt(0));

                const depositAlphaTx = await contractForCall.depositAlpha(netuid, tao(10).toString(), hotkey.publicKey);
                const depositReceipt = await depositAlphaTx.wait();
                expect(depositReceipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 2);

                const stakeAfterDeposit = await getStake(api, hotkeySs58, walletSs58, netuid);
                expect(stakeAfterDeposit).toBeLessThan(stakeBeforeDeposit);

                const contractStake = await getStake(api, hotkeySs58, convertH160ToSS58(contractAddress), netuid);
                expect(contractStake).toBeGreaterThan(BigInt(0));

                const alphaBalanceOnContract = await contractForCall.alphaBalance(ethWallet.address, netuid);
                expect(tao(10) - alphaBalanceOnContract).toBeLessThan(BigInt(1000));

                const stakeFromContract = BigInt(
                    await stakingPrecompile.getStake(hotkey.publicKey, contractPublicKey, netuid)
                );
                expect(stakeFromContract).toEqual(await contractForCall.getContractStake(netuid));
            },
        });

        it({
            id: "T05",
            title: "Staker add and remove stake",
            test: async () => {
                await ensureSubnetReady();
                await ensureProxyWalletsReady();

                const deployedContract = await deployAndFundStakeWrap(stakeWallet);

                const stakeTx = await deployedContract.stake(hotkey.publicKey, netuid, tao(2));
                const stakeReceipt = await stakeTx.wait();
                expect(stakeReceipt?.status).toEqual(1);

                const removeTx = await deployedContract.removeStake(hotkey.publicKey, netuid, tao(1));
                const removeReceipt = await removeTx.wait();
                expect(removeReceipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 1);
            },
        });

        it({
            id: "T06",
            title: "Staker add stake limit",
            test: async () => {
                await ensureSubnetReady();
                await ensureProxyWalletsReady();

                const deployedContract = await deployAndFundStakeWrap(stakeWallet);

                const tx = await deployedContract.stakeLimit(hotkey.publicKey, netuid, tao(2000), tao(1000), true);
                const receipt = await tx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 1);
            },
        });

        it({
            id: "T07",
            title: "Call createPureProxy, then use proxy to call transfer",
            test: async () => {
                await ensureProxyWalletsReady();

                const proxies = await getProxies(api, convertH160ToSS58(proxyWallet1.address));
                const contract = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, proxyWallet1);

                const type = 0;
                const delay = 0;
                const index = 0;
                const tx = await contract.createPureProxy(type, delay, index);
                const response = await tx.wait();
                expect(response?.status).toEqual(1);

                const proxiesAfterAdd = await waitForPureProxyCount(
                    convertH160ToSS58(proxyWallet1.address),
                    proxies.length + 1
                );

                const proxy = proxiesAfterAdd[proxiesAfterAdd.length - 1];
                await forceSetBalance(api, proxy);

                const receiverSs58 = convertPublicKeyToSs58(pureProxyReceiver.publicKey);
                const balance = await getBalance(api, receiverSs58);
                const amount = 1_000_000_000;
                const callCode = await getTransferCallCode(api, pureProxyReceiver, amount);

                const tx2 = await contract.proxyCall(decodeAddress(proxy), [type], callCode);
                const response2 = await tx2.wait();
                expect(response2?.status).toEqual(1);

                await waitForBalanceIncrease(receiverSs58, balance, BigInt(amount));
            },
        });

        it({
            id: "T08",
            title: "Call createPureProxy, add multiple proxies",
            test: async () => {
                await ensureProxyWalletsReady();

                const contract = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, proxyWallet1);
                const type = 0;
                const delay = 0;
                const index = 0;
                const delegateSs58 = convertH160ToSS58(proxyWallet1.address);
                const proxies = await getProxies(api, delegateSs58);
                const length = proxies.length;

                for (let i = 0; i < 5; i++) {
                    const tx = await contract.createPureProxy(type, delay, index);
                    await tx.wait();
                    await waitForPureProxyCount(delegateSs58, length + i + 1);
                }
            },
        });

        it({
            id: "T09",
            title: "Call createPureProxy, edge cases, call via wrong proxy",
            test: async () => {
                await ensureProxyWalletsReady();

                const contract = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, proxyWallet2);
                const amount = 1_000_000_000;
                const wrongReceiver = generateKeyringPair("sr25519");
                const callCode = await getTransferCallCode(api, wrongReceiver, amount);
                const type = 0;

                await expect(
                    contract.proxyCall(wrongReceiver.publicKey, [type], callCode).then((proxyTx) => proxyTx.wait())
                ).rejects.toBeDefined();
            },
        });

        it({
            id: "T10",
            title: "Call createProxy, then use proxy to call transfer",
            test: async () => {
                await ensureProxyWalletsReady();

                const proxies = await api.query.Proxy.Proxies.getValue(convertH160ToSS58(proxyWallet2.address));
                const contract = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, proxyWallet2);

                const proxiesFromContract = await contract.getProxies(convertH160ToPublicKey(proxyWallet2.address));
                expect(proxiesFromContract.length).toEqual(proxies[0].length);

                const type = 0;
                const delay = 0;

                const tx = await contract.addProxy(convertH160ToPublicKey(proxyWallet3.address), type, delay);
                await tx.wait();

                const proxiesList = await waitForProxyDelegates(
                    convertH160ToSS58(proxyWallet2.address),
                    proxies[0].length + 1
                );

                const proxiesFromContractAfterAdd = await contract.getProxies(
                    convertH160ToPublicKey(proxyWallet2.address)
                );
                expect(proxiesFromContractAfterAdd.length).toEqual(proxiesList.length);

                for (let index = 0; index < proxiesFromContractAfterAdd.length; index++) {
                    const proxyInfo = proxiesFromContractAfterAdd[index];
                    const proxySs58 = convertPublicKeyToSs58(proxyInfo[0]);
                    expect(proxiesList.includes(proxySs58)).toBe(true);
                    if (index === proxiesFromContractAfterAdd.length - 1) {
                        expect(Number(proxyInfo[1])).toEqual(type);
                        expect(Number(proxyInfo[2])).toEqual(delay);
                    }
                }

                expect(proxiesList.length).toEqual(proxies[0].length + 1);
                const proxy = proxiesList[proxiesList.length - 1];
                expect(proxy).toEqual(convertH160ToSS58(proxyWallet3.address));

                const receiverSs58 = convertPublicKeyToSs58(delegateProxyReceiver.publicKey);
                const balance = await getBalance(api, receiverSs58);
                const amount = 1_000_000_000;

                const contract2 = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, proxyWallet3);
                const callCode = await getTransferCallCode(api, delegateProxyReceiver, amount);
                const tx2 = await contract2.proxyCall(convertH160ToPublicKey(proxyWallet2.address), [type], callCode);
                await tx2.wait();

                await waitForBalanceIncrease(receiverSs58, balance, BigInt(amount));
            },
        });

        it({
            id: "T11",
            title: "Call addProxy many times, then check getProxies is correct",
            test: async () => {
                await ensureProxyWalletsReady();

                const proxies = await api.query.Proxy.Proxies.getValue(convertH160ToSS58(proxyWallet4.address));
                const contract = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, proxyWallet4);
                expect(proxies[0].length).toEqual(0);

                const proxiesFromContract = await contract.getProxies(convertH160ToPublicKey(proxyWallet4.address));
                expect(proxiesFromContract.length).toEqual(proxies[0].length);

                const type = 1;
                const delay = 2;

                for (let i = 0; i < 5; i++) {
                    const delegateWallet = createEthersWallet(provider);
                    const addTx = await contract.addProxy(convertH160ToPublicKey(delegateWallet.address), type, delay);
                    await addTx.wait();
                }

                const proxiesList = await waitForProxyDelegates(convertH160ToSS58(proxyWallet4.address), 5);

                const proxiesFromContractAfterAdd = await contract.getProxies(
                    convertH160ToPublicKey(proxyWallet4.address)
                );
                expect(proxiesFromContractAfterAdd.length).toEqual(proxiesList.length);

                for (let index = 0; index < proxiesFromContractAfterAdd.length; index++) {
                    const proxyInfo = proxiesFromContractAfterAdd[index];
                    const proxySs58 = convertPublicKeyToSs58(proxyInfo[0]);
                    expect(proxiesList.includes(proxySs58)).toBe(true);
                    expect(Number(proxyInfo[1])).toEqual(type);
                    expect(Number(proxyInfo[2])).toEqual(delay);
                }
            },
        });
    },
});
