import { beforeAll, describeSuite, expect } from "@moonwall/cli";
import { subtensor } from "@polkadot-api/descriptors";
import type { KeyringPair } from "@polkadot/keyring/types";
import { u8aToHex } from "@polkadot/util";
import { ethers } from "ethers";
import { Binary, type TypedApi } from "polkadot-api";
import {
    addNewSubnetwork,
    convertH160ToPublicKey,
    convertH160ToSS58,
    createEthersWallet,
    disableWhiteListCheck,
    enableGamma,
    feedTaoUsdPrice,
    forceSetBalance,
    gamma,
    generateKeyringPair,
    getBalance,
    getGammaBalance,
    getGammaDeposit,
    getStake,
    IGAMMA_ADDRESS,
    IGammaABI,
    ISTAKING_V2_ADDRESS,
    IStakingV2ABI,
    ISUBNET_ADDRESS,
    ISubnetABI,
    MINT_FOR_USER_CONTRACT_ABI,
    MINT_FOR_USER_CONTRACT_BYTECODE,
    startCall,
    sudoSetLockReductionInterval,
    sudoSetSubnetGammaEmissionWhitelisted,
    tao,
    waitForTransactionWithRetry,
    waitUntilBlockFinalized,
} from "../../utils";

// Enough gas to run a refused call on chain instead of failing its estimate.
const GAS_LIMIT = 1_000_000;
const MAX_SHARE = BigInt("18446744073709551615"); // u64::MAX, one whole emission

describeSuite({
    id: "gamma-precompile",
    title: "Gamma and subnet precompile gamma tests",
    foundationMethods: "zombie",
    testCases: ({ it, context }) => {
        let api: TypedApi<typeof subtensor>;
        let ethWallet: ethers.Wallet;
        let ethWallet2: ethers.Wallet;
        let walletSs58: string;
        let walletPublicKey: Uint8Array;
        let gammaPrecompile: ethers.Contract;
        let subnetPrecompile: ethers.Contract;
        let ownerHotkey: KeyringPair;
        let netuid: number;

        beforeAll(async () => {
            api = context.papi("Node").getTypedApi(subtensor);
            const provider = context.ethers("EVM").provider as ethers.JsonRpcProvider;

            ethWallet = createEthersWallet(provider);
            ethWallet2 = createEthersWallet(provider);
            walletSs58 = convertH160ToSS58(ethWallet.address);
            walletPublicKey = convertH160ToPublicKey(ethWallet.address);
            await forceSetBalance(api, walletSs58);
            await forceSetBalance(api, convertH160ToSS58(ethWallet2.address));
            await disableWhiteListCheck(api, true);

            ownerHotkey = generateKeyringPair("sr25519");
            const ownerColdkey = generateKeyringPair("sr25519");
            await forceSetBalance(api, ownerHotkey.address);
            await forceSetBalance(api, ownerColdkey.address);

            await sudoSetLockReductionInterval(api, 1);
            netuid = await addNewSubnetwork(api, ownerHotkey, ownerColdkey);
            await startCall(api, netuid, ownerColdkey);
            await enableGamma(api, netuid);

            // The subnet owner's hotkey publishes a service, so hotkeys can spend gamma at it.
            const setService = api.tx.SubtensorModule.set_service({
                netuid: netuid,
                hotkey: ownerHotkey.address,
                url: Binary.fromText("https://service.example/services.json"),
            });
            await waitForTransactionWithRetry(api, setService, ownerColdkey, "set_service");

            gammaPrecompile = new ethers.Contract(IGAMMA_ADDRESS, IGammaABI, ethWallet);
            subnetPrecompile = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, ethWallet);
        }, 600000);

        /** Sends a call and waits for its block to finalize, so chain reads see it. */
        async function send(call: Promise<ethers.ContractTransactionResponse>): Promise<ethers.TransactionReceipt> {
            const receipt = await (await call).wait();
            if (receipt == null) throw new Error("Transaction was not mined");
            expect(receipt.status).toEqual(1);
            await waitUntilBlockFinalized(api, receipt.blockNumber);
            return receipt;
        }

        /** Sends a call with a fixed gas limit, so it runs on chain, and expects it to fail. */
        async function sendRefused(call: Promise<ethers.ContractTransactionResponse>): Promise<void> {
            const tx = await call;
            await expect(tx.wait()).rejects.toMatchObject({ code: "CALL_EXCEPTION" });
            const receipt = await tx.provider.getTransactionReceipt(tx.hash);
            if (receipt == null) throw new Error("Transaction was not mined");
            expect(receipt.status).toEqual(0);
            await waitUntilBlockFinalized(api, receipt.blockNumber);
        }

        it({
            id: "T01",
            title: "Reads gamma as enabled network-wide and on the subnet",
            test: async () => {
                expect(await gammaPrecompile.getGammaEnabled()).toBe(true);
                expect(await subnetPrecompile.getSubnetGammaEnabled(netuid)).toBe(true);
            },
        });

        it({
            id: "T02",
            title: "Simulates a mint as the runtime API does",
            test: async () => {
                await feedTaoUsdPrice(api);
                // Both read the same finalized block, which the client keeps pinned.
                const block = await context.papi("Node").getFinalizedBlock();

                const viaPrecompile = await gammaPrecompile.simMintGamma(netuid, tao(1), { blockTag: block.number });
                const viaRuntimeApi = await api.apis.GammaRuntimeApi.sim_mint_gamma(netuid, tao(1), {
                    at: block.hash,
                });
                if (!viaRuntimeApi.success) throw new Error("sim_mint_gamma failed");

                expect([viaPrecompile.alpha, viaPrecompile.value, viaPrecompile.gamma]).toEqual(viaRuntimeApi.value);
                expect(viaPrecompile.gamma).toBeGreaterThan(gamma(1));
            },
        });

        it({
            id: "T03",
            title: "Mints gamma into the caller's deposit and withdraws it to the balance",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519");
                const balanceBefore = await getGammaBalance(api, netuid, walletSs58);

                await feedTaoUsdPrice(api);
                await send(gammaPrecompile.mintGamma(hotkey.publicKey, netuid, tao(10), 0, false, 0));

                const deposit = await gammaPrecompile.getGammaDeposit(netuid, hotkey.publicKey);
                expect(deposit.exists).toBe(true);
                expect(deposit.coldkey).toEqual(u8aToHex(walletPublicKey));
                expect(deposit.balance).toBeGreaterThan(gamma(1));

                await send(gammaPrecompile.withdrawGamma(hotkey.publicKey, netuid, deposit.balance));

                const balanceAfter = await getGammaBalance(api, netuid, walletSs58);
                expect(balanceAfter).toEqual(balanceBefore + deposit.balance);
                expect(await gammaPrecompile.getGammaBalance(netuid, walletPublicKey)).toEqual(balanceAfter);
            },
        });

        it({
            id: "T04",
            title: "Mints gamma with stake into the caller's deposit at a hotkey",
            test: async () => {
                const stakingPrecompile = new ethers.Contract(ISTAKING_V2_ADDRESS, IStakingV2ABI, ethWallet);
                await send(stakingPrecompile.addStake(ownerHotkey.publicKey, tao(10), netuid));
                const stakeBefore = await getStake(api, ownerHotkey.address, walletSs58, netuid);
                const hotkey = generateKeyringPair("sr25519");

                await feedTaoUsdPrice(api);
                await send(
                    gammaPrecompile.mintGammaWithStake(
                        ownerHotkey.publicKey,
                        hotkey.publicKey,
                        netuid,
                        stakeBefore / 2n,
                        0,
                        false,
                        0
                    )
                );

                expect(await getStake(api, ownerHotkey.address, walletSs58, netuid)).toBeLessThan(stakeBefore);
                expect((await gammaPrecompile.getGammaDeposit(netuid, hotkey.publicKey)).balance).toBeGreaterThan(0n);
            },
        });

        it({
            id: "T05",
            title: "Deposits gamma at a hotkey and withdraws it back",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519");
                const balanceBefore = await getGammaBalance(api, netuid, walletSs58);

                await send(gammaPrecompile.depositGamma(hotkey.publicKey, netuid, gamma(3)));

                const deposit = await gammaPrecompile.getGammaDeposit(netuid, hotkey.publicKey);
                expect(deposit.exists).toBe(true);
                expect(deposit.coldkey).toEqual(u8aToHex(walletPublicKey));
                expect(deposit.balance).toEqual(gamma(3));
                expect(deposit.hasDailySpendCap).toBe(false);
                expect(deposit.spendable).toEqual(gamma(3));
                expect(await gammaPrecompile.getGammaDepositHotkeys(netuid, walletPublicKey)).toContain(
                    u8aToHex(hotkey.publicKey)
                );
                expect(await getGammaBalance(api, netuid, walletSs58)).toEqual(balanceBefore - gamma(3));

                await send(gammaPrecompile.withdrawGamma(hotkey.publicKey, netuid, gamma(3)));

                expect((await gammaPrecompile.getGammaDeposit(netuid, hotkey.publicKey)).exists).toBe(false);
                expect(await getGammaDeposit(api, netuid, hotkey.address)).toBeUndefined();
                expect(await getGammaBalance(api, netuid, walletSs58)).toEqual(balanceBefore);
            },
        });

        it({
            id: "T06",
            title: "Moves gamma between deposits and caps one",
            test: async () => {
                const origin = generateKeyringPair("sr25519");
                const destination = generateKeyringPair("sr25519");
                await send(gammaPrecompile.depositGamma(origin.publicKey, netuid, gamma(2)));

                await send(gammaPrecompile.moveGamma(origin.publicKey, destination.publicKey, netuid, gamma(2)));

                expect(
                    await getGammaDeposit(api, netuid, origin.address),
                    "emptying a deposit closes it"
                ).toBeUndefined();
                expect((await getGammaDeposit(api, netuid, destination.address))?.balance).toEqual(gamma(2));

                await send(gammaPrecompile.setGammaDepositDailySpendCap(destination.publicKey, netuid, true, gamma(1)));

                const capped = await gammaPrecompile.getGammaDeposit(netuid, destination.publicKey);
                expect(capped.hasDailySpendCap).toBe(true);
                expect(capped.dailySpendCap).toEqual(gamma(1));
                expect(capped.spendable).toBeLessThanOrEqual(gamma(1));
                expect((await getGammaDeposit(api, netuid, destination.address))?.daily_spend_cap?.cap).toEqual(
                    gamma(1)
                );
            },
        });

        it({
            id: "T07",
            title: "Transfers gamma to another coldkey's balance and into a deposit it opens",
            test: async () => {
                const receiver = generateKeyringPair("sr25519");
                const hotkey = generateKeyringPair("sr25519");
                const balanceBefore = await getGammaBalance(api, netuid, walletSs58);

                await send(
                    gammaPrecompile.transferGamma(
                        receiver.publicKey,
                        false,
                        ethers.ZeroHash,
                        false,
                        ethers.ZeroHash,
                        netuid,
                        gamma(1)
                    )
                );
                await send(
                    gammaPrecompile.transferGamma(
                        receiver.publicKey,
                        false,
                        ethers.ZeroHash,
                        true,
                        hotkey.publicKey,
                        netuid,
                        gamma(1)
                    )
                );

                expect(await getGammaBalance(api, netuid, receiver.address)).toEqual(gamma(1));
                const deposit = await getGammaDeposit(api, netuid, hotkey.address);
                expect(deposit?.coldkey).toEqual(receiver.address);
                expect(deposit?.balance).toEqual(gamma(1));
                expect(deposit?.daily_spend_cap).toBeUndefined();
                expect(await getGammaBalance(api, netuid, walletSs58)).toEqual(balanceBefore - gamma(2));
            },
        });

        it({
            id: "T08",
            title: "A hotkey spends its deposit at a service and its coldkey closes the empty deposit",
            test: async () => {
                // The second wallet's account is the hotkey of a deposit the first wallet owns.
                const hotkeyPublicKey = convertH160ToPublicKey(ethWallet2.address);
                await send(gammaPrecompile.depositGamma(hotkeyPublicKey, netuid, gamma(1)));

                const asHotkey = gammaPrecompile.connect(ethWallet2) as ethers.Contract;
                await send(asHotkey.spendGamma(ownerHotkey.publicKey, netuid, gamma(1)));

                expect(await gammaPrecompile.getGammaSpent(netuid, ownerHotkey.publicKey, hotkeyPublicKey)).toEqual(
                    gamma(1)
                );
                const spent = await gammaPrecompile.getGammaDeposit(netuid, hotkeyPublicKey);
                expect(spent.exists, "a deposit emptied by spending stays open").toBe(true);
                expect(spent.balance).toEqual(BigInt(0));

                await send(gammaPrecompile.closeGammaDeposit(hotkeyPublicKey, netuid));

                expect((await gammaPrecompile.getGammaDeposit(netuid, hotkeyPublicKey)).exists).toBe(false);
            },
        });

        it({
            id: "T09",
            title: "Refuses a transfer below 0.1 gamma and changes nothing",
            test: async () => {
                const receiver = generateKeyringPair("sr25519");
                const balanceBefore = await getGammaBalance(api, netuid, walletSs58);

                await sendRefused(
                    gammaPrecompile.transferGamma(
                        receiver.publicKey,
                        false,
                        ethers.ZeroHash,
                        false,
                        ethers.ZeroHash,
                        netuid,
                        gamma(1) / 10n - 1n,
                        { gasLimit: GAS_LIMIT }
                    )
                );

                expect(await getGammaBalance(api, netuid, walletSs58)).toEqual(balanceBefore);
                expect(await getGammaBalance(api, netuid, receiver.address)).toEqual(BigInt(0));
            },
        });

        it({
            id: "T10",
            title: "A contract mints gamma for a user, and a refused transfer undoes the mint",
            test: async () => {
                const factory = new ethers.ContractFactory(
                    MINT_FOR_USER_CONTRACT_ABI,
                    MINT_FOR_USER_CONTRACT_BYTECODE,
                    ethWallet
                );
                const deployed = await factory.deploy();
                await deployed.waitForDeployment();
                const contractAddress = await deployed.getAddress();
                const contractSs58 = convertH160ToSS58(contractAddress);
                await forceSetBalance(api, contractSs58);
                const contract = new ethers.Contract(contractAddress, MINT_FOR_USER_CONTRACT_ABI, ethWallet);

                const user = generateKeyringPair("sr25519");
                const userHotkey = generateKeyringPair("sr25519");
                await feedTaoUsdPrice(api);
                const receipt = await send(contract.mintFor(netuid, tao(1), user.publicKey, userHotkey.publicKey));

                const minted = receipt.logs
                    .map((entry) => contract.interface.parseLog(entry))
                    .find((event) => event?.name === "Minted");
                if (minted == null) throw new Error("Minted event not found");
                const credited: bigint = minted.args.credited;
                expect(credited).toBeGreaterThan(gamma(1));

                const deposit = await getGammaDeposit(api, netuid, userHotkey.address);
                expect(deposit?.coldkey, "the user owns the deposit").toEqual(user.address);
                expect(deposit?.balance, "the deposit holds what the mint returned").toEqual(credited);
                expect(await getGammaBalance(api, netuid, contractSs58)).toEqual(BigInt(0));

                // The subnet owner's coldkey owns its hotkey, so the transfer to the user is refused.
                const taoBefore = await getBalance(api, contractSs58);
                await feedTaoUsdPrice(api);
                await sendRefused(
                    contract.mintFor(netuid, tao(1), user.publicKey, ownerHotkey.publicKey, { gasLimit: GAS_LIMIT })
                );

                expect(await getBalance(api, contractSs58), "the mint's TAO is back").toEqual(taoBefore);
                expect(await getGammaBalance(api, netuid, contractSs58)).toEqual(BigInt(0));
                expect(await getGammaDeposit(api, netuid, ownerHotkey.address)).toBeUndefined();
            },
        });

        it({
            id: "T11",
            title: "The subnet owner enables gamma, sets gamma shares and publishes its service",
            test: async () => {
                const hotkey = generateKeyringPair("sr25519");
                await forceSetBalance(api, hotkey.address);
                const ownedNetuid = await api.query.SubtensorModule.TotalNetworks.getValue();
                await send(subnetPrecompile.registerNetwork(hotkey.publicKey));
                expect(await api.query.SubtensorModule.SubnetOwner.getValue(ownedNetuid)).toEqual(walletSs58);

                const stranger = subnetPrecompile.connect(ethWallet2) as ethers.Contract;
                await sendRefused(stranger.setSubnetGammaEnabled(ownedNetuid, true, { gasLimit: GAS_LIMIT }));
                expect(await subnetPrecompile.getSubnetGammaEnabled(ownedNetuid)).toBe(false);

                await send(subnetPrecompile.setSubnetGammaEnabled(ownedNetuid, true));
                expect(await subnetPrecompile.getSubnetGammaEnabled(ownedNetuid)).toBe(true);
                expect(await api.query.SubtensorModule.SubnetGammaEnabled.getValue(ownedNetuid)).toBe(true);

                // A gamma share buys gamma on a whitelisted subnet with gamma enabled.
                await sudoSetSubnetGammaEmissionWhitelisted(api, netuid, true);
                const [weights] = await subnetPrecompile.getSubnetEmissionWeightsV2(ownedNetuid);
                const [burn, recycle, owner, validator, miner] = weights as bigint[];
                const share = MAX_SHARE / 20n;
                await sendRefused(
                    stranger.setSubnetEmissionWeightsV2(
                        ownedNetuid,
                        burn,
                        recycle,
                        owner,
                        validator,
                        miner - share,
                        [netuid],
                        [share],
                        { gasLimit: GAS_LIMIT }
                    )
                );
                await send(
                    subnetPrecompile.setSubnetEmissionWeightsV2(
                        ownedNetuid,
                        burn,
                        recycle,
                        owner,
                        validator,
                        miner - share,
                        [netuid],
                        [share]
                    )
                );

                const updated = await subnetPrecompile.getSubnetEmissionWeightsV2(ownedNetuid);
                expect(updated.weights).toEqual([burn, recycle, owner, validator, miner - share]);
                expect(updated.gammaNetuids).toEqual([BigInt(netuid)]);
                expect(updated.gammaShares).toEqual([share]);
                expect(await api.query.SubtensorModule.SubnetGammaEmission.getValue(ownedNetuid)).toEqual([
                    [netuid, share],
                ]);

                const url = "https://owned.example/services.json";
                await send(gammaPrecompile.setService(ownedNetuid, hotkey.publicKey, url));
                const service = await gammaPrecompile.getService(ownedNetuid, hotkey.publicKey);
                expect(service.exists).toBe(true);
                expect(service.url).toEqual(url);
            },
        });
    },
});
