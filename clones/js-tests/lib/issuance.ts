import assert from "node:assert/strict";

import type { ApiPromise } from "@polkadot/api";

export async function assertIssuanceMatch(api: ApiPromise, label: string, blockHash?: string) {
  // A moving head can advance between storage RPCs, even in Promise.all.
  // Never "repair" a mismatch by writing an issuance value from an older block.
  const hash = blockHash ?? (await api.rpc.chain.getFinalizedHead()).toHex();
  const [balancesIssuance, subtensorIssuance] = await Promise.all([
    api.query.balances.totalIssuance.at(hash),
    api.query.subtensorModule.totalIssuance.at(hash),
  ]);
  const balances = BigInt(balancesIssuance.toString());
  const subtensor = BigInt(subtensorIssuance.toString());
  assert.equal(
    subtensor,
    balances,
    `${label} at ${hash}: SubtensorModule.TotalIssuance ${subtensor} does not match Balances.TotalIssuance ${balances}`,
  );
  console.log(`${label}: issuance match at ${hash}`, balances.toString());
}
