import type { ApiPromise } from "@polkadot/api";
import { xxhashAsHex } from "@polkadot/util-crypto";

export interface MigrationReadinessDescriptor {
  name: string;
  cursorStorageItem: string;
  deferredStorageItems: readonly string[];
}

export const BETA_BASKET_V2_MIGRATION: MigrationReadinessDescriptor = {
  name: "migrate_seed_beta_basket_v2",
  cursorStorageItem: "SeedBetaBasketV2Migration",
  deferredStorageItems: ["DeferredRootAlphaDividends"],
};

export interface MigrationReadinessSnapshot {
  cursorExists: boolean;
  completionFlag: boolean;
  deferredEntriesExist: boolean;
}

export interface MigrationReadinessHistory {
  sawCursor: boolean;
  sawDeferredEntries: boolean;
}

export type MigrationReadinessEvaluation =
  | { kind: "not-observed"; history: MigrationReadinessHistory }
  | {
      kind: "waiting";
      stage: "migration" | "deferred-release";
      history: MigrationReadinessHistory;
    }
  | { kind: "ready"; history: MigrationReadinessHistory }
  | { kind: "invalid"; history: MigrationReadinessHistory; reason: string };

export interface MigrationReadinessObservation extends MigrationReadinessHistory {
  descriptor: string;
  mode: "not-observed" | "cursorless-complete" | "multi-block";
  startBlock: number;
  cursorCompletionBlock: number | null;
  readinessBlock: number;
  observedBlocks: number;
  observedWallMs: number;
}

export interface WaitForMigrationReadinessOptions {
  deadlineEpochMs: number;
  descriptor?: MigrationReadinessDescriptor;
  pollIntervalMs?: number;
  log?: (message: string) => void;
}

const EMPTY_HISTORY: MigrationReadinessHistory = {
  sawCursor: false,
  sawDeferredEntries: false,
};

export function evaluateMigrationReadiness(
  snapshot: MigrationReadinessSnapshot,
  previous: MigrationReadinessHistory = EMPTY_HISTORY,
): MigrationReadinessEvaluation {
  const history = {
    sawCursor: previous.sawCursor || snapshot.cursorExists,
    sawDeferredEntries: previous.sawDeferredEntries || snapshot.deferredEntriesExist,
  };

  if (snapshot.cursorExists && snapshot.completionFlag) {
    return {
      kind: "invalid",
      history,
      reason: "migration cursor and completion flag both exist",
    };
  }
  if (
    !snapshot.cursorExists &&
    !snapshot.completionFlag &&
    !snapshot.deferredEntriesExist &&
    !history.sawCursor &&
    !history.sawDeferredEntries
  ) {
    return { kind: "not-observed", history };
  }
  if (previous.sawCursor && !snapshot.cursorExists && !snapshot.completionFlag) {
    return {
      kind: "invalid",
      history,
      reason: "migration cursor disappeared without its completion flag",
    };
  }
  if (!snapshot.cursorExists && !snapshot.completionFlag && snapshot.deferredEntriesExist) {
    return {
      kind: "invalid",
      history,
      reason: "deferred migration work exists without a cursor or completion flag",
    };
  }
  if (snapshot.cursorExists) {
    return { kind: "waiting", stage: "migration", history };
  }
  if (!snapshot.completionFlag) {
    return {
      kind: "invalid",
      history,
      reason: "observed migration state has no completion flag",
    };
  }
  if (snapshot.deferredEntriesExist) {
    return { kind: "waiting", stage: "deferred-release", history };
  }
  return { kind: "ready", history };
}

export async function waitForMigrationReadiness(
  api: ApiPromise,
  options: WaitForMigrationReadinessOptions,
): Promise<MigrationReadinessObservation> {
  const descriptor = options.descriptor ?? BETA_BASKET_V2_MIGRATION;
  const pollIntervalMs = options.pollIntervalMs ?? 1_000;
  const log = options.log ?? (() => undefined);
  if (!Number.isSafeInteger(options.deadlineEpochMs) || options.deadlineEpochMs <= Date.now()) {
    throw new Error(`invalid migration readiness deadline: ${options.deadlineEpochMs}`);
  }
  if (!Number.isSafeInteger(pollIntervalMs) || pollIntervalMs < 1) {
    throw new Error(`invalid migration readiness poll interval: ${pollIntervalMs}`);
  }

  const startedAtEpochMs = Date.now();
  const start = await bestHeader(api);
  let header = start;
  let lastBlock = start.block;
  let lastReportedBlock = start.block - 50;
  let history = EMPTY_HISTORY;
  let cursorCompletionBlock: number | null = null;

  for (;;) {
    ensureDeadline(options.deadlineEpochMs, descriptor.name);
    if (header.block < lastBlock) {
      throw new Error(`best block regressed from ${lastBlock} to ${header.block}`);
    }
    lastBlock = header.block;

    const snapshot = await readMigrationSnapshot(api, descriptor, header.hash);
    const evaluation = evaluateMigrationReadiness(snapshot, history);
    history = evaluation.history;
    if (!snapshot.cursorExists && snapshot.completionFlag && cursorCompletionBlock === null) {
      cursorCompletionBlock = header.block;
    }

    if (evaluation.kind === "invalid") {
      throw new Error(`${descriptor.name}: ${evaluation.reason} at block ${header.block}`);
    }
    if (evaluation.kind === "not-observed") {
      log(
        `No ${descriptor.name} cursor, completion marker, or deferred work observed after ` +
          "the runtime upgrade; continuing immediately.",
      );
      return observation(
        descriptor,
        "not-observed",
        start.block,
        header.block,
        cursorCompletionBlock,
        history,
        startedAtEpochMs,
      );
    }
    if (evaluation.kind === "ready") {
      const mode = history.sawCursor ? "multi-block" : "cursorless-complete";
      log(
        `${descriptor.name} ready at block ${header.block}; mode=${mode} ` +
          `cursor_completion=${cursorCompletionBlock ?? "not-observed"} ` +
          `deferred_release_observed=${history.sawDeferredEntries}`,
      );
      return observation(
        descriptor,
        mode,
        start.block,
        header.block,
        cursorCompletionBlock,
        history,
        startedAtEpochMs,
      );
    }
    if (header.block >= lastReportedBlock + 50 || header.block === start.block) {
      lastReportedBlock = header.block;
      log(
        `Waiting for ${descriptor.name}: block=${header.block} stage=${evaluation.stage} ` +
          `cursor=${snapshot.cursorExists} completed=${snapshot.completionFlag} ` +
          `deferred=${snapshot.deferredEntriesExist}`,
      );
    }
    await delay(pollIntervalMs);
    header = await bestHeader(api);
  }
}

async function readMigrationSnapshot(
  api: ApiPromise,
  descriptor: MigrationReadinessDescriptor,
  hash: string,
): Promise<MigrationReadinessSnapshot> {
  const [cursorExists, completionFlag, deferredEntries] = await Promise.all([
    storageValueExistsAt(api, storagePrefix(descriptor.cursorStorageItem), hash),
    hasMigrationRunAt(api, descriptor.name, hash),
    Promise.all(
      descriptor.deferredStorageItems.map((item) =>
        storageEntriesExistAt(api, storagePrefix(item), hash),
      ),
    ),
  ]);
  return {
    cursorExists,
    completionFlag,
    deferredEntriesExist: deferredEntries.some(Boolean),
  };
}

async function hasMigrationRunAt(api: ApiPromise, name: string, hash: string): Promise<boolean> {
  const at = await api.at(hash);
  const value = await at.query.subtensorModule.hasMigrationRun([...Buffer.from(name)]);
  return value.toString() === "true";
}

async function storageValueExistsAt(api: ApiPromise, key: string, hash: string): Promise<boolean> {
  const value = await api.rpc.state.getStorage(key, hash);
  if (value === null || typeof value !== "object" || !("isSome" in value)) {
    throw new Error(`unexpected storage response for ${key}`);
  }
  return value.isSome === true;
}

async function storageEntriesExistAt(
  api: ApiPromise,
  prefix: string,
  hash: string,
): Promise<boolean> {
  const keys = await api.rpc.state.getKeysPaged(prefix, 1, prefix, hash);
  return keys.length > 0;
}

async function bestHeader(api: ApiPromise): Promise<{ block: number; hash: string }> {
  const header = await api.rpc.chain.getHeader();
  return { block: header.number.toNumber(), hash: header.hash.toHex() };
}

function observation(
  descriptor: MigrationReadinessDescriptor,
  mode: MigrationReadinessObservation["mode"],
  startBlock: number,
  readinessBlock: number,
  cursorCompletionBlock: number | null,
  history: MigrationReadinessHistory,
  startedAtEpochMs: number,
): MigrationReadinessObservation {
  return {
    descriptor: descriptor.name,
    mode,
    ...history,
    startBlock,
    cursorCompletionBlock,
    readinessBlock,
    observedBlocks: Math.max(0, readinessBlock - startBlock),
    observedWallMs: Date.now() - startedAtEpochMs,
  };
}

function storagePrefix(item: string): string {
  return `${xxhashAsHex("SubtensorModule", 128)}${xxhashAsHex(item, 128).slice(2)}`;
}

function ensureDeadline(deadlineEpochMs: number, migrationName: string) {
  if (Date.now() >= deadlineEpochMs) {
    throw new Error(
      `${migrationName} readiness deadline ${new Date(deadlineEpochMs).toISOString()} was reached`,
    );
  }
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
