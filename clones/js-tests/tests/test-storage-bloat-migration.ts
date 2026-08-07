import assert from "node:assert/strict";

import { xxhashAsHex } from "@polkadot/util-crypto";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const TARGET_SPEC_VERSION = BigInt(process.env.TARGET_SPEC_VERSION ?? 444);
const PAGE_SIZE = Number(process.env.STORAGE_MIGRATION_PAGE_SIZE ?? 1000);
const MAX_HEAD_GAP_MS = Number(process.env.MAX_HEAD_GAP_MS ?? 30_000);
const MIGRATION_TIMEOUT_MS = Number(process.env.MIGRATION_TIMEOUT_MS ?? 60 * 60 * 1000);
const MIGRATION_NAME = "migrate_storage_bloat_v2";
const logger = createTempLogger("storage-bloat-migration.log");

const SUBTENSOR_CLEAR = [
  "TotalHotkeyStake",
  "PendingdHotkeyEmission",
  "PendingdHotkeyEmissionUntouchable",
  "LastHotkeyEmissionDrain",
  "StakeDeltaSinceLastEmissionDrain",
  "TotalColdkeyStake",
  "LastAddStakeIncrease",
  "ColdkeyArbitrationBlock",
] as const;

const SUBTENSOR_ZERO = [
  "Alpha",
  "TotalHotkeyShares",
  "TotalHotkeyAlpha",
  "TotalHotkeyAlphaLastEpoch",
  "StakingHotkeys",
] as const;

const SUBTENSOR_ROOT_AGE = "LastColdkeyHotkeyStakeBlock";

const SWAP_CLEAR = [
  "AlphaSqrtPrice",
  "CurrentTick",
  "EnabledUserLiquidity",
  "FeeGlobalTao",
  "FeeGlobalAlpha",
  "LastPositionId",
  "ScrapReservoirTao",
  "ScrapReservoirAlpha",
  "Ticks",
  "TickIndexBitmapWords",
  "SwapV3Initialized",
  "CurrentLiquidity",
  "Positions",
] as const;

const SWAP_ZERO = ["BalancerTaoReservoir", "BalancerAlphaReservoir"] as const;

type PrefixStats = {
  count: number;
  zero: number;
  nonzero: number;
};

type HeadSample = {
  number: bigint;
  arrivalMs: number;
};

function prefix(pallet: string, storage: string): string {
  return `${xxhashAsHex(pallet, 128)}${xxhashAsHex(storage, 128).slice(2)}`;
}

function isAllZero(valueHex: string): boolean {
  return valueHex.length > 2 && /^0x0+$/.test(valueHex);
}

function percentile(values: number[], fraction: number): number {
  assert.ok(values.length > 0, "cannot calculate a percentile without samples");
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))];
}

function gaps(samples: HeadSample[]): number[] {
  return samples.slice(1).map((sample, index) => sample.arrivalMs - samples[index].arrivalMs);
}

async function scanPrefix(
  api: any,
  pallet: string,
  storage: string,
  atHash: any,
  checkHalt: () => void,
): Promise<PrefixStats> {
  const storagePrefix = prefix(pallet, storage);
  let startKey: any = undefined;
  let count = 0;
  let zero = 0;
  let pages = 0;

  for (;;) {
    checkHalt();
    const keys: any[] = await api.rpc.state.getKeysPaged(
      storagePrefix,
      PAGE_SIZE,
      startKey,
      atHash,
    );
    if (keys.length === 0) break;

    const changeSets: any = await api.rpc.state.queryStorageAt(keys, atHash);
    const values = new Map<string, string>();
    if (
      Array.isArray(changeSets) &&
      changeSets.length === keys.length &&
      changeSets.every((value) => typeof value?.isSome === "boolean")
    ) {
      for (let index = 0; index < keys.length; index += 1) {
        const maybeValue = changeSets[index];
        values.set(
          keys[index].toHex().toLowerCase(),
          maybeValue.isSome ? maybeValue.unwrap().toHex() : "0x",
        );
      }
    } else {
      const decodedSets = changeSets.changes ? [changeSets] : Array.from(changeSets);
      for (const changeSet of decodedSets as any[]) {
        const changes =
          typeof changeSet.changes === "function" ? changeSet.changes() : changeSet.changes;
        for (const [key, maybeValue] of changes) {
          values.set(
            key.toHex().toLowerCase(),
            maybeValue.isSome ? maybeValue.unwrap().toHex() : "0x",
          );
        }
      }
    }

    for (const key of keys) {
      if (isAllZero(values.get(key.toHex().toLowerCase()) ?? "0x")) zero += 1;
    }
    count += keys.length;
    pages += 1;
    startKey = keys.at(-1);
    if (pages === 1 || pages % 200 === 0) {
      await logger.info(
        `scan pallet=${pallet} storage=${storage} pages=${pages} keys=${count} zero=${zero}`,
      );
    }
  }

  const result = { count, zero, nonzero: count - zero };
  await logger.info(`PREFIX ${pallet}.${storage} ${JSON.stringify(result)}`);
  return result;
}

async function snapshot(
  api: any,
  atHash: any,
  checkHalt: () => void,
): Promise<Map<string, PrefixStats>> {
  const result = new Map<string, PrefixStats>();
  for (const name of SUBTENSOR_CLEAR) {
    result.set(
      `SubtensorModule.${name}`,
      await scanPrefix(api, "SubtensorModule", name, atHash, checkHalt),
    );
  }
  for (const name of SUBTENSOR_ZERO) {
    result.set(
      `SubtensorModule.${name}`,
      await scanPrefix(api, "SubtensorModule", name, atHash, checkHalt),
    );
  }
  result.set(
    `SubtensorModule.${SUBTENSOR_ROOT_AGE}`,
    await scanPrefix(api, "SubtensorModule", SUBTENSOR_ROOT_AGE, atHash, checkHalt),
  );
  for (const name of SWAP_CLEAR) {
    result.set(`Swap.${name}`, await scanPrefix(api, "Swap", name, atHash, checkHalt));
  }
  for (const name of SWAP_ZERO) {
    result.set(`Swap.${name}`, await scanPrefix(api, "Swap", name, atHash, checkHalt));
  }
  return result;
}

function assertCleanup(before: Map<string, PrefixStats>, after: Map<string, PrefixStats>): void {
  for (const name of SUBTENSOR_CLEAR) {
    const key = `SubtensorModule.${name}`;
    assert.ok((before.get(key)?.count ?? 0) > 0, `${key} had no mainnet rows before migration`);
    assert.equal(after.get(key)?.count, 0, `${key} was not fully removed`);
  }

  for (const name of SUBTENSOR_ZERO) {
    const key = `SubtensorModule.${name}`;
    const pre = before.get(key)!;
    const post = after.get(key)!;
    assert.ok(pre.zero > 0, `${key} had no zero rows to exercise the migration`);
    assert.ok(pre.nonzero > 0, `${key} had no nonzero rows to preserve`);
    assert.equal(post.zero, 0, `${key} retained explicit zero rows`);
    assert.ok(post.nonzero > 0, `${key} lost all nonzero rows`);
  }

  const rootAgeKey = `SubtensorModule.${SUBTENSOR_ROOT_AGE}`;
  assert.ok((before.get(rootAgeKey)?.count ?? 0) > 0, `${rootAgeKey} had no rows before migration`);
  assert.deepEqual(after.get(rootAgeKey), before.get(rootAgeKey), `${rootAgeKey} changed`);

  for (const name of SWAP_CLEAR) {
    const key = `Swap.${name}`;
    assert.ok((before.get(key)?.count ?? 0) > 0, `${key} had no mainnet rows before migration`);
    assert.equal(after.get(key)?.count, 0, `${key} was not fully removed`);
  }

  for (const name of SWAP_ZERO) {
    const key = `Swap.${name}`;
    assert.ok((before.get(key)?.zero ?? 0) > 0, `${key} had no zero rows before migration`);
    assert.equal(after.get(key)?.zero, 0, `${key} retained explicit zero rows`);
  }
}

async function main(): Promise<void> {
  await logger.start();
  const api = await connectApi(WS_ENDPOINT, { log: (...args) => logger.info(...args) });
  const samples: HeadSample[] = [];
  let lastHeadAt = Date.now();
  let lastLoggedHead = -1n;

  const unsubscribe = await api.rpc.chain.subscribeNewHeads((header: any) => {
    const arrivalMs = Date.now();
    const number = BigInt(header.number.toString());
    samples.push({ number, arrivalMs });
    lastHeadAt = arrivalMs;
  });

  const checkHalt = (): void => {
    const gap = Date.now() - lastHeadAt;
    assert.ok(gap <= MAX_HEAD_GAP_MS, `block production halted for ${gap}ms`);
  };

  try {
    const initialRuntime = await api.rpc.state.getRuntimeVersion();
    assert.ok(
      BigInt(initialRuntime.specVersion.toString()) < TARGET_SPEC_VERSION,
      `expected a pre-upgrade runtime, got spec ${initialRuntime.specVersion.toString()}`,
    );
    const beforeHeader = await api.rpc.chain.getHeader();
    const beforeHash = beforeHeader.hash;
    const rootUnlockInterval = BigInt(
      (await (api.query.subtensorModule as any).rootStakeUnlockInterval.at(beforeHash)).toString(),
    );
    assert.equal(rootUnlockInterval, 0n, "root stake hold is enabled; root-age cleanup is unsafe");

    await logger.info(
      `baseline block=${beforeHeader.number.toString()} hash=${beforeHash.toString()} spec=${initialRuntime.specVersion.toString()}`,
    );
    const before = await snapshot(api, beforeHash, checkHalt);
    await logger.info("READY_FOR_RUNTIME_UPGRADE");

    const startedAt = Date.now();
    let upgradeDetectedAt = 0;
    let upgradeBlock = 0n;
    let completionBlock = 0n;
    for (;;) {
      checkHalt();
      assert.ok(Date.now() - startedAt <= MIGRATION_TIMEOUT_MS, "migration timed out");

      const header = await api.rpc.chain.getHeader();
      const block = BigInt(header.number.toString());
      const runtime = await api.rpc.state.getRuntimeVersion();
      const spec = BigInt(runtime.specVersion.toString());
      if (spec >= TARGET_SPEC_VERSION && upgradeDetectedAt === 0) {
        upgradeDetectedAt = Date.now();
        upgradeBlock = block;
        await logger.info(`UPGRADE_DETECTED block=${block} spec=${spec}`);
      }

      if (upgradeDetectedAt !== 0) {
        const marker = await (api.query.subtensorModule as any).hasMigrationRun(MIGRATION_NAME);
        const progress: any = await api.rpc.state.getStorage(
          prefix("SubtensorModule", "StorageBloatCleanupMigration"),
        );
        if (block >= lastLoggedHead + 10n || marker.isTrue) {
          await logger.info(
            `progress block=${block} marker=${marker.toString()} cursor_present=${progress.isSome}`,
          );
          lastLoggedHead = block;
        }
        if (marker.isTrue && progress.isNone) {
          completionBlock = block;
          break;
        }
      }

      await new Promise((resolve) => setTimeout(resolve, 2_000));
    }

    while ((samples.at(-1)?.number ?? 0n) < completionBlock + 10n) {
      checkHalt();
      await new Promise((resolve) => setTimeout(resolve, 1_000));
    }

    const afterHeader = await api.rpc.chain.getHeader();
    const afterHash = afterHeader.hash;
    const after = await snapshot(api, afterHash, checkHalt);
    assertCleanup(before, after);

    const baselineSamples = samples.filter((sample) => sample.arrivalMs < upgradeDetectedAt);
    const migrationSamples = samples.filter(
      (sample) => sample.number >= upgradeBlock && sample.number <= completionBlock + 10n,
    );
    const baselineGaps = gaps(baselineSamples);
    const migrationGaps = gaps(migrationSamples);
    assert.ok(baselineGaps.length >= 5, "not enough baseline block-time samples");
    assert.ok(migrationGaps.length >= 10, "not enough migration block-time samples");
    const baselineMedian = percentile(baselineGaps, 0.5);
    const migrationMedian = percentile(migrationGaps, 0.5);
    const migrationP95 = percentile(migrationGaps, 0.95);
    const migrationMax = Math.max(...migrationGaps);
    assert.ok(migrationMax <= MAX_HEAD_GAP_MS, `maximum migration head gap was ${migrationMax}ms`);
    assert.ok(
      migrationMedian <= Math.max(6_000, baselineMedian * 3),
      `median head gap regressed from ${baselineMedian}ms to ${migrationMedian}ms`,
    );

    await logger.info(
      `TIMING baseline_median_ms=${baselineMedian} migration_median_ms=${migrationMedian} migration_p95_ms=${migrationP95} migration_max_ms=${migrationMax}`,
    );
    await logger.info(
      `PASS upgrade_block=${upgradeBlock} completion_block=${completionBlock} migration_blocks=${completionBlock - upgradeBlock + 1n} final_block=${afterHeader.number.toString()}`,
    );
  } finally {
    unsubscribe();
    await api.disconnect();
    await logger.flush();
  }
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
