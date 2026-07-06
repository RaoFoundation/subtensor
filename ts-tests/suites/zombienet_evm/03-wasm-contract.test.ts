import { beforeAll, beforeEach, describeSuite, expect } from "@moonwall/cli";
import { contracts, MultiAddress, subtensor } from "@polkadot-api/descriptors";
import { getInkClient, InkClient } from "@polkadot-api/ink-contracts";
import type { KeyringPair } from "@polkadot/keyring/types";
import fs from "node:fs";
import { Binary, type TypedApi } from "polkadot-api";
import {
    addNewSubnetwork,
    BITTENSOR_WASM_PATH,
    burnedRegister,
    convertPublicKeyToSs58,
    forceSetBalance,
    generateKeyringPair,
    getBalance,
    instantiateWasmContract,
    sendWasmContractExtrinsic,
    sendWasmContractExtrinsicAllowFailure,
    setTargetRegistrationsPerInterval,
    startCall,
    sudoSetAdminFreezeWindow,
    sudoSetLockReductionInterval,
    tao,
    waitForFinalizedBlocks,
    waitForTransactionWithRetry,
} from "../../utils";

const bittensorBytecode = fs.readFileSync(BITTENSOR_WASM_PATH);

async function fundAccount(
    api: TypedApi<typeof subtensor>,
    faucet: KeyringPair,
    address: string,
    amount: bigint = tao(10_000)
): Promise<void> {
    const tx = api.tx.Balances.transfer_keep_alive({
        dest: MultiAddress.Id(address),
        value: amount,
    });
    await waitForTransactionWithRetry(api, tx, faucet, "fund_account", 5);
}

describeSuite({
    id: "wasm-contract",
    title: "Wasm ink contract tests",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let faucet: KeyringPair;
        let hotkey: KeyringPair;
        let coldkey: KeyringPair;
        let hotkey2: KeyringPair;
        let coldkey2: KeyringPair;
        let netuid = 0;
        let contractAddress = "";
        let inkClient: InkClient<typeof contracts.bittensor>;

        async function addStakeViaContract(addStakeToContract: boolean) {
            if (contractAddress === "") {
                return;
            }

            const amount = tao(100);
            let message;
            let dest;
            if (addStakeToContract) {
                message = inkClient.message("add_stake");
                dest = contractAddress;
            } else {
                message = inkClient.message("caller_add_stake");
                dest = convertPublicKeyToSs58(coldkey.publicKey);
            }

            const data = message.encode({
                hotkey: Binary.fromBytes(hotkey.publicKey),
                netuid: netuid,
                amount: amount,
            });
            await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

            const stake = (
                await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    dest,
                    netuid
                )
            )?.stake;

            expect(stake).toBeDefined();
            expect(stake > BigInt(0)).toBeTruthy();
        }

        async function getContractStake(): Promise<bigint> {
            const stake = (
                await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    contractAddress,
                    netuid
                )
            )?.stake;

            expect(stake).toBeDefined();
            return stake as bigint;
        }

        async function getContractStakeOnRoot(): Promise<bigint> {
            const stake = (
                await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    contractAddress,
                    0
                )
            )?.stake;

            expect(stake).toBeDefined();
            return stake as bigint;
        }

        async function initSecondColdAndHotkey() {
            hotkey2 = generateKeyringPair("sr25519");
            coldkey2 = generateKeyringPair("sr25519");
            await fundAccount(api, faucet, convertPublicKeyToSs58(coldkey2.publicKey));
            await fundAccount(api, faucet, convertPublicKeyToSs58(hotkey2.publicKey));
            await burnedRegister(api, netuid, convertPublicKeyToSs58(hotkey2.publicKey), coldkey2);
        }

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            await waitForFinalizedBlocks(api, 2);
            await sudoSetLockReductionInterval(api, 1);
            await sudoSetAdminFreezeWindow(api, 0);

            inkClient = getInkClient(contracts.bittensor);
            faucet = generateKeyringPair("sr25519");
            await forceSetBalance(api, convertPublicKeyToSs58(faucet.publicKey), tao(1e9));

            hotkey = generateKeyringPair("sr25519");
            coldkey = generateKeyringPair("sr25519");
            await fundAccount(api, faucet, convertPublicKeyToSs58(coldkey.publicKey));
            await fundAccount(api, faucet, convertPublicKeyToSs58(hotkey.publicKey));

            netuid = await addNewSubnetwork(api, hotkey, coldkey);
            await startCall(api, netuid, coldkey);
            await addNewSubnetwork(api, hotkey, coldkey);
            await startCall(api, netuid + 1, coldkey);
            await setTargetRegistrationsPerInterval(api, netuid);
            await waitForFinalizedBlocks(api, 1);
        }, 900000);

        beforeEach(async () => {
            hotkey = generateKeyringPair("sr25519");
            coldkey = generateKeyringPair("sr25519");
            await fundAccount(api, faucet, convertPublicKeyToSs58(coldkey.publicKey));
            await fundAccount(api, faucet, convertPublicKeyToSs58(hotkey.publicKey));
            await burnedRegister(api, netuid, convertPublicKeyToSs58(hotkey.publicKey), coldkey);
        }, 300000);

        it({
            id: "T01",
            title: "Can instantiate contract",
            test: async () => {
                const constructor = inkClient.constructor("new");
                const data = constructor.encode();
                contractAddress = await instantiateWasmContract(api, coldkey, bittensorBytecode, data);

                const transfer = api.tx.Balances.transfer_keep_alive({
                    dest: MultiAddress.Id(contractAddress),
                    value: tao(2000),
                });
                await waitForTransactionWithRetry(api, transfer, coldkey, "transfer_to_contract", 5);
                await waitForFinalizedBlocks(api, 1);
            },
        });

        it({
            id: "T02",
            title: "Can query stake info from contract",
            test: async () => {
                const queryMessage = inkClient.message("get_stake_info_for_hotkey_coldkey_netuid");

                const data = queryMessage.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    coldkey: Binary.fromBytes(coldkey.publicKey),
                    netuid: netuid,
                });

                const response = await api.apis.ContractsApi.call(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    contractAddress,
                    BigInt(0),
                    undefined,
                    undefined,
                    Binary.fromBytes(data.asBytes())
                );

                expect(response.result.success).toBeTruthy();
                const result = queryMessage.decode(response.result.value).value.value;

                if (
                    typeof result === "object" &&
                    "hotkey" in result &&
                    "coldkey" in result &&
                    "netuid" in result &&
                    "stake" in result &&
                    "locked" in result &&
                    "emission" in result &&
                    "tao_emission" in result &&
                    "drain" in result &&
                    "is_registered" in result
                ) {
                    expect(result.hotkey).toEqual(convertPublicKeyToSs58(hotkey.publicKey));
                    expect(result.coldkey).toEqual(convertPublicKeyToSs58(coldkey.publicKey));
                    expect(result.netuid).toEqual(netuid);
                    expect(result.is_registered).toEqual(true);
                } else {
                    throw new Error("result is not an object");
                }
            },
        });

        it({
            id: "T03",
            title: "Can add stake to contract",
            test: async () => {
                await addStakeViaContract(true);
            },
        });

        it({
            id: "T04",
            title: "Can remove stake to contract",
            test: async () => {
                await addStakeViaContract(true);
                const stake = await getContractStake();

                let amount = stake / BigInt(2);
                const message = inkClient.message("remove_stake");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid: netuid,
                    amount: amount,
                });

                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfterAddStake = await getContractStake();

                expect(stakeAfterAddStake < stake).toBeTruthy();
            },
        });

        it({
            id: "T05",
            title: "Can unstake all from contract",
            test: async () => {
                await addStakeViaContract(true);
                // Get stake before unstake_all
                const stakeBefore = await getContractStake();

                expect(stakeBefore > BigInt(0)).toBeTruthy();

                // Call unstake_all
                const unstakeMessage = inkClient.message("unstake_all");
                const unstakeData = unstakeMessage.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, unstakeData);

                // Verify stake is now zero
                const stakeAfter = await getContractStake();

                expect(stakeAfter).toEqual(BigInt(0));
            },
        });

        it({
            id: "T06",
            title: "Can unstake all alpha from contract",
            test: async () => {
                await addStakeViaContract(true);
                // Get stake before unstake_all_alpha
                const stakeBefore = await getContractStake();

                expect(stakeBefore > BigInt(0)).toBeTruthy();

                // Call unstake_all_alpha
                const message = inkClient.message("unstake_all_alpha");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                // Verify stake is now zero
                const stakeAfter = await getContractStake();

                expect(stakeAfter).toEqual(BigInt(0));
            },
        });

        it({
            id: "T07",
            title: "Can move stake between hotkeys",
            test: async () => {
                await addStakeViaContract(true);
                await initSecondColdAndHotkey();
                // Get initial stakes
                const originStakeBefore = await getContractStake();

                const destStakeBefore =
                    (
                        await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                            convertPublicKeyToSs58(hotkey2.publicKey),
                            contractAddress,
                            netuid
                        )
                    )?.stake || BigInt(0);

                expect(originStakeBefore > BigInt(0)).toBeTruthy();

                // Move stake
                const moveAmount = originStakeBefore / BigInt(2);
                const message = inkClient.message("move_stake");
                const data = message.encode({
                    origin_hotkey: Binary.fromBytes(hotkey.publicKey),
                    destination_hotkey: Binary.fromBytes(hotkey2.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid,
                    amount: moveAmount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                // Verify stakes changed
                const originStakeAfter = await getContractStake();

                const destStakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey2.publicKey),
                        contractAddress,
                        netuid
                    )
                )?.stake;

                expect(destStakeAfter).toBeDefined();
                expect(originStakeAfter < originStakeBefore).toBeTruthy();
                expect(destStakeAfter > destStakeBefore).toBeTruthy();
            },
        });

        it({
            id: "T08",
            title: "Can transfer stake between coldkeys",
            test: async () => {
                await addStakeViaContract(true);
                await initSecondColdAndHotkey();
                // Get initial stake
                const stakeBeforeOrigin = await getContractStake();

                const stakeBeforeDest = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey2.publicKey),
                        netuid
                    )
                )?.stake;

                expect(stakeBeforeOrigin > BigInt(0)).toBeTruthy();
                expect(stakeBeforeDest).toBeDefined();

                // Transfer stake
                const transferAmount = stakeBeforeOrigin / BigInt(2);
                const message = inkClient.message("transfer_stake");
                const data = message.encode({
                    destination_coldkey: Binary.fromBytes(coldkey2.publicKey),
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid,
                    amount: transferAmount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                // Verify stake transferred
                const stakeAfterOrigin = await getContractStake();

                const stakeAfterDest = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey2.publicKey),
                        netuid
                    )
                )?.stake;

                expect(stakeAfterDest).toBeDefined();
                expect(stakeAfterOrigin < stakeBeforeOrigin).toBeTruthy();
                expect(stakeAfterDest > stakeBeforeDest!).toBeTruthy();
            },
        });

        it({
            id: "T09",
            title: "Can swap stake between networks",
            test: async () => {
                await addStakeViaContract(true);
                // Get initial stakes
                const stakeBefore = await getContractStake();

                const stakeBefore2 =
                    (
                        await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                            convertPublicKeyToSs58(hotkey.publicKey),
                            contractAddress,
                            netuid + 1
                        )
                    )?.stake || BigInt(0);

                expect(stakeBefore > BigInt(0)).toBeTruthy();

                // Swap stake
                const swapAmount = stakeBefore / BigInt(2);
                const message = inkClient.message("swap_stake");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid + 1,
                    amount: swapAmount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                // Verify stakes swapped
                const stakeAfter = await getContractStake();

                const stakeAfter2 = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        contractAddress,
                        netuid + 1
                    )
                )?.stake;

                expect(stakeAfter2).toBeDefined();
                expect(stakeAfter < stakeBefore).toBeTruthy();
                expect(stakeAfter2 > stakeBefore2).toBeTruthy();
            },
        });

        it({
            id: "T10",
            title: "Can add stake with limit",
            test: async () => {
                const stakeBefore = await getContractStake();

                const message = inkClient.message("add_stake_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid: netuid,
                    amount: tao(200),
                    limit_price: tao(100),
                    allow_partial: false,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                // Verify stake was added
                const stakeAfter = await getContractStake();

                expect(stakeAfter > stakeBefore).toBeTruthy();
            },
        });

        it({
            id: "T11",
            title: "Can remove stake with limit",
            test: async () => {
                await addStakeViaContract(true);
                const stakeBefore = await getContractStake();

                expect(stakeBefore > BigInt(0)).toBeTruthy();

                const message = inkClient.message("remove_stake_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid: netuid,
                    amount: stakeBefore / BigInt(2),
                    limit_price: tao(1),
                    allow_partial: false,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();

                expect(stakeAfter < stakeBefore).toBeTruthy();
            },
        });

        it({
            id: "T12",
            title: "Can swap stake with limit",
            test: async () => {
                await addStakeViaContract(true);

                const stakeBefore = await getContractStake();

                const stakeBefore2 = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        contractAddress,
                        netuid + 1
                    )
                )?.stake;

                expect(stakeBefore > BigInt(0)).toBeTruthy();
                expect(stakeBefore2).toBeDefined();

                const message = inkClient.message("swap_stake_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid + 1,
                    amount: stakeBefore / BigInt(2),
                    limit_price: tao(1),
                    allow_partial: false,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();

                const stakeAfter2 = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        contractAddress,
                        netuid + 1
                    )
                )?.stake;

                expect(stakeAfter2).toBeDefined();
                expect(stakeAfter < stakeBefore).toBeTruthy();
                expect(stakeAfter2 > stakeBefore2).toBeTruthy();
            },
        });

        it({
            id: "T13",
            title: "Can remove stake full limit",
            test: async () => {
                await addStakeViaContract(true);
                const stakeBefore = await getContractStake();

                expect(stakeBefore > BigInt(0)).toBeTruthy();

                const message = inkClient.message("remove_stake_full_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid: netuid,
                    limit_price: BigInt(0),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();

                expect(stakeAfter < stakeBefore).toBeTruthy();
            },
        });

        it({
            id: "T14",
            title: "Can set coldkey auto stake hotkey",
            test: async () => {
                const message = inkClient.message("set_coldkey_auto_stake_hotkey");
                const data = message.encode({
                    netuid: netuid,
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                let autoStakeHotkey = await api.query.SubtensorModule.AutoStakeDestination.getValue(
                    contractAddress,
                    netuid
                );

                expect(autoStakeHotkey).toBeDefined();
                expect(autoStakeHotkey).toEqual(convertPublicKeyToSs58(hotkey.publicKey));
            },
        });

        it({
            id: "T15",
            title: "Can add and remove proxy",
            test: async () => {
                const message = inkClient.message("add_proxy");
                const data = message.encode({
                    delegate: Binary.fromBytes(hotkey.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                let proxies = await api.query.Proxy.Proxies.getValue(contractAddress);
                expect(proxies).toBeDefined();
                expect(proxies.length > 0 && proxies[0].length > 0).toBeTruthy();
                expect(proxies[0][0].delegate).toEqual(convertPublicKeyToSs58(hotkey.publicKey));

                const removeMessage = inkClient.message("remove_proxy");
                const removeData = removeMessage.encode({
                    delegate: Binary.fromBytes(hotkey.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, removeData);

                let proxiesAfterRemove = await api.query.Proxy.Proxies.getValue(contractAddress);
                expect(proxiesAfterRemove).toBeDefined();
                expect(proxiesAfterRemove[0].length).toEqual(0);
            },
        });

        it({
            id: "T16",
            title: "Can get alpha price",
            test: async () => {
                const message = inkClient.message("get_alpha_price");
                const data = message.encode({
                    netuid: netuid,
                });

                const response = await api.apis.ContractsApi.call(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    contractAddress,
                    BigInt(0),
                    undefined,
                    undefined,
                    Binary.fromBytes(data.asBytes())
                );

                expect(response.result.success).toBeTruthy();
                const result = message.decode(response.result.value).value.value;

                expect(result).toBeDefined();
            },
        });

        it({
            id: "T17",
            title: "Can recycle alpha from contract stake",
            test: async () => {
                await addStakeViaContract(true);
                await waitForFinalizedBlocks(api, 2);
                const stakeBefore = await getContractStake();
                const alphaOutBefore = await api.query.SubtensorModule.SubnetAlphaOut.getValue(netuid);

                const message = inkClient.message("recycle_alpha");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount: stakeBefore / BigInt(2),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();
                const alphaOutAfter = await api.query.SubtensorModule.SubnetAlphaOut.getValue(netuid);

                expect(stakeAfter < stakeBefore).toBeTruthy();
                expect(alphaOutAfter < alphaOutBefore).toBeTruthy();
            },
        });

        it({
            id: "T18",
            title: "Can burn alpha from contract stake",
            test: async () => {
                await addStakeViaContract(true);
                await waitForFinalizedBlocks(api, 2);
                const stakeBefore = await getContractStake();
                const alphaBurnedBefore = await api.query.AlphaAssets.AlphaBurned.getValue(netuid);

                const message = inkClient.message("burn_alpha");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount: stakeBefore / BigInt(2),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();
                const alphaBurnedAfter = await api.query.AlphaAssets.AlphaBurned.getValue(netuid);

                expect(stakeAfter < stakeBefore).toBeTruthy();
                expect(alphaBurnedBefore < alphaBurnedAfter).toBeTruthy();
            },
        });

        it({
            id: "T19",
            title: "Can add stake and recycle resulting alpha",
            test: async () => {
                const stakeBefore = await getContractStake();

                const message = inkClient.message("add_stake_recycle");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount: tao(100),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();

                expect(stakeAfter).toEqual(stakeBefore);
            },
        });

        it({
            id: "T20",
            title: "Can add stake and burn resulting alpha",
            test: async () => {
                const stakeBefore = await getContractStake();
                const alphaOutBefore = await api.query.SubtensorModule.SubnetAlphaOut.getValue(netuid);

                const message = inkClient.message("add_stake_burn");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount: tao(100),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStake();
                const alphaOutAfter = await api.query.SubtensorModule.SubnetAlphaOut.getValue(netuid);

                expect(stakeAfter).toEqual(stakeBefore);
                expect(alphaOutAfter > alphaOutBefore).toBeTruthy();
            },
        });

        it({
            id: "T21",
            title: "Can caller add stake (fn 20)",
            test: async () => {
                await addStakeViaContract(false);
            },
        });

        it({
            id: "T22",
            title: "Can caller remove stake (fn 21)",
            test: async () => {
                await addStakeViaContract(false);
                const stake = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stake).toBeDefined();
                const amount = stake / BigInt(2);
                const message = inkClient.message("caller_remove_stake");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfter !== undefined && stakeAfter < stake!).toBeTruthy();
            },
        });

        it({
            id: "T23",
            title: "Can caller unstake_all (fn 22)",
            test: async () => {
                await addStakeViaContract(false);
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeBefore !== undefined && stakeBefore > BigInt(0)).toBeTruthy();
                const message = inkClient.message("caller_unstake_all");
                const data = message.encode({ hotkey: Binary.fromBytes(hotkey.publicKey) });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfter).toBeDefined();
                expect(stakeAfter < stakeBefore!).toBeTruthy();
            },
        });

        it({
            id: "T24",
            title: "Can caller unstake_all_alpha (fn 23)",
            test: async () => {
                await addStakeViaContract(false);
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeBefore !== undefined && stakeBefore > BigInt(0)).toBeTruthy();
                const message = inkClient.message("caller_unstake_all_alpha");
                const data = message.encode({ hotkey: Binary.fromBytes(hotkey.publicKey) });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfter).toBeDefined();
                expect(stakeAfter < stakeBefore!).toBeTruthy();
            },
        });

        it({
            id: "T25",
            title: "Can caller move_stake (fn 24)",
            test: async () => {
                await addStakeViaContract(false);
                await initSecondColdAndHotkey();
                const originStakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const destStakeBefore =
                    (
                        await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                            convertPublicKeyToSs58(hotkey2.publicKey),
                            convertPublicKeyToSs58(coldkey.publicKey),
                            netuid
                        )
                    )?.stake || BigInt(0);
                expect(originStakeBefore !== undefined && originStakeBefore > BigInt(0)).toBeTruthy();
                const moveAmount = originStakeBefore / BigInt(2);
                const message = inkClient.message("caller_move_stake");
                const data = message.encode({
                    origin_hotkey: Binary.fromBytes(hotkey.publicKey),
                    destination_hotkey: Binary.fromBytes(hotkey2.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid,
                    amount: moveAmount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const originStakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const destStakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey2.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(originStakeAfter !== undefined && destStakeAfter !== undefined).toBeTruthy();
                expect(originStakeAfter < originStakeBefore!).toBeTruthy();
                expect(destStakeAfter > destStakeBefore).toBeTruthy();
            },
        });

        it({
            id: "T26",
            title: "Can caller transfer_stake (fn 25)",
            test: async () => {
                await addStakeViaContract(false);
                await initSecondColdAndHotkey();
                const stakeBeforeOrigin = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const stakeBeforeDest = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey2.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeBeforeOrigin !== undefined && stakeBeforeOrigin > BigInt(0)).toBeTruthy();
                expect(stakeBeforeDest).toBeDefined();
                const transferAmount = stakeBeforeOrigin / BigInt(2);
                const message = inkClient.message("caller_transfer_stake");
                const data = message.encode({
                    destination_coldkey: Binary.fromBytes(coldkey2.publicKey),
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid,
                    amount: transferAmount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfterOrigin = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const stakeAfterDest = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey2.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfterOrigin !== undefined && stakeAfterDest !== undefined).toBeTruthy();
                expect(stakeAfterOrigin < stakeBeforeOrigin!).toBeTruthy();
                expect(stakeAfterDest > stakeBeforeDest!).toBeTruthy();
            },
        });

        it({
            id: "T27",
            title: "Can caller swap_stake (fn 26)",
            test: async () => {
                await addStakeViaContract(false);
                await initSecondColdAndHotkey();
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const stakeBefore2 =
                    (
                        await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                            convertPublicKeyToSs58(hotkey.publicKey),
                            convertPublicKeyToSs58(coldkey.publicKey),
                            netuid + 1
                        )
                    )?.stake || BigInt(0);
                expect(stakeBefore !== undefined && stakeBefore > BigInt(0)).toBeTruthy();
                const swapAmount = stakeBefore / BigInt(2);
                const message = inkClient.message("caller_swap_stake");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid + 1,
                    amount: swapAmount,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const stakeAfter2 = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid + 1
                    )
                )?.stake;
                expect(stakeAfter !== undefined && stakeAfter2 !== undefined).toBeTruthy();
                expect(stakeAfter < stakeBefore).toBeTruthy();
                expect(stakeAfter2 > stakeBefore2).toBeTruthy();
            },
        });

        it({
            id: "T28",
            title: "Can caller add_stake_limit (fn 27)",
            test: async () => {
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeBefore).toBeDefined();
                const message = inkClient.message("caller_add_stake_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount: tao(200),
                    limit_price: tao(100),
                    allow_partial: false,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfter !== undefined && stakeAfter > stakeBefore!).toBeTruthy();
            },
        });

        it({
            id: "T29",
            title: "Can caller remove_stake_limit (fn 28)",
            test: async () => {
                await addStakeViaContract(false);
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeBefore !== undefined && stakeBefore > BigInt(0)).toBeTruthy();
                const message = inkClient.message("caller_remove_stake_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    amount: stakeBefore / BigInt(2),
                    limit_price: tao(1),
                    allow_partial: false,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfter !== undefined && stakeAfter < stakeBefore!).toBeTruthy();
            },
        });

        it({
            id: "T30",
            title: "Can caller swap_stake_limit (fn 29)",
            test: async () => {
                await addStakeViaContract(false);
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const stakeBefore2 = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid + 1
                    )
                )?.stake;
                expect(stakeBefore !== undefined && stakeBefore > BigInt(0)).toBeTruthy();
                expect(stakeBefore2).toBeDefined();
                const message = inkClient.message("caller_swap_stake_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    origin_netuid: netuid,
                    destination_netuid: netuid + 1,
                    amount: stakeBefore / BigInt(2),
                    limit_price: tao(1),
                    allow_partial: false,
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                const stakeAfter2 = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid + 1
                    )
                )?.stake;
                expect(stakeAfter !== undefined && stakeAfter2 !== undefined).toBeTruthy();
                expect(stakeAfter < stakeBefore).toBeTruthy();
                expect(stakeAfter2 > stakeBefore2!).toBeTruthy();
            },
        });

        it({
            id: "T31",
            title: "Can caller remove_stake_full_limit (fn 30)",
            test: async () => {
                await addStakeViaContract(false);
                const stakeBefore = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeBefore !== undefined && stakeBefore > BigInt(0)).toBeTruthy();
                const message = inkClient.message("caller_remove_stake_full_limit");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid,
                    limit_price: BigInt(0),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const stakeAfter = (
                    await api.apis.StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        convertPublicKeyToSs58(coldkey.publicKey),
                        netuid
                    )
                )?.stake;
                expect(stakeAfter !== undefined && stakeAfter < stakeBefore!).toBeTruthy();
            },
        });

        it({
            id: "T32",
            title: "Can caller set_coldkey_auto_stake_hotkey (fn 31)",
            test: async () => {
                await addStakeViaContract(false);
                await initSecondColdAndHotkey();
                const message = inkClient.message("caller_set_coldkey_auto_stake_hotkey");
                const data = message.encode({
                    netuid,
                    hotkey: Binary.fromBytes(hotkey2.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, data);
                const autoStakeHotkey = await api.query.SubtensorModule.AutoStakeDestination.getValue(
                    convertPublicKeyToSs58(coldkey.publicKey),
                    netuid
                );
                expect(autoStakeHotkey).toEqual(convertPublicKeyToSs58(hotkey2.publicKey));
            },
        });

        it({
            id: "T33",
            title: "Can caller add_proxy and remove_proxy (fn 32-33)",
            test: async () => {
                const addMessage = inkClient.message("caller_add_proxy");
                const addData = addMessage.encode({
                    delegate: Binary.fromBytes(hotkey2.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, addData);
                let proxies = await api.query.Proxy.Proxies.getValue(convertPublicKeyToSs58(coldkey.publicKey));
                expect(proxies !== undefined && proxies[0].length > 0).toBeTruthy();
                expect(proxies[0][0].delegate).toEqual(convertPublicKeyToSs58(hotkey2.publicKey));

                const removeMessage = inkClient.message("caller_remove_proxy");
                const removeData = removeMessage.encode({
                    delegate: Binary.fromBytes(hotkey2.publicKey),
                });
                await sendWasmContractExtrinsic(api, coldkey, contractAddress, removeData);
                proxies = await api.query.Proxy.Proxies.getValue(convertPublicKeyToSs58(coldkey.publicKey));
                expect(proxies !== undefined && proxies[0].length).toEqual(0);
            },
        });

        it({
            id: "T34",
            title: "Check add_stake_recycle is atomic operation",
            test: async () => {
                const stakeBefore = await getContractStakeOnRoot();
                const balanceBefore = await getBalance(api, convertPublicKeyToSs58(coldkey.publicKey));

                // recycle alpha on root subnet is not allowed, the extrinsic should be failed.
                const message = inkClient.message("add_stake_recycle");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid: 0,
                    amount: tao(100),
                });
                await sendWasmContractExtrinsicAllowFailure(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStakeOnRoot();
                const balanceAfter = await getBalance(api, convertPublicKeyToSs58(coldkey.publicKey));

                expect(balanceBefore - balanceAfter < 10_000_000).toBeTruthy();
                expect(stakeAfter).toEqual(stakeBefore);
            },
        });

        it({
            id: "T35",
            title: "Check add_stake_burn is atomic operation",
            test: async () => {
                const stakeBefore = await getContractStakeOnRoot();
                const balanceBefore = await getBalance(api, convertPublicKeyToSs58(coldkey.publicKey));
                const alphaOutBefore = await api.query.SubtensorModule.SubnetAlphaOut.getValue(netuid);

                const message = inkClient.message("add_stake_burn");
                const data = message.encode({
                    hotkey: Binary.fromBytes(hotkey.publicKey),
                    netuid: 0,
                    amount: tao(100),
                });
                await sendWasmContractExtrinsicAllowFailure(api, coldkey, contractAddress, data);

                const stakeAfter = await getContractStakeOnRoot();
                const alphaOutAfter = await api.query.SubtensorModule.SubnetAlphaOut.getValue(netuid);
                const balanceAfter = await getBalance(api, convertPublicKeyToSs58(coldkey.publicKey));

                expect(balanceBefore - balanceAfter < 10_000_000).toBeTruthy();
                expect(stakeAfter).toEqual(stakeBefore);
                expect(alphaOutAfter > alphaOutBefore).toBeTruthy();
            },
        });

        it({
            id: "T36",
            title: "Can get subnet registration state",
            test: async () => {
                const queryMessage = inkClient.message("get_subnet_registration_state");

                const data = queryMessage.encode({
                    netuid: netuid,
                });

                const response = await api.apis.ContractsApi.call(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    contractAddress,
                    BigInt(0),
                    undefined,
                    undefined,
                    Binary.fromBytes(data.asBytes())
                );

                expect(response.result.success).toBeTruthy();
                const result = queryMessage.decode(response.result.value).value.value;
                if (
                    typeof result === "object" &&
                    "netuid" in result &&
                    "exists" in result &&
                    "registered_subnet_counter" in result
                ) {
                    expect(result.netuid).toEqual(netuid);
                    expect(result.registered_subnet_counter).toBeGreaterThanOrEqual(BigInt(0));
                    expect(result.exists).toEqual(true);
                } else {
                    throw new Error("result is not an object");
                }
            },
        });

        it({
            id: "T37",
            title: "Can get coldkey lock",
            test: async () => {
                const queryMessage = inkClient.message("get_coldkey_lock");
                const queryArgs = {
                    coldkey: Binary.fromBytes(coldkey.publicKey),
                    netuid: netuid,
                };

                async function queryColdkeyLock() {
                    const data = queryMessage.encode(queryArgs);
                    const response = await api.apis.ContractsApi.call(
                        convertPublicKeyToSs58(hotkey.publicKey),
                        contractAddress,
                        BigInt(0),
                        undefined,
                        undefined,
                        Binary.fromBytes(data.asBytes())
                    );
                    expect(response.result.success).toBeTruthy();
                    return queryMessage.decode(response.result.value).value.value as
                        | {
                              locked_mass: bigint;
                              conviction_bits: bigint;
                              last_update: bigint;
                          }
                        | undefined;
                }

                let lock = await queryColdkeyLock();
                if (!lock) {
                    await addStakeViaContract(false);

                    const lockAmount = tao(1);
                    const lockTx = api.tx.SubtensorModule.lock_stake({
                        hotkey: convertPublicKeyToSs58(hotkey.publicKey),
                        netuid: netuid,
                        amount: lockAmount,
                    });
                    await waitForTransactionWithRetry(api, lockTx, coldkey, "lock_stake");

                    lock = await queryColdkeyLock();
                }

                expect(lock).toBeDefined();

                if (
                    typeof lock === "object" &&
                    "locked_mass" in lock &&
                    "conviction_bits" in lock &&
                    "last_update" in lock
                ) {
                    expect(lock.locked_mass).toBeGreaterThanOrEqual(BigInt(0));
                    expect(lock.conviction_bits).toBeGreaterThanOrEqual(BigInt(0));
                    expect(lock.last_update).toBeGreaterThanOrEqual(BigInt(0));
                } else {
                    throw new Error("result is not an object");
                }
            },
        });

        it({
            id: "T38",
            title: "Can get stake availability",
            test: async () => {
                const queryMessage = inkClient.message("get_stake_availability");

                const data = queryMessage.encode({
                    coldkey: Binary.fromBytes(coldkey.publicKey),
                    netuid: netuid,
                });

                const response = await api.apis.ContractsApi.call(
                    convertPublicKeyToSs58(hotkey.publicKey),
                    contractAddress,
                    BigInt(0),
                    undefined,
                    undefined,
                    Binary.fromBytes(data.asBytes())
                );

                expect(response.result.success).toBeTruthy();
                const result = queryMessage.decode(response.result.value).value.value;
                if (
                    typeof result === "object" &&
                    "netuid" in result &&
                    "locked" in result &&
                    "available" in result &&
                    "total" in result
                ) {
                    expect(result.netuid).toEqual(netuid);
                    expect(result.locked).toBeGreaterThanOrEqual(BigInt(0));
                    expect(result.available).toBeGreaterThanOrEqual(BigInt(0));
                    expect(result.total).toBeGreaterThanOrEqual(BigInt(0));
                } else {
                    throw new Error("result is not an object");
                }
            },
        });
    },
});
