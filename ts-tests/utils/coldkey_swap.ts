import { blake2_256, bytesToHex } from "@bittensor/sdk";
import type { KeyringPair } from "@moonwall/util";
import { keyringPairFromUri } from "./account.ts";
import { waitForTransactionWithRetry } from "./transactions.js";
import type { TypedApi } from "polkadot-api";
import type { subtensor } from "@polkadot-api/descriptors";
import { FixedSizeBinary } from "polkadot-api";

export const ANNOUNCEMENT_DELAY = 20;
export const REANNOUNCEMENT_DELAY = 20;

/** Compute BLAKE2-256 hash of a keypair's public key as a FixedSizeBinary (used for announcements). */
export function coldkeyHashBinary(pair: KeyringPair): FixedSizeBinary<32> {
    return FixedSizeBinary.fromHex(bytesToHex(blake2_256(pair.publicKey)));
}

/** Compute BLAKE2-256 hash of a keypair's public key as hex string. */
export function coldkeyHash(pair: KeyringPair): string {
    return bytesToHex(blake2_256(pair.publicKey));
}

// ── Sudo configuration ──────────────────────────────────────────────────

export async function sudoSetAnnouncementDelay(api: TypedApi<typeof subtensor>, delay: number): Promise<void> {
    const alice = keyringPairFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_coldkey_swap_announcement_delay({
        duration: delay,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_coldkey_swap_announcement_delay");
}

export async function sudoSetReannouncementDelay(api: TypedApi<typeof subtensor>, delay: number): Promise<void> {
    const alice = keyringPairFromUri("//Alice");
    const internalCall = api.tx.AdminUtils.sudo_set_coldkey_swap_reannouncement_delay({
        duration: delay,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_set_coldkey_swap_reannouncement_delay");
}

// ── Transaction wrappers (throw on failure) ─────────────────────────────

export async function announceColdkeySwap(
    api: TypedApi<typeof subtensor>,
    signer: KeyringPair,
    newColdkeyHash: FixedSizeBinary<32>
): Promise<void> {
    const tx = api.tx.SubtensorModule.announce_coldkey_swap({
        new_coldkey_hash: newColdkeyHash,
    });
    await waitForTransactionWithRetry(api, tx, signer, "announce_coldkey_swap");
}

export async function swapColdkeyAnnounced(
    api: TypedApi<typeof subtensor>,
    signer: KeyringPair,
    newAddress: string
): Promise<void> {
    const tx = api.tx.SubtensorModule.swap_coldkey_announced({
        new_coldkey: newAddress,
    });
    await waitForTransactionWithRetry(api, tx, signer, "swap_coldkey_announced");
}

export async function disputeColdkeySwap(api: TypedApi<typeof subtensor>, signer: KeyringPair): Promise<void> {
    const tx = api.tx.SubtensorModule.dispute_coldkey_swap();
    await waitForTransactionWithRetry(api, tx, signer, "dispute_coldkey_swap");
}

export async function clearColdkeySwapAnnouncement(
    api: TypedApi<typeof subtensor>,
    signer: KeyringPair
): Promise<void> {
    const tx = api.tx.SubtensorModule.clear_coldkey_swap_announcement();
    await waitForTransactionWithRetry(api, tx, signer, "clear_coldkey_swap_announcement");
}

export async function sudoResetColdkeySwap(api: TypedApi<typeof subtensor>, coldkeyAddress: string): Promise<void> {
    const alice = keyringPairFromUri("//Alice");
    const internalCall = api.tx.SubtensorModule.reset_coldkey_swap({
        coldkey: coldkeyAddress,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_reset_coldkey_swap");
}

export async function sudoSwapColdkey(
    api: TypedApi<typeof subtensor>,
    oldAddress: string,
    newAddress: string,
    swapCost = 0n
): Promise<void> {
    const alice = keyringPairFromUri("//Alice");
    const internalCall = api.tx.SubtensorModule.swap_coldkey({
        old_coldkey: oldAddress,
        new_coldkey: newAddress,
        swap_cost: swapCost,
    });
    const tx = api.tx.Sudo.sudo({ call: internalCall.decodedCall });
    await waitForTransactionWithRetry(api, tx, alice, "sudo_swap_coldkey");
}

// ── Storage query helpers ───────────────────────────────────────────────

export async function getColdkeySwapAnnouncement(
    api: TypedApi<typeof subtensor>,
    address: string
): Promise<{ when: number; hash: string } | null> {
    const result = await api.query.SubtensorModule.ColdkeySwapAnnouncements.getValue(address);
    if (!result) return null;
    const [when, hash] = result;
    return { when, hash: hash.asHex() };
}

export async function getColdkeySwapDispute(api: TypedApi<typeof subtensor>, address: string): Promise<number | null> {
    const result = await api.query.SubtensorModule.ColdkeySwapDisputes.getValue(address);
    if (result === undefined) return null;
    return Number(result);
}

/** Get the owner coldkey of a hotkey. */
export async function getHotkeyOwner(api: TypedApi<typeof subtensor>, hotkey: string): Promise<string> {
    return await api.query.SubtensorModule.Owner.getValue(hotkey);
}

/** Get the list of hotkeys owned by a coldkey. */
export async function getOwnedHotkeys(api: TypedApi<typeof subtensor>, coldkey: string): Promise<string[]> {
    return await api.query.SubtensorModule.OwnedHotkeys.getValue(coldkey);
}

/** Get the list of hotkeys a coldkey is staking to. */
export async function getStakingHotkeys(api: TypedApi<typeof subtensor>, coldkey: string): Promise<string[]> {
    return await api.query.SubtensorModule.StakingHotkeys.getValue(coldkey);
}

/** Get the owner coldkey of a subnet. */
export async function getSubnetOwner(api: TypedApi<typeof subtensor>, netuid: number): Promise<string> {
    return await api.query.SubtensorModule.SubnetOwner.getValue(netuid);
}
