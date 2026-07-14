import type { DevModeContext } from "@moonwall/cli";
import type { KeyringPair } from "@moonwall/util";
import type { ApiPromise } from "@polkadot/api";
import type { SubmittableExtrinsic } from "@polkadot/api/types";
import { generateKeyringPair } from "./account";

export type Collective = "Proposers" | "Triumvirate" | "Economic" | "Building" | "EconomicEligible";

export type ReferendumStatusKind =
    | "ongoing"
    | "approved"
    | "delegated"
    | "rejected"
    | "cancelled"
    | "expired"
    | "fastTracked"
    | "enacted"
    | "killed";

export type DispatchModuleError = { section: string; name: string };
export type DispatchFailure = DispatchModuleError | { kind: string; raw: string };
export type EventRecordLike = {
    event: {
        section: string;
        method: string;
        data: { toJSON(): unknown } & ArrayLike<unknown>;
    };
};

type NumberCodecLike = { toNumber(): number; toJSON(): unknown };
type OptionCodecLike = { isNone: boolean; isSome: boolean; toJSON(): unknown };
type AccountInfoLike = { data: { free: { toBigInt(): bigint } } };

export const DEV_TRACK = { TRIUMVIRATE: 0, REVIEW: 1 } as const;
export const DEFAULT_FUND = 1_000_000_000_000n;

type SudoExtrinsic = SubmittableExtrinsic<"promise">;

/**
 * Sign an extrinsic with `signer` and seal it into a fresh block.
 *
 * Transactions are signed with `era: 0` (immortal). Mortal extrinsics check
 * their birth block against `BlockHash<T>`; under the parallel test runner,
 * the in-process `ApiPromise` can briefly hold a stale "best block" while
 * other forks' nodes drive their own chains forward, and a freshly signed
 * mortal tx can be rejected as `AncientBirthBlock` before it reaches the
 * pool. Immortal signing sidesteps that race without changing observable
 * behavior on the chain under test.
 */
export async function inBlock(context: DevModeContext, signer: KeyringPair, tx: SudoExtrinsic): Promise<void> {
    await context.createBlock([await tx.signAsync(signer, { era: 0 })]);
}

/** Wrap `inner` in `sudo.sudo` and execute it in its own block as `sudoer`. */
export async function sudoInBlock(
    api: ApiPromise,
    context: DevModeContext,
    sudoer: KeyringPair,
    inner: SudoExtrinsic
): Promise<void> {
    await inBlock(context, sudoer, api.tx.sudo.sudo(inner));
}

/** Top up the free balance of each address. Idempotent on repeat addresses. */
export async function fundAccounts(
    api: ApiPromise,
    context: DevModeContext,
    sudoer: KeyringPair,
    addresses: string[],
    fund: bigint = DEFAULT_FUND
): Promise<void> {
    const seen = new Set<string>();
    for (const address of addresses) {
        if (seen.has(address)) continue;
        seen.add(address);
        await sudoInBlock(api, context, sudoer, api.tx.balances.forceSetBalance(address, fund));
    }
}

/** Add each `{collective, account}` entry to its collective. */
export async function addMembers(
    api: ApiPromise,
    context: DevModeContext,
    sudoer: KeyringPair,
    entries: Array<{ collective: Collective; account: KeyringPair | string }>
): Promise<void> {
    for (const { collective, account } of entries) {
        const address = typeof account === "string" ? account : account.address;
        await sudoInBlock(api, context, sudoer, api.tx.multiCollective.addMember(collective, address));
    }
}

export type GovernanceMembership = {
    /** First Proposer; convenient default for tests that only need one. */
    proposer: KeyringPair;
    /** Full Proposers list, length matches `layout.proposers` (≥ 1). */
    proposers: KeyringPair[];
    triumvirate: KeyringPair[];
    economic: KeyringPair[];
    building: KeyringPair[];
};

export type MembershipLayout = {
    triumvirate: number;
    economic: number;
    building: number;
    /**
     * How many Proposers to seat. Distinct proposers are useful when a single
     * suite needs to file more than `MaxActivePerProposer` (= 5) referenda
     * without freeing slots first. Defaults to 1.
     */
    proposers?: number;
};

/**
 * Mint and seat a standard membership layout. Returns the generated keypairs
 * so tests can keep using them.
 *
 * Triumvirate must equal 3 to satisfy `min_members` once seeded; the others
 * accept any size up to the per-collective `max_members`.
 */
export async function bootstrapMembership(
    api: ApiPromise,
    context: DevModeContext,
    sudoer: KeyringPair,
    layout: MembershipLayout
): Promise<GovernanceMembership> {
    const proposerCount = layout.proposers ?? 1;
    const proposers = Array.from({ length: proposerCount }, () => generateKeyringPair("sr25519"));
    const triumvirate = Array.from({ length: layout.triumvirate }, () => generateKeyringPair("sr25519"));
    const economic = Array.from({ length: layout.economic }, () => generateKeyringPair("sr25519"));
    const building = Array.from({ length: layout.building }, () => generateKeyringPair("sr25519"));

    await fundAccounts(
        api,
        context,
        sudoer,
        [...proposers, ...triumvirate, ...economic, ...building].map((kp) => kp.address)
    );

    const entries: Array<{ collective: Collective; account: KeyringPair }> = [
        ...proposers.map((account) => ({ collective: "Proposers" as Collective, account })),
        ...triumvirate.map((account) => ({ collective: "Triumvirate" as Collective, account })),
        ...economic.map((account) => ({ collective: "Economic" as Collective, account })),
        ...building.map((account) => ({ collective: "Building" as Collective, account })),
    ];

    await addMembers(api, context, sudoer, entries);

    return { proposer: proposers[0], proposers, triumvirate, economic, building };
}

/** Submit `inner` on `track` as `proposer`. Returns the assigned index. */
export async function submitOnTrack(
    api: ApiPromise,
    context: DevModeContext,
    proposer: KeyringPair,
    track: number,
    inner: SudoExtrinsic
): Promise<number> {
    const index = await referendumCount(api);
    await inBlock(context, proposer, api.tx.referenda.submit(track, inner));
    return index;
}

export async function castVote(
    api: ApiPromise,
    context: DevModeContext,
    voter: KeyringPair,
    pollIndex: number,
    approve: boolean
): Promise<void> {
    await inBlock(context, voter, api.tx.signedVoting.vote(pollIndex, approve));
}

export async function removeVote(
    api: ApiPromise,
    context: DevModeContext,
    voter: KeyringPair,
    pollIndex: number
): Promise<void> {
    await inBlock(context, voter, api.tx.signedVoting.removeVote(pollIndex));
}

export async function killReferendum(
    api: ApiPromise,
    context: DevModeContext,
    sudoer: KeyringPair,
    index: number
): Promise<void> {
    await sudoInBlock(api, context, sudoer, api.tx.referenda.kill(index));
}

/** Seal `count` empty blocks so the scheduler can fire pending alarms/tasks. */
export async function nudge(context: DevModeContext, count = 1): Promise<void> {
    for (let i = 0; i < count; i++) {
        await context.createBlock([]);
    }
}

type RawDispatchError = {
    isModule: boolean;
    asModule: Parameters<ApiPromise["registry"]["findMetaError"]>[0];
    type?: string;
    toString(): string;
};

function decodeDispatchError(api: ApiPromise, dispatchError: RawDispatchError): DispatchFailure {
    if (dispatchError.isModule) {
        const decoded = api.registry.findMetaError(dispatchError.asModule);
        return { section: decoded.section, name: decoded.name };
    }
    return { kind: dispatchError.type ?? "other", raw: dispatchError.toString() };
}

export async function systemEvents(api: ApiPromise): Promise<EventRecordLike[]> {
    return (await api.query.system.events()) as unknown as EventRecordLike[];
}

export async function referendumCount(api: ApiPromise): Promise<number> {
    return ((await api.query.referenda.referendumCount()) as unknown as NumberCodecLike).toNumber();
}

export async function referendumStatusFor(api: ApiPromise, index: number): Promise<OptionCodecLike> {
    return (await api.query.referenda.referendumStatusFor(index)) as unknown as OptionCodecLike;
}

export async function isReferendumStatusNone(api: ApiPromise, index: number): Promise<boolean> {
    return (await referendumStatusFor(api, index)).isNone;
}

export async function isEnactmentTaskNone(api: ApiPromise, index: number): Promise<boolean> {
    return ((await api.query.referenda.enactmentTask(index)) as unknown as OptionCodecLike).isNone;
}

export async function isVotingForNone(api: ApiPromise, index: number, address: string): Promise<boolean> {
    return ((await api.query.signedVoting.votingFor(index, address)) as unknown as OptionCodecLike).isNone;
}

export async function freeBalance(api: ApiPromise, address: string): Promise<bigint> {
    return ((await api.query.system.account(address)) as unknown as AccountInfoLike).data.free.toBigInt();
}

/**
 * Decoded summary of the most recent failure in the latest block.
 *
 * Captures both:
 *  - `system.ExtrinsicFailed` for direct signed calls, and
 *  - `sudo.Sudid { sudo_result: Err(...) }` for calls wrapped in `sudo.sudo`,
 *    where the outer extrinsic succeeds but the wrapped call returns `Err`.
 *
 * Returns `null` when the block contains neither.
 */
export async function lastModuleError(api: ApiPromise): Promise<DispatchFailure | null> {
    const events = await systemEvents(api);

    const failed = events.find((e) => e.event.section === "system" && e.event.method === "ExtrinsicFailed");
    if (failed) {
        return decodeDispatchError(api, failed.event.data[0] as unknown as RawDispatchError);
    }

    const sudid = events.find((e) => e.event.section === "sudo" && e.event.method === "Sudid");
    if (sudid) {
        const result = sudid.event.data[0] as unknown as {
            isErr: boolean;
            asErr: RawDispatchError;
        };
        if (result.isErr) {
            return decodeDispatchError(api, result.asErr);
        }
    }

    return null;
}

/** Reads the variant name of `referendumStatusFor(index)`. */
export async function getStatusKind(api: ApiPromise, index: number): Promise<ReferendumStatusKind | null> {
    const opt = await referendumStatusFor(api, index);
    if (opt.isNone) return null;
    const json = opt.toJSON() as Record<string, unknown> | string | null;
    if (!json || typeof json === "string") return null;
    const keys = Object.keys(json);
    if (keys.length === 0) return null;
    return keys[0] as ReferendumStatusKind;
}

export type Tally = { ayes: number; nays: number; total: number };

export async function getTally(api: ApiPromise, index: number): Promise<Tally | null> {
    const opt = (await api.query.signedVoting.tallyOf(index)) as unknown as OptionCodecLike;
    return opt.isNone ? null : (opt.toJSON() as Tally);
}

export async function getMembers(api: ApiPromise, collective: Collective): Promise<string[]> {
    const members = await api.query.multiCollective.members(collective);
    return (members.toJSON() as string[]) ?? [];
}

export async function getActiveCount(api: ApiPromise): Promise<number> {
    return (await api.query.referenda.activeCount()).toJSON() as number;
}

export async function getActivePerProposer(api: ApiPromise, address: string): Promise<number> {
    return (await api.query.referenda.activePerProposer(address)).toJSON() as number;
}
