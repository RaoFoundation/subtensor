import type { ApiPromise } from "@polkadot/api";

export interface IssuanceMirror {
  balancesTotalIssuance: bigint;
  subtensorTotalIssuance: bigint;
}

export async function readIssuanceMirror(
  api: ApiPromise,
  hash?: Parameters<ApiPromise["at"]>[0],
): Promise<IssuanceMirror> {
  const query = hash === undefined ? api.query : (await api.at(hash)).query;
  const [balances, subtensor] = await Promise.all([
    query.balances.totalIssuance(),
    query.subtensorModule.totalIssuance(),
  ]);
  return {
    balancesTotalIssuance: BigInt(balances.toString()),
    subtensorTotalIssuance: BigInt(subtensor.toString()),
  };
}

export function assertIssuanceMirror(invariant: IssuanceMirror, label: string) {
  if (invariant.balancesTotalIssuance !== invariant.subtensorTotalIssuance) {
    throw new Error(
      `${label}: Balances.TotalIssuance ${invariant.balancesTotalIssuance} does not match ` +
        `SubtensorModule.TotalIssuance ${invariant.subtensorTotalIssuance}`,
    );
  }
}

export function serializeIssuanceMirror(invariant: IssuanceMirror) {
  return {
    balancesTotalIssuance: invariant.balancesTotalIssuance.toString(),
    subtensorTotalIssuance: invariant.subtensorTotalIssuance.toString(),
  };
}
