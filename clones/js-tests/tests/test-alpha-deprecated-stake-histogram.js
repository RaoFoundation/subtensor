import assert from "node:assert/strict";

import { connectApi } from "../lib/api.js";
import { createTempLogger } from "../lib/file-log.js";

const WS_ENDPOINT = process.env.WS_ENDPOINT ?? "ws://127.0.0.1:9944";
const PAGE_SIZE = Number(process.env.ALPHA_HISTOGRAM_PAGE_SIZE ?? 1000);
const ONE_ALPHA_RAO = 1_000_000_000n;
const U64F64_SCALE = 1n << 64n;
const logger = createTempLogger("alpha-deprecated-stake-histogram-final.log");

const BANDS = [
  { label: "0-1000", min: 0n, max: 1000n },
  { label: "1001-10000", min: 1001n, max: 10_000n },
  { label: "10001-100000", min: 10_001n, max: 100_000n },
  { label: "100001-1000000", min: 100_001n, max: 1_000_000n },
  { label: "1000001-10000000", min: 1_000_001n, max: 10_000_000n },
  { label: "10000001-100000000", min: 10_000_001n, max: 100_000_000n },
  { label: "100000001-1000000000", min: 100_000_001n, max: ONE_ALPHA_RAO },
  { label: ">1000000000", min: ONE_ALPHA_RAO + 1n, max: null },
];

async function main() {
  await logger.start();

  const api = await connectApi(WS_ENDPOINT, { log: (...args) => logger.info(...args) });

  try {
    assert.ok(api.query.subtensorModule?.alpha, "SubtensorModule.Alpha is not available");
    assert.ok(
      api.query.subtensorModule?.totalHotkeyAlpha,
      "SubtensorModule.TotalHotkeyAlpha is not available"
    );
    assert.ok(
      api.query.subtensorModule?.totalHotkeyShares,
      "SubtensorModule.TotalHotkeyShares is not available"
    );
    assert.ok(
      api.query.subtensorModule?.totalHotkeySharesV2,
      "SubtensorModule.TotalHotkeySharesV2 is not available"
    );

    const header = await api.rpc.chain.getHeader();
    const blockHash = await api.rpc.chain.getBlockHash(header.number);
    const counts = Object.fromEntries(BANDS.map(({ label }) => [label, 0n]));
    const hotkeyTotals = new Map();
    const zeroDenominators = new Set();

    let startKey;
    let pages = 0;
    let total = 0n;
    let minStakeRao;
    let maxStakeRao = 0n;

    await logger.info(`endpoint=${WS_ENDPOINT}`);
    await logger.info("map=SubtensorModule.Alpha");
    await logger.info(
      "formula=floor(Alpha share * TotalHotkeyAlpha / denominator), denominator=TotalHotkeyShares || TotalHotkeySharesV2"
    );
    await logger.info("unit=AlphaBalance::from(1) rao");
    await logger.info(`page_size=${PAGE_SIZE}`);
    await logger.info(`block=${header.number.toString()}`);
    await logger.info(`block_hash=${blockHash.toString()}`);

    for (;;) {
      const entries = await api.query.subtensorModule.alpha.entriesPaged({
        args: [],
        pageSize: PAGE_SIZE,
        startKey,
      });

      if (entries.length === 0) {
        break;
      }

      pages += 1;

      for (const [storageKey, shareValue] of entries) {
        const [hotkey, , netuid] = storageKey.args;
        const totals = await getHotkeyTotals(api, hotkey, netuid, hotkeyTotals);

        const shareRaw = codecToBigInt(shareValue);
        const stakeRao =
          totals.denominatorNumerator === 0n
            ? 0n
            : (shareRaw * totals.totalAlphaRao * totals.denominatorDenominator) /
              (U64F64_SCALE * totals.denominatorNumerator);

        if (totals.denominatorNumerator === 0n) {
          zeroDenominators.add(totals.key);
        }

        incrementBand(counts, stakeRao);
        total += 1n;
        minStakeRao = minStakeRao === undefined || stakeRao < minStakeRao ? stakeRao : minStakeRao;
        maxStakeRao = stakeRao > maxStakeRao ? stakeRao : maxStakeRao;
      }

      startKey = entries.at(-1)[0];

      if (pages === 1 || pages % 25 === 0) {
        await logger.info(`progress_page=${pages} counted=${total.toString()} last_key=${startKey.toHex()}`);
      }
    }

    await logger.info(`pages=${pages}`);
    await logger.info(`counted_alpha_keys=${total.toString()}`);
    await logger.info(`hotkey_total_cache_entries=${hotkeyTotals.size}`);
    await logger.info(`zero_total_hotkey_shares_keys=${zeroDenominators.size}`);
    await logger.info(`min_deprecated_stake_rao=${minStakeRao?.toString() ?? "n/a"}`);
    await logger.info(`max_deprecated_stake_rao=${maxStakeRao.toString()}`);

    for (const band of BANDS) {
      await logger.info(`${band.label}=${counts[band.label].toString()}`);
    }
  } finally {
    await api.disconnect();
    await logger.flush();
  }
}

async function getHotkeyTotals(api, hotkey, netuid, cache) {
  const key = `${hotkey.toString()}|${netuid.toString()}`;
  const cached = cache.get(key);

  if (cached) {
    return cached;
  }

  const [totalAlpha, sharesV1, sharesV2] = await Promise.all([
    api.query.subtensorModule.totalHotkeyAlpha(hotkey, netuid),
    api.query.subtensorModule.totalHotkeyShares(hotkey, netuid),
    api.query.subtensorModule.totalHotkeySharesV2(hotkey, netuid),
  ]);
  const denominatorV1 = u64f64Rational(sharesV1);
  const denominatorV2 = safeFloatRational(sharesV2);
  const denominator = denominatorV1.numerator === 0n ? denominatorV2 : denominatorV1;
  const totals = {
    key,
    totalAlphaRao: codecToBigInt(totalAlpha),
    denominatorNumerator: denominator.numerator,
    denominatorDenominator: denominator.denominator,
  };

  cache.set(key, totals);
  return totals;
}

function codecToBigInt(codec) {
  if (typeof codec.toBigInt === "function") {
    return codec.toBigInt();
  }

  const json = typeof codec.toJSON === "function" ? codec.toJSON() : null;
  if (json && typeof json === "object" && "bits" in json) {
    return BigInt(json.bits);
  }

  return BigInt(codec.toString());
}

function u64f64Rational(codec) {
  return {
    numerator: codecToBigInt(codec),
    denominator: U64F64_SCALE,
  };
}

function safeFloatRational(codec) {
  const json = codec.toJSON();
  assert.ok(json && typeof json === "object", `unexpected SafeFloat JSON: ${JSON.stringify(json)}`);

  const mantissa = BigInt(json.mantissa);
  const exponent = BigInt(json.exponent);

  if (exponent >= 0n) {
    return {
      numerator: mantissa * 10n ** exponent,
      denominator: 1n,
    };
  }

  return {
    numerator: mantissa,
    denominator: 10n ** -exponent,
  };
}

function incrementBand(counts, stakeRao) {
  const band = BANDS.find(({ min, max }) => stakeRao >= min && (max === null || stakeRao <= max));
  assert.ok(band, `stake value did not fit any band: ${stakeRao}`);
  counts[band.label] += 1n;
}

main().catch(async (error) => {
  await logger.error(error);
  await logger.flush();
  process.exit(1);
});
