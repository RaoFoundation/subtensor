import assert from "node:assert/strict";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const PAGE_SIZE = Number(process.env.STAKING_HOTKEYS_PAGE_SIZE ?? 1000);
const MIGRATION_TIMEOUT_MS = Number(process.env.STAKING_HOTKEYS_MIGRATION_TIMEOUT_MS ?? 30 * 60_000);
const MIGRATION_NAME = new TextEncoder().encode("migrate_cleanup_staking_hotkeys");
const logger = createTempLogger("staking-hotkeys-cleanup-migration.log");

async function main() {
  await logger.start();
  const api = await connectApi(WS_ENDPOINT, { log: (...args) => logger.info(...args) });

  try {
    assert.ok(
      api.query.subtensorModule?.hasMigrationRun,
      "SubtensorModule.HasMigrationRun is not available",
    );
    assert.ok(api.query.subtensorModule?.stakingHotkeys, "SubtensorModule.StakingHotkeys is not available");
    assert.ok(api.query.subtensorModule?.alpha, "SubtensorModule.Alpha is not available");
    assert.ok(api.query.subtensorModule?.alphaV2, "SubtensorModule.AlphaV2 is not available");
    assert.ok(api.query.subtensorModule?.basketClaimed, "SubtensorModule.BasketClaimed is not available");

    const startHeader = await api.rpc.chain.getHeader();
    await logger.info(`migration_wait_start_block=${startHeader.number.toString()}`);
    await waitForMigration(api);

    // Freeze every paged query at one post-migration block so the audit cannot combine state
    // from different blocks while the accelerated clone continues sealing.
    const header = await api.rpc.chain.getHeader();
    const blockHash = await api.rpc.chain.getBlockHash(header.number.unwrap());
    const apiAt = await api.at(blockHash);
    await logger.info(`audit_block=${header.number.toString()}`);
    await logger.info(`audit_block_hash=${blockHash.toString()}`);

    const stakedPairs = new Set<string>();
    const alphaStats = await collectNonzeroPairs(apiAt.query.subtensorModule.alpha, stakedPairs, "Alpha");
    const alphaV2Stats = await collectNonzeroPairs(
      apiAt.query.subtensorModule.alphaV2,
      stakedPairs,
      "AlphaV2",
    );

    const basketPairs = new Set<string>();
    const basketStats = await collectNonzeroPairs(
      apiAt.query.subtensorModule.basketClaimed,
      basketPairs,
      "BasketClaimed",
    );

    let rows = 0;
    let relationships = 0;
    let relationshipsWithStake = 0;
    let basketProtectedWithoutStake = 0;
    let staleRelationships = 0;
    let emptyRows = 0;
    const staleExamples: string[] = [];

    await forEachEntry(apiAt.query.subtensorModule.stakingHotkeys, "StakingHotkeys", async ([key, value]) => {
      const coldkey = key.args[0].toString();
      const hotkeys = value as unknown as Iterable<{ toString(): string }>;
      let rowRelationships = 0;
      rows += 1;

      for (const hotkeyCodec of hotkeys) {
        const hotkey = hotkeyCodec.toString();
        const pair = pairKey(hotkey, coldkey);
        rowRelationships += 1;
        relationships += 1;

        if (stakedPairs.has(pair)) {
          relationshipsWithStake += 1;
        } else if (basketPairs.has(pair)) {
          basketProtectedWithoutStake += 1;
        } else {
          staleRelationships += 1;
          if (staleExamples.length < 20) {
            staleExamples.push(`${coldkey}|${hotkey}`);
          }
        }
      }

      if (rowRelationships === 0) {
        emptyRows += 1;
      }
    });

    await logger.info(`staking_hotkeys_rows=${rows}`);
    await logger.info(`staking_hotkeys_relationships=${relationships}`);
    await logger.info(`relationships_with_nonzero_stake=${relationshipsWithStake}`);
    await logger.info(`basket_protected_without_stake=${basketProtectedWithoutStake}`);
    await logger.info(`stale_relationships_without_stake_or_basket=${staleRelationships}`);
    await logger.info(`empty_staking_hotkeys_rows=${emptyRows}`);
    await logger.info(`stale_examples=${JSON.stringify(staleExamples)}`);
    await logger.info(`alpha_rows=${alphaStats.rows} alpha_zero_rows=${alphaStats.zeroRows}`);
    await logger.info(`alpha_v2_rows=${alphaV2Stats.rows} alpha_v2_zero_rows=${alphaV2Stats.zeroRows}`);
    await logger.info(
      `basket_claimed_rows=${basketStats.rows} basket_claimed_zero_rows=${basketStats.zeroRows}`,
    );

    assert.equal(
      staleRelationships,
      0,
      `found ${staleRelationships} StakingHotkeys relationships with neither stake nor basket state`,
    );
    assert.equal(emptyRows, 0, `found ${emptyRows} empty StakingHotkeys rows`);
  } finally {
    await api.disconnect();
    await logger.flush();
  }
}

async function waitForMigration(api) {
  const deadline = Date.now() + MIGRATION_TIMEOUT_MS;
  let lastLoggedBlock = -1;

  while (Date.now() < deadline) {
    const [completed, header] = await Promise.all([
      api.query.subtensorModule.hasMigrationRun(MIGRATION_NAME),
      api.rpc.chain.getHeader(),
    ]);
    const block = header.number.toNumber();

    if (completed.isTrue) {
      await logger.info(`migration_completed_block=${block}`);
      return;
    }

    if (lastLoggedBlock < 0 || block - lastLoggedBlock >= 100) {
      await logger.info(`migration_pending_block=${block}`);
      lastLoggedBlock = block;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }

  throw new Error(`migration did not complete within ${MIGRATION_TIMEOUT_MS}ms`);
}

async function collectNonzeroPairs(query, destination: Set<string>, label: string) {
  let rows = 0;
  let zeroRows = 0;

  await forEachEntry(query, label, async ([key, value]) => {
    rows += 1;
    if (codecIsZero(value)) {
      zeroRows += 1;
      return;
    }

    const hotkey = key.args[0].toString();
    const coldkey = key.args[1].toString();
    destination.add(pairKey(hotkey, coldkey));
  });

  return { rows, zeroRows };
}

async function forEachEntry(query, label: string, visit: (entry: any) => Promise<void>) {
  let startKey;
  let pages = 0;
  let entriesSeen = 0;

  for (;;) {
    const entries = await query.entriesPaged({ args: [], pageSize: PAGE_SIZE, startKey });
    if (entries.length === 0) {
      break;
    }

    pages += 1;
    for (const entry of entries) {
      await visit(entry);
      entriesSeen += 1;
    }
    startKey = entries.at(-1)[0];

    if (pages === 1 || pages % 100 === 0) {
      await logger.info(`${label}_progress_pages=${pages} entries=${entriesSeen}`);
    }
  }

  await logger.info(`${label}_scan_complete_pages=${pages} entries=${entriesSeen}`);
}

function pairKey(hotkey: string, coldkey: string): string {
  return `${hotkey}|${coldkey}`;
}

function codecIsZero(codec): boolean {
  const json = codec.toJSON();
  if (json && typeof json === "object" && "mantissa" in json) {
    return BigInt(json.mantissa) === 0n;
  }
  if (typeof codec.toBigInt === "function") {
    return codec.toBigInt() === 0n;
  }
  return BigInt(codec.toString()) === 0n;
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
