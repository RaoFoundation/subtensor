import assert from "node:assert/strict";

import type { ApiPromise } from "@polkadot/api";

import { createTempLogger } from "../lib/file-log.js";
import { assertIssuanceMatch } from "../lib/issuance.js";

const logger = createTempLogger("test-issuance-snapshot.log");
logger.captureConsole();

async function main() {
  await logger.start();
  let head = 1;
  let mismatch = 0n;
  const reads: string[] = [];
  const issuance = (block: number) => 1_000_000_000n + BigInt(block) * 500_000_000n;
  const codec = (value: bigint) => ({ toBigInt: () => value, toString: () => String(value) });
  const query = (mirror: boolean) => Object.assign(
    async () => codec(issuance(head++) + (mirror ? mismatch : 0n)),
    {
      at: async (hash: string) => {
        reads.push(hash);
        head++; // A block arrives between every read.
        return codec(issuance(Number(hash)) + (mirror ? mismatch : 0n));
      },
    },
  );
  const fake = {
    rpc: { chain: { getFinalizedHead: async () => ({ toHex: () => String(head) }) } },
    query: {
      balances: { totalIssuance: query(false) },
      subtensorModule: { totalIssuance: query(true) },
    },
  };
  const api = fake as unknown as ApiPromise;

  // The old unpinned reads fabricate a mismatch on a healthy chain.
  const balance = await fake.query.balances.totalIssuance();
  const mirror = await fake.query.subtensorModule.totalIssuance();
  assert.notEqual(balance.toBigInt(), mirror.toBigInt());

  const pinnedHead = String(head);
  await assertIssuanceMatch(api, "advancing head");
  assert.deepEqual(reads.splice(0), [pinnedHead, pinnedHead]);

  await assertIssuanceMatch(api, "explicit inclusion block", "1");
  assert.deepEqual(reads.splice(0), ["1", "1"]);

  // Pinning must not hide a real difference, even a single rao.
  for (const delta of [-1n, 1n]) {
    mismatch = delta;
    await assert.rejects(assertIssuanceMatch(api, "real mismatch", "1"), {
      code: "ERR_ASSERTION",
    });
    assert.deepEqual(reads.splice(0), ["1", "1"]);
  }
  console.log("issuance snapshot regressions: ok");
  await logger.flush();
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
