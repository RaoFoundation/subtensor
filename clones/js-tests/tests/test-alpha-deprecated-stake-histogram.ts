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
    const blockHash = await api.rpc.chain.getBlockHash(header.number.unwrap());
    const counts = Object.fromEntries(BANDS.map(({ label }) => [label, 0n]));
    const zeroDenominators = new Set();

    // Prefetch the per-hotkey totals maps in full (a few hundred pages) so the
    // Alpha scan below needs zero per-entry RPC round-trips. Looking the
    // totals up one (hotkey, netuid) at a time meant hundreds of thousands of
    // sequential storage queries on mainnet state, which blew the shard's
    // per-test time budget before the scan even finished.
    const totalsByKey = buildHotkeyTotals(
      await fetchFullMap(api.query.subtensorModule.totalHotkeyAlpha, "prefetch_total_hotkey_alpha"),
      await fetchFullMap(api.query.subtensorModule.totalHotkeyShares, "prefetch_total_hotkey_shares"),
      await fetchFullMap(api.query.subtensorModule.totalHotkeySharesV2, "prefetch_total_hotkey_shares_v2")
    );

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
        const totals = getHotkeyTotals(hotkey, netuid, totalsByKey);

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
    await logger.info(`hotkey_total_cache_entries=${totalsByKey.size}`);
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

async function fetchFullMap(query, label) {
  const map = new Map();
  let startKey;
  let pages = 0;

  for (;;) {
    const entries = await query.entriesPaged({ args: [], pageSize: PAGE_SIZE, startKey });
    if (entries.length === 0) {
      break;
    }
    pages += 1;
    for (const [storageKey, value] of entries) {
      const [hotkey, netuid] = storageKey.args;
      map.set(`${hotkey.toString()}|${netuid.toString()}`, value);
    }
    startKey = entries.at(-1)[0];
  }

  await logger.info(`${label}: entries=${map.size} pages=${pages}`);
  return map;
}

function buildHotkeyTotals(totalAlphaMap, sharesV1Map, sharesV2Map) {
  const totals = new Map();
  const keys = new Set([...totalAlphaMap.keys(), ...sharesV1Map.keys(), ...sharesV2Map.keys()]);

  for (const key of keys) {
    const totalAlpha = totalAlphaMap.get(key);
    const sharesV1 = sharesV1Map.get(key);
    const sharesV2 = sharesV2Map.get(key);
    // A key absent from a map has the storage default (zero), same as the
    // previous per-entry queries returned.
    const denominatorV1 = sharesV1 ? u64f64Rational(sharesV1) : { numerator: 0n, denominator: U64F64_SCALE };
    const denominatorV2 = sharesV2 ? safeFloatRational(sharesV2) : { numerator: 0n, denominator: 1n };
    const denominator = denominatorV1.numerator === 0n ? denominatorV2 : denominatorV1;
    totals.set(key, {
      key,
      totalAlphaRao: totalAlpha ? codecToBigInt(totalAlpha) : 0n,
      denominatorNumerator: denominator.numerator,
      denominatorDenominator: denominator.denominator,
    });
  }

  return totals;
}

const ZERO_TOTALS_TEMPLATE = {
  totalAlphaRao: 0n,
  denominatorNumerator: 0n,
  denominatorDenominator: 1n,
};

function getHotkeyTotals(hotkey, netuid, totalsByKey) {
  const key = `${hotkey.toString()}|${netuid.toString()}`;
  return totalsByKey.get(key) ?? { key, ...ZERO_TOTALS_TEMPLATE };
}

function codecToBigInt(codec): bigint {
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
