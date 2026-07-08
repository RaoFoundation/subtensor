import assert from "node:assert/strict";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "wss://test.finney.opentensor.ai:443";
const EXPECTED_RUNTIME = Number(process.env.EXPECTED_RUNTIME ?? 420);
const STAKE_TAO_AMOUNT = BigInt(process.env.TESTNET_BALANCER_SLIPPAGE_STAKE_TAO ?? "1000000000");
const RAO_PER_TAO = 1_000_000_000n;
const PERQUINTILL = 1_000_000_000_000_000_000n;
const COHORT_SIZE = 5;
const SLIPPAGE_FLOOR_PPM = 1n;

const logger = createTempLogger("test-balancer-testnet-liquidity-slippage.log");
logger.captureConsole();

let api;

async function main() {
  await logger.start();
  api = await connectApi(WS_ENDPOINT, { log: console.log });

  try {
    const chain = await api.rpc.system.chain();
    const runtimeVersion = await api.rpc.state.getRuntimeVersion();
    const header = await api.rpc.chain.getHeader();
    const specVersion = runtimeVersion.specVersion.toNumber();
    console.log("chain:", chain.toString());
    console.log("runtime:", runtimeVersion.specName.toString(), specVersion.toString());
    console.log("block:", header.number.toString());
    console.log("stake tao amount:", STAKE_TAO_AMOUNT.toString());

    assert.equal(specVersion, EXPECTED_RUNTIME, `expected runtime ${EXPECTED_RUNTIME}, got ${specVersion}`);
    assertMetadataAvailable();

    const pools = await loadInitializedPools();
    assert.ok(
      pools.length >= COHORT_SIZE * 2,
      `expected at least ${COHORT_SIZE * 2} initialized non-zero pools, got ${pools.length}`
    );

    const sortedByLiquidity = pools.sort((a, b) => (a.tao < b.tao ? -1 : a.tao > b.tao ? 1 : a.netuid - b.netuid));
    const lowLiquidity = sortedByLiquidity.slice(0, COHORT_SIZE);
    const highLiquidity = sortedByLiquidity.slice(-COHORT_SIZE).reverse();

    console.log("low liquidity netuids:", formatPoolList(lowLiquidity));
    console.log("high liquidity netuids:", formatPoolList(highLiquidity));

    const lowResults = [];
    const highResults = [];
    for (const pool of lowLiquidity) {
      lowResults.push(await measurePoolSlippage(pool, "low"));
    }
    for (const pool of highLiquidity) {
      highResults.push(await measurePoolSlippage(pool, "high"));
    }

    const lowStakeAverage = averagePpm(lowResults.map((result) => result.stakeSlippagePpm));
    const highStakeAverage = averagePpm(highResults.map((result) => result.stakeSlippagePpm));
    const lowUnstakeAverage = averagePpm(lowResults.map((result) => result.unstakeSlippagePpm));
    const highUnstakeAverage = averagePpm(highResults.map((result) => result.unstakeSlippagePpm));

    assert.ok(
      lowStakeAverage > highStakeAverage,
      `expected lower SubnetTAO pools to have higher staking slippage: low=${lowStakeAverage}, high=${highStakeAverage}`
    );
    assert.ok(
      lowUnstakeAverage > highUnstakeAverage,
      `expected lower SubnetTAO pools to have higher unstaking slippage: low=${lowUnstakeAverage}, high=${highUnstakeAverage}`
    );

    console.log(
      "testnet balancer liquidity slippage: ok",
      `stake_slippage_ppm low_avg=${lowStakeAverage} high_avg=${highStakeAverage}`,
      `unstake_slippage_ppm low_avg=${lowUnstakeAverage} high_avg=${highUnstakeAverage}`
    );
  } finally {
    await api?.disconnect();
    await logger.flush();
  }
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});

function assertMetadataAvailable() {
  const missing = [
    ["Swap.SwapBalancer", api.query.swap?.swapBalancer],
    ["Swap.PalSwapInitialized", api.query.swap?.palSwapInitialized],
    ["SubtensorModule.SubnetTAO", api.query.subtensorModule?.subnetTAO],
    ["SubtensorModule.SubnetAlphaIn", api.query.subtensorModule?.subnetAlphaIn],
  ].filter(([, value]) => !value);

  assert.equal(missing.length, 0, `${missing.map(([name]) => name).join(", ")} unavailable`);
}

async function loadInitializedPools() {
  const initializedEntries = await api.query.swap.palSwapInitialized.entries();
  const pools = [];

  for (const [key, initialized] of initializedEntries) {
    if (!initialized.isTrue) continue;

    const netuid = key.args[0].toNumber();
    if (netuid === 0) continue;

    const [tao, alpha, balancer] = await Promise.all([
      api.query.subtensorModule.subnetTAO(netuid),
      api.query.subtensorModule.subnetAlphaIn(netuid),
      api.query.swap.swapBalancer(netuid),
    ]);
    const pool = {
      netuid,
      tao: tao.toBigInt(),
      alpha: alpha.toBigInt(),
      weight: extractQuotePerquintill(balancer),
    };
    if (pool.tao === 0n || pool.alpha === 0n || pool.weight === null) continue;

    pool.price = weightedBalancerPrice(pool);
    if (pool.price === 0n) continue;
    pools.push(pool);
  }

  return pools;
}

async function measurePoolSlippage(pool, cohort) {
  const stakeEstimateAlpha = (STAKE_TAO_AMOUNT * RAO_PER_TAO) / pool.price;
  assert.ok(stakeEstimateAlpha > 0n, `netuid ${pool.netuid} stake estimate rounded to zero`);

  const stakeSim = await simSwapTaoForAlpha(pool.netuid, STAKE_TAO_AMOUNT);
  assert.ok(stakeSim.alphaAmount > 0n, `netuid ${pool.netuid} stake simulation returned zero alpha`);
  assert.ok(
    stakeSim.alphaAmount < stakeEstimateAlpha,
    `netuid ${pool.netuid} stake output should be below spot estimate: actual=${stakeSim.alphaAmount}, estimate=${stakeEstimateAlpha}`
  );
  const stakeSlippagePpm = relativeShortfallPpm(stakeEstimateAlpha, stakeSim.alphaAmount);
  assert.ok(
    stakeSlippagePpm >= SLIPPAGE_FLOOR_PPM,
    `netuid ${pool.netuid} stake slippage too small to measure: ${stakeSlippagePpm} ppm`
  );

  const unstakeEstimateTao = (stakeEstimateAlpha * pool.price) / RAO_PER_TAO;
  assert.ok(unstakeEstimateTao > 0n, `netuid ${pool.netuid} unstake estimate rounded to zero`);

  const unstakeSim = await simSwapAlphaForTao(pool.netuid, stakeEstimateAlpha);
  assert.ok(unstakeSim.taoAmount > 0n, `netuid ${pool.netuid} unstake simulation returned zero TAO`);
  assert.ok(
    unstakeSim.taoAmount < unstakeEstimateTao,
    `netuid ${pool.netuid} unstake output should be below spot estimate: actual=${unstakeSim.taoAmount}, estimate=${unstakeEstimateTao}`
  );
  const unstakeSlippagePpm = relativeShortfallPpm(unstakeEstimateTao, unstakeSim.taoAmount);
  assert.ok(
    unstakeSlippagePpm >= SLIPPAGE_FLOOR_PPM,
    `netuid ${pool.netuid} unstake slippage too small to measure: ${unstakeSlippagePpm} ppm`
  );

  console.log(
    "pool slippage:",
    `cohort=${cohort}`,
    `netuid=${pool.netuid}`,
    `subnet_tao=${pool.tao}`,
    `subnet_alpha=${pool.alpha}`,
    `price=${pool.price}`,
    `stake_estimate_alpha=${stakeEstimateAlpha}`,
    `stake_actual_alpha=${stakeSim.alphaAmount}`,
    `stake_slippage_ppm=${stakeSlippagePpm}`,
    `unstake_estimate_tao=${unstakeEstimateTao}`,
    `unstake_actual_tao=${unstakeSim.taoAmount}`,
    `unstake_slippage_ppm=${unstakeSlippagePpm}`
  );

  return {
    netuid: pool.netuid,
    stakeSlippagePpm,
    unstakeSlippagePpm,
  };
}

async function simSwapTaoForAlpha(netuid, amount) {
  const value = await api._rpcCore.provider.send("swap_simSwapTaoForAlpha", [netuid, Number(amount), null]);
  return normalizeSimSwap(value, "taoForAlpha");
}

async function simSwapAlphaForTao(netuid, amount) {
  assert.ok(amount <= BigInt(Number.MAX_SAFE_INTEGER), `alpha amount ${amount} exceeds safe RPC number range`);
  const value = await api._rpcCore.provider.send("swap_simSwapAlphaForTao", [netuid, Number(amount), null]);
  return normalizeSimSwap(value, "alphaForTao");
}

function normalizeSimSwap(value, direction) {
  const decoded = Array.isArray(value) ? decodeSimSwapBytes(value) : normalizeRpcObject(value);
  if (decoded.tao_amount !== undefined || decoded.taoAmount !== undefined) {
    return {
      taoAmount: readBigInt(decoded, ["tao_amount", "taoAmount"]),
      alphaAmount: readBigInt(decoded, ["alpha_amount", "alphaAmount"]),
      taoFee: readBigInt(decoded, ["tao_fee", "taoFee"]),
      alphaFee: readBigInt(decoded, ["alpha_fee", "alphaFee"]),
      taoSlippage: readBigInt(decoded, ["tao_slippage", "taoSlippage"]),
      alphaSlippage: readBigInt(decoded, ["alpha_slippage", "alphaSlippage"]),
    };
  }

  const amountPaidIn = readBigInt(decoded, ["amount_paid_in", "amountPaidIn"]);
  const amountPaidOut = readBigInt(decoded, ["amount_paid_out", "amountPaidOut"]);
  const feePaid = readBigInt(decoded, ["fee_paid", "feePaid"]);
  return direction === "taoForAlpha"
    ? { taoAmount: amountPaidIn, alphaAmount: amountPaidOut, taoFee: feePaid, alphaFee: 0n, taoSlippage: 0n, alphaSlippage: 0n }
    : { taoAmount: amountPaidOut, alphaAmount: amountPaidIn, taoFee: 0n, alphaFee: feePaid, taoSlippage: 0n, alphaSlippage: 0n };
}

function normalizeRpcObject(value) {
  if (value && typeof value.toJSON === "function") {
    return value.toJSON();
  }
  if (value && typeof value === "object") {
    return value;
  }
  return { value: value?.toString?.() ?? String(value) };
}

function decodeSimSwapBytes(bytes) {
  assert.ok(bytes.length === 48 || bytes.length >= 32, `sim swap result was ${bytes.length} bytes`);
  if (bytes.length >= 48) {
    return {
      tao_amount: readLittleEndianU64(bytes, 0),
      alpha_amount: readLittleEndianU64(bytes, 8),
      tao_fee: readLittleEndianU64(bytes, 16),
      alpha_fee: readLittleEndianU64(bytes, 24),
      tao_slippage: readLittleEndianU64(bytes, 32),
      alpha_slippage: readLittleEndianU64(bytes, 40),
    };
  }

  return {
    amountPaidIn: readLittleEndianU64(bytes, 0),
    amountPaidOut: readLittleEndianU64(bytes, 8),
    feePaid: readLittleEndianU64(bytes, 16),
    feeToBlockAuthor: readLittleEndianU64(bytes, 24),
  };
}

function readBigInt(object, names) {
  for (const name of names) {
    if (object[name] !== undefined) {
      return BigInt(object[name].toString());
    }
  }
  return 0n;
}

function readLittleEndianU64(bytes, offset) {
  let value = 0n;
  for (let index = 7; index >= 0; index--) {
    value = (value << 8n) + BigInt(bytes[offset + index]);
  }
  return value;
}

function weightedBalancerPrice({ tao, alpha, weight }) {
  const baseWeight = PERQUINTILL - weight;
  return (baseWeight * tao * RAO_PER_TAO) / (weight * alpha);
}

function extractQuotePerquintill(value) {
  if (value.quote?.toBigInt) {
    return value.quote.toBigInt();
  }

  const json = value.toJSON();
  if (json && typeof json === "object") {
    const quote = json.quote ?? json.Quote;
    if (typeof quote === "number" || typeof quote === "string") {
      return BigInt(quote);
    }
  }

  const human = value.toHuman();
  if (human && typeof human === "object") {
    const quote = human.quote ?? human.Quote;
    if (typeof quote === "string") {
      return quote.endsWith("%")
        ? BigInt(Math.round(Number(quote.slice(0, -1)) * 10_000_000_000_000_000))
        : BigInt(quote.replaceAll(",", ""));
    }
  }

  return null;
}

function relativeShortfallPpm(expected, actual) {
  assert.ok(expected > actual, `expected ${expected} to be greater than actual ${actual}`);
  return ((expected - actual) * 1_000_000n) / expected;
}

function averagePpm(values) {
  assert.ok(values.length > 0, "cannot average empty slippage set");
  return values.reduce((sum, value) => sum + value, 0n) / BigInt(values.length);
}

function formatPoolList(pools) {
  return pools.map((pool) => `${pool.netuid}:tao=${pool.tao},price=${pool.price}`).join("; ");
}
