import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import { subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import { ethers } from "ethers";
import { Binary, type TypedApi } from "polkadot-api";
import {
    addNewSubnetwork,
    convertH160ToPublicKey,
    convertH160ToSS58,
    convertPublicKeyToSs58,
    createEthersWallet,
    disableWhiteListCheck,
    forceSetBalance,
    generateKeyringPair,
    getBalance,
    getStake,
    IPROXY_ADDRESS,
    IProxyABI,
    PRECOMPILE_WRAPPER_ABI,
    PRECOMPILE_WRAPPER_BYTECODE,
    raoToEth,
    startCall,
    sudoSetLockReductionInterval,
    tao,
    waitForFinalizedBlocks,
    waitForTransactionWithRetry,
} from "../../utils";

describeSuite({
    id: "direct-call-precompile",
    title: "PrecompileWrapper direct call tests",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let provider: ethers.JsonRpcProvider;
        let ethWallet: ethers.Wallet;
        let ethWallet2: ethers.Wallet;
        let hotkey: KeyringPair;
        let coldkey: KeyringPair;
        let wrapperContract: ethers.Contract;
        let wrapperAddress: string;
        let netuid: number;
        let subnetReady = false;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            provider = context.ethers("EVM").provider as ethers.JsonRpcProvider;

            ethWallet = createEthersWallet(provider);
            ethWallet2 = createEthersWallet(provider);

            await forceSetBalance(api, convertH160ToSS58(ethWallet.address));
            await forceSetBalance(api, convertH160ToSS58(ethWallet2.address));
            await disableWhiteListCheck(api, true);

            hotkey = generateKeyringPair("sr25519");
            coldkey = generateKeyringPair("sr25519");

            await sudoSetLockReductionInterval(api, 1);
            await forceSetBalance(api, convertPublicKeyToSs58(hotkey.publicKey));
            await forceSetBalance(api, convertPublicKeyToSs58(coldkey.publicKey));

            netuid = await addNewSubnetwork(api, hotkey, coldkey);
            await startCall(api, netuid, coldkey);

            const factory = new ethers.ContractFactory(PRECOMPILE_WRAPPER_ABI, PRECOMPILE_WRAPPER_BYTECODE, ethWallet);
            const deployed = await factory.deploy();
            await deployed.waitForDeployment();
            wrapperAddress = await deployed.getAddress();
            await forceSetBalance(api, convertH160ToSS58(wrapperAddress));
            await waitForFinalizedBlocks(api, 1);

            wrapperContract = new ethers.Contract(wrapperAddress, PRECOMPILE_WRAPPER_ABI, ethWallet);
            subnetReady = true;
        }, 600000);

        async function ensureSubnetAndWrapperReady(): Promise<void> {
            expect(subnetReady).toBe(true);
        }

        async function getCrowdloanEndBlock(): Promise<number> {
            const currentBlock = await api.query.System.Number.getValue();
            const minDuration = await api.constants.Crowdloan.MinimumBlockDuration();
            return currentBlock + minDuration + 100;
        }

        async function waitForCrowdloanId(expected: number): Promise<void> {
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const nextId = await api.query.Crowdloan.NextCrowdloanId.getValue();
                if (nextId === expected) {
                    return;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const nextId = await api.query.Crowdloan.NextCrowdloanId.getValue();
            expect(nextId).toEqual(expected);
        }

        async function waitForBalanceAtLeast(ss58Address: string, minimum: bigint): Promise<bigint> {
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const balance = await getBalance(api, ss58Address);
                if (balance >= minimum) {
                    return balance;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const balance = await getBalance(api, ss58Address);
            expect(balance).toBeGreaterThanOrEqual(minimum);
            return balance;
        }

        async function waitForTotalNetworks(expected: number): Promise<void> {
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const total = await api.query.SubtensorModule.TotalNetworks.getValue();
                if (total === expected) {
                    return;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const total = await api.query.SubtensorModule.TotalNetworks.getValue();
            expect(total).toEqual(expected);
        }

        async function waitForProxyCount(realSs58: string, expected: number): Promise<void> {
            const deadline = Date.now() + 120_000;
            while (Date.now() < deadline) {
                const proxies = await api.query.Proxy.Proxies.getValue(realSs58);
                if (proxies[0].length === expected) {
                    return;
                }
                await waitForFinalizedBlocks(api, 1);
            }
            const proxies = await api.query.Proxy.Proxies.getValue(realSs58);
            expect(proxies[0].length).toEqual(expected);
        }

        it({
            id: "T01",
            title: "Should transfer balance via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const recipient = generateKeyringPair("sr25519");
                const transferAmount = raoToEth(tao(1));

                const transferTx = await wrapperContract.transfer(recipient.publicKey, {
                    value: transferAmount,
                });
                const receipt = await transferTx.wait();
                expect(receipt?.status).toEqual(1);

                await waitForBalanceAtLeast(convertPublicKeyToSs58(recipient.publicKey), tao(1));
            },
        });

        it({
            id: "T02",
            title: "Should get UID count via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();
                const uidCount = await wrapperContract.getUidCount(netuid);
                expect(uidCount).toBeDefined();
            },
        });

        it({
            id: "T03",
            title: "Should get serving rate limit via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();
                const rateLimit = await wrapperContract.getServingRateLimit(netuid);
                expect(rateLimit).toBeDefined();
            },
        });

        it({
            id: "T04",
            title: "Should get network registered block via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const onchainValue = await api.query.SubtensorModule.NetworkRegisteredAt.getValue(netuid);
                const valueViaWrapper = Number(await wrapperContract.getNetworkRegistrationBlock(netuid));

                expect(valueViaWrapper).toBeGreaterThan(0);
                expect(valueViaWrapper).toEqual(Number(onchainValue));
            },
        });

        it({
            id: "T05",
            title: "Should register network with details via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const newHotkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, convertPublicKeyToSs58(newHotkey.publicKey));

                const totalNetworksBefore = await api.query.SubtensorModule.TotalNetworks.getValue();
                const registerTx = await wrapperContract.registerNetworkWithDetails(
                    newHotkey.publicKey,
                    "Test Subnet",
                    "https://github.com/test/repo",
                    "test@example.com",
                    "https://test.example.com",
                    "test#1234",
                    "Test description",
                    "Additional info",
                    { value: raoToEth(tao(100)) }
                );
                const receipt = await registerTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForTotalNetworks(totalNetworksBefore + 1);
            },
        });

        it({
            id: "T06",
            title: "Should register neuron via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const newHotkey = generateKeyringPair("sr25519");
                const newColdkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, convertPublicKeyToSs58(newHotkey.publicKey));
                await forceSetBalance(api, convertPublicKeyToSs58(newColdkey.publicKey));

                const burnAmount = tao(100);
                const registerTx = await wrapperContract.burnedRegister(netuid, newHotkey.publicKey, {
                    value: raoToEth(burnAmount),
                });
                const receipt = await registerTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 2);

                const uid = await api.query.SubtensorModule.Uids.getValue(
                    netuid,
                    convertPublicKeyToSs58(newHotkey.publicKey)
                );
                expect(uid).toBeDefined();
            },
        });

        it({
            id: "T07",
            title: "Should get total coldkey stake via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();
                const stake = await wrapperContract.getTotalColdkeyStake(coldkey.publicKey);
                expect(stake).toBeDefined();
            },
        });

        it({
            id: "T08",
            title: "Should get total hotkey stake via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();
                const stake = await wrapperContract.getTotalHotkeyStake(hotkey.publicKey);
                expect(stake).toBeDefined();
            },
        });

        it({
            id: "T09",
            title: "Should add stake via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const stakeAmount = tao(2);
                const wrapperSs58 = convertH160ToSS58(wrapperAddress);
                const hotkeySs58 = convertPublicKeyToSs58(hotkey.publicKey);
                const stakeBefore = await getStake(api, hotkeySs58, wrapperSs58, netuid);

                const addStakeTx = await wrapperContract.addStake(hotkey.publicKey, stakeAmount, netuid, {
                    value: raoToEth(stakeAmount),
                });
                const receipt = await addStakeTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 2);

                const stakeAfter = await getStake(api, hotkeySs58, wrapperSs58, netuid);
                expect(stakeAfter).toBeGreaterThan(stakeBefore);
            },
        });

        it({
            id: "T10",
            title: "Should remove stake via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const removeAmount = tao(1);
                const wrapperSs58 = convertH160ToSS58(wrapperAddress);
                const hotkeySs58 = convertPublicKeyToSs58(hotkey.publicKey);
                const stakeBefore = await getStake(api, hotkeySs58, wrapperSs58, netuid);

                const removeStakeTx = await wrapperContract.removeStake(
                    hotkey.publicKey,
                    removeAmount.toString(),
                    netuid
                );
                const receipt = await removeStakeTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 2);

                const stakeAfter = await getStake(api, hotkeySs58, wrapperSs58, netuid);
                expect(stakeAfter).toBeLessThan(stakeBefore);
            },
        });

        it({
            id: "T11",
            title: "Should lookup UID via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();
                const lookup = await wrapperContract.uidLookup(netuid, ethWallet.address, 10);
                expect(Array.isArray(lookup)).toBe(true);
            },
        });

        it({
            id: "T12",
            title: "Should get alpha price via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();
                const price = await wrapperContract.getAlphaPrice(netuid);
                expect(price).toBeDefined();
            },
        });

        it({
            id: "T13",
            title: "Should get crowdloan via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const nextId = await api.query.Crowdloan.NextCrowdloanId.getValue();
                const end = await getCrowdloanEndBlock();
                const deposit = BigInt(15_000_000_000);
                const minContribution = BigInt(1_000_000_000);
                const cap = BigInt(100_000_000_000);

                const tx = api.tx.Crowdloan.create({
                    deposit,
                    min_contribution: minContribution,
                    cap,
                    end,
                    target_address: undefined,
                    call: api.tx.System.remark({ remark: Binary.fromText("test") }).decodedCall,
                });
                await waitForTransactionWithRetry(api, tx, coldkey, "crowdloan_create", 5);
                await waitForFinalizedBlocks(api, 1);

                const crowdloan = await wrapperContract.getCrowdloan(nextId);
                expect(crowdloan).toBeDefined();
            },
        });

        it({
            id: "T14",
            title: "Should get contribution via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const nextId = await api.query.Crowdloan.NextCrowdloanId.getValue();
                expect(nextId).toBeGreaterThan(0);
                const contribution = await wrapperContract.getContribution(nextId - 1, coldkey.publicKey);
                expect(contribution).toBeDefined();
            },
        });

        it({
            id: "T15",
            title: "Should create crowdloan via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const deposit = BigInt(20_000_000_000);
                const minContribution = BigInt(2_000_000_000);
                const cap = BigInt(200_000_000_000);
                const end = await getCrowdloanEndBlock();
                const nextIdBefore = await api.query.Crowdloan.NextCrowdloanId.getValue();

                const createTx = await wrapperContract.createCrowdloan(
                    deposit,
                    minContribution,
                    cap,
                    end,
                    ethWallet2.address,
                    { value: raoToEth(deposit) }
                );
                const receipt = await createTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForCrowdloanId(nextIdBefore + 1);
            },
        });

        it({
            id: "T16",
            title: "Should get contributor share via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const crowdloanDeposit = BigInt(100_000_000_000);
                const networkLastLockCost = await api.query.SubtensorModule.NetworkLastLockCost.getValue();
                const crowdloanCap = networkLastLockCost * BigInt(2);
                const crowdloanEnd = await getCrowdloanEndBlock();
                const leasingEmissionsShare = 15;
                const leasingEndBlock = crowdloanEnd + 200;

                const tx = api.tx.Crowdloan.create({
                    deposit: crowdloanDeposit,
                    min_contribution: BigInt(1_000_000_000),
                    cap: crowdloanCap,
                    end: crowdloanEnd,
                    target_address: undefined,
                    call: api.tx.SubtensorModule.register_leased_network({
                        emissions_share: leasingEmissionsShare,
                        end_block: leasingEndBlock,
                    }).decodedCall,
                });
                await waitForTransactionWithRetry(api, tx, coldkey, "lease_crowdloan_create", 5);
                await waitForFinalizedBlocks(api, 1);

                const nextLeaseId = await api.query.SubtensorModule.NextSubnetLeaseId.getValue();
                const share = await wrapperContract.getContributorShare(nextLeaseId, coldkey.publicKey);
                expect(share).toBeDefined();
            },
        });

        it({
            id: "T17",
            title: "Should create lease crowdloan via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const crowdloanDeposit = BigInt(100_000_000_000);
                const crowdloanMinContribution = BigInt(1_000_000_000);
                const networkLastLockCost = await api.query.SubtensorModule.NetworkLastLockCost.getValue();
                const crowdloanCap = networkLastLockCost * BigInt(2);
                const crowdloanEnd = await getCrowdloanEndBlock();
                const leasingEmissionsShare = 15;
                const leasingEndBlock = crowdloanEnd + 200;
                const nextCrowdloanIdBefore = await api.query.Crowdloan.NextCrowdloanId.getValue();

                const createTx = await wrapperContract.createLeaseCrowdloan(
                    crowdloanDeposit,
                    crowdloanMinContribution,
                    crowdloanCap,
                    crowdloanEnd,
                    leasingEmissionsShare,
                    true,
                    leasingEndBlock,
                    { value: raoToEth(crowdloanDeposit) }
                );
                const receipt = await createTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForCrowdloanId(nextCrowdloanIdBefore + 1);
            },
        });

        it({
            id: "T18",
            title: "Should get proxies via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const accountKey = convertH160ToPublicKey(ethWallet.address);
                const proxies = await wrapperContract.getProxies(accountKey);
                expect(proxies).toBeDefined();
                expect(Array.isArray(proxies)).toBe(true);
            },
        });

        it({
            id: "T19",
            title: "Should add proxy via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const delegate = generateKeyringPair("sr25519");
                await forceSetBalance(api, convertPublicKeyToSs58(delegate.publicKey));

                const wrapperSs58 = convertH160ToSS58(wrapperAddress);
                const proxiesBefore = await api.query.Proxy.Proxies.getValue(wrapperSs58);

                const addProxyTx = await wrapperContract.addProxy(delegate.publicKey, 0, 0);
                const receipt = await addProxyTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForProxyCount(wrapperSs58, proxiesBefore[0].length + 1);
            },
        });

        it({
            id: "T20",
            title: "Should proxy call via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const proxyType = 0;
                const delay = 0;
                const proxyContract = new ethers.Contract(IPROXY_ADDRESS, IProxyABI, ethWallet);
                const addProxyTx = await proxyContract.addProxy(
                    convertH160ToPublicKey(wrapperAddress),
                    proxyType,
                    delay
                );
                const receipt = await addProxyTx.wait();
                expect(receipt?.status).toEqual(1);
                await waitForFinalizedBlocks(api, 1);

                const remarkCall = api.tx.System.remark({ remark: Binary.fromText("") });
                const callData = await remarkCall.getEncodedData();
                const data = callData.asBytes();

                const proxyCallTx = await wrapperContract.proxyCall(
                    convertH160ToPublicKey(ethWallet.address),
                    [proxyType],
                    [...data]
                );
                const proxyReceipt = await proxyCallTx.wait();
                expect(proxyReceipt?.status).toEqual(1);
            },
        });

        it({
            id: "T21",
            title: "Should map address via wrapper",
            test: async () => {
                await ensureSubnetAndWrapperReady();

                const mapped = await wrapperContract.addressMapping(ethWallet.address);
                expect(mapped).toBeDefined();
                expect(mapped).not.toEqual("0x0000000000000000000000000000000000000000000000000000000000000000");
            },
        });
    },
});
