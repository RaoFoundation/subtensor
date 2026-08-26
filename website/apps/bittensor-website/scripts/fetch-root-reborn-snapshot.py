"""Refresh public/catalog/root-reborn-snapshot.json from TaoMarketCap + finney.

Raw network values behind the Root Reborn launch article: root stake, subnet
pool liquidity, the measured root dividend stream, and the top dividend-paying
subnets. Subnet-side data comes from TMC's public API; issuance, total stake,
and the root pool come from finney storage.

Usage (from bittensor-website/, with the bittensor SDK venv active):

    python scripts/fetch-root-reborn-snapshot.py
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import bittensor as bt
from bittensor._generated import runtime_apis as api
from bittensor._generated import storage as st
from bittensor.basket_index import fund_pricing
from bittensor.basket_index_data import EPOCH_BLOCK, INDEX
from bittensor.cli.helpers import chain_identity_names

RAO = 1_000_000_000
BLOCKS_PER_DAY = 7200
TMC_SUBNETS_URL = "https://api.taomarketcap.com/public/v1/subnets/"
TMC_REVENUE_URL = (
    "https://api.taomarketcap.com/internal/v1/general/protocol-revenue-chart/?span=ALL"
)
TMC_TAO_PRICE_URL = "https://api.taomarketcap.com/internal/v1/market/candle-data/?span=1D"
TOP_N = 8
BASKET_TOP_N = 8
WEBSITE_DIR = Path(__file__).resolve().parent.parent
OUTPUT = WEBSITE_DIR / "public" / "catalog" / "root-reborn-snapshot.json"


def tmc_num(val) -> float:
    if val is None:
        return 0.0
    if isinstance(val, (int, float)):
        return float(val)
    if isinstance(val, str):
        try:
            return float(val)
        except ValueError:
            return 0.0
    if isinstance(val, dict) and "bits" in val:
        return int(val["bits"]) / 2**32
    return 0.0


def fetch_tmc_subnets(retries: int = 5) -> list[dict]:
    rows: list[dict] = []
    offset = 0
    while True:
        url = f"{TMC_SUBNETS_URL}?limit=50&offset={offset}"
        for attempt in range(retries):
            try:
                req = urllib.request.Request(
                    url,
                    headers={"User-Agent": "bittensor-website/1.0", "Accept": "application/json"},
                )
                with urllib.request.urlopen(req, timeout=90) as response:
                    page = json.loads(response.read())
                break
            except (urllib.error.HTTPError, TimeoutError, urllib.error.URLError) as exc:
                if attempt + 1 == retries:
                    raise RuntimeError(f"TMC fetch failed at offset {offset}: {exc}") from exc
                time.sleep(2**attempt)
        rows.extend(page["results"])
        if not page.get("next"):
            break
        offset += 50
        time.sleep(0.4)
    return rows


def subnet_name(row: dict) -> str:
    snap = row.get("latest_snapshot") or {}
    identities = snap.get("subnet_identities_v3") or {}
    return identities.get("subnetName") or f"SN{row['netuid']}"


def fetch_tao_usd(retries: int = 5) -> float:
    """Latest TAO/USD close from TMC's market candle series."""
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                TMC_TAO_PRICE_URL,
                headers={"User-Agent": "bittensor-website/1.0", "Accept": "application/json"},
            )
            with urllib.request.urlopen(req, timeout=90) as response:
                rows = json.loads(response.read())
            break
        except (urllib.error.HTTPError, TimeoutError, urllib.error.URLError) as exc:
            if attempt + 1 == retries:
                raise RuntimeError(f"TMC TAO price fetch failed: {exc}") from exc
            time.sleep(2**attempt)

    if not rows:
        raise RuntimeError("TMC candle-data returned no rows")
    return round(float(rows[-1]["close"]), 2)


def fetch_protocol_revenue(retries: int = 5) -> dict:
    """Daily root dividend revenue from TMC's protocol-revenue series (TAO-equivalent,
    alpha valued at the daily close). This is full daily accounting, so it is the
    source of truth for the revenue run-rate; the per-subnet pending-dividend
    extrapolation below is only used for relative shares."""
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                TMC_REVENUE_URL,
                headers={"User-Agent": "bittensor-website/1.0", "Accept": "application/json"},
            )
            with urllib.request.urlopen(req, timeout=90) as response:
                rows = json.loads(response.read())
            break
        except (urllib.error.HTTPError, TimeoutError, urllib.error.URLError) as exc:
            if attempt + 1 == retries:
                raise RuntimeError(f"TMC revenue fetch failed: {exc}") from exc
            time.sleep(2**attempt)

    final = [r for r in rows if not r.get("is_projected")]
    last = final[-1]
    last7 = [r["revenue_tao"] / RAO for r in final[-7:]]
    return {
        "asOfDay": last["day"],
        "lastDayTao": round(last["revenue_tao"] / RAO, 0),
        "pctOfEmission": round(last["pct_of_emission"] / 100, 4),
        "sevenDayAvgTao": round(sum(last7) / len(last7), 0),
        "cumulativeTao": round(sum(r["revenue_tao"] for r in final) / RAO, 0),
        "sinceDay": final[0]["day"],
    }


async def chain_fields() -> dict:
    async with bt.Subtensor() as client:
        view = await client.at()
        total_issuance = int(await view.query(st.SubtensorModule.TotalIssuance))
        root_tao = int(await view.query(st.SubtensorModule.SubnetTAO, [0]))
        total_stake = int(await view.query(st.SubtensorModule.TotalStake))
        basket_chart = await fetch_basket_chart(client)
    return {
        "totalIssuanceTao": round(total_issuance / RAO, 1),
        "rootStakeTao": round(root_tao / RAO, 1),
        "totalStakeTao": round(total_stake / RAO, 1),
        "basketChart": basket_chart,
    }


async def _basket_prices(client, block: int) -> dict[str, tuple[int, float]]:
    """hotkey -> (nav rao, raw beta price) for every live fund at ``block``."""
    snap = await client.at(block)
    rows = await snap.runtime(api.BetaBasketRuntimeApi.get_all_validator_baskets, [])
    out: dict[str, tuple[int, float]] = {}
    for row in rows or []:
        shares = int(row.get("shares") or 0)
        nav = int(row.get("nav_tao") or 0)
        if shares and nav:
            out[str(row["hotkey"])] = (nav, nav / shares)
    return out


async def fetch_basket_chart(client) -> dict:
    """Index-spliced display-price history for the largest validator baskets.

    Rides the frozen basket-index sample grid (half-daily blocks) so the
    article's chart shows exactly the numbers btcli shows: each fund's raw
    beta price divided by its frozen launch baseline, beside the index line.
    """
    now = await client.at()
    latest = await _basket_prices(client, now.block)
    top = sorted(latest.items(), key=lambda item: -item[1][0])[:BASKET_TOP_N]
    hotkeys = [hotkey for hotkey, _ in top]
    names = await chain_identity_names(client, hotkeys)

    blocks = [block for block, _ in INDEX]
    series: dict[str, list] = {hotkey: [] for hotkey in hotkeys}
    for block in blocks:
        prices = await _basket_prices(client, block)
        for hotkey in hotkeys:
            entry = prices.get(hotkey)
            if entry is None:
                series[hotkey].append(None)
            else:
                _, raw = entry
                baseline = fund_pricing(hotkey, raw).baseline
                series[hotkey].append(round(raw / baseline, 4))

    return {
        "epochBlock": EPOCH_BLOCK,
        "blocks": blocks,
        "index": [round(level, 4) for _, level in INDEX],
        "funds": [
            {
                "hotkey": hotkey,
                "name": names.get(hotkey),
                "navTao": round(nav / RAO, 1),
                "series": series[hotkey],
            }
            for hotkey, (nav, _) in top
        ],
    }


def build_snapshot(tmc_rows: list[dict], chain: dict) -> dict:
    non_root = [r for r in tmc_rows if int(r["netuid"]) != 0]
    live = [r for r in non_root if (r.get("latest_snapshot") or {}).get("subnet_emission_enabled")]
    block = max(int((r.get("latest_snapshot") or {}).get("block_number") or 0) for r in tmc_rows)

    # SubnetTAO across every non-root subnet — the TAO the chain (and stakers)
    # put into pools via TAO→alpha buys. It sits as protocol liquidity until the
    # subnet is deregistered (or someone sells alpha back out). Include inactive
    # subnets: their pools still hold that TAO.
    tao_in_pools = 0.0
    ema_price_sum = 0.0
    alpha_mcap_tao = 0.0
    miners_tao_per_day = 0.0
    tao_per_day_into_pools = 0.0
    root_div_rows: list[dict] = []

    for r in non_root:
        snap = r.get("latest_snapshot") or {}
        ema_price_sum += tmc_num(snap.get("subnet_moving_price"))
        tao_in_pools += tmc_num(snap.get("subnet_tao")) / RAO

    for r in live:
        snap = r["latest_snapshot"]
        price = tmc_num(snap.get("price"))
        pool_tao = tmc_num(snap.get("subnet_tao")) / RAO
        alpha_mcap_tao += tmc_num((snap.get("dtao") or {}).get("marketCap"))
        miners_tao_per_day += tmc_num(snap.get("miners_tao_per_day"))
        tao_per_day_into_pools += tmc_num(snap.get("subnet_tao_in_emission")) / RAO * BLOCKS_PER_DAY

        # Per-subnet root dividend run-rate: pending root alpha divs accrue over the
        # current tempo, so pending / blocks_elapsed * blocks_per_day * price estimates
        # this subnet's TAO/day flow to root. The absolute level is a mid-tempo,
        # spot-priced extrapolation (it undershoots the full daily accounting), so only
        # the *relative shares* are published; the headline run-rate comes from the
        # protocol-revenue series.
        blocks_elapsed = tmc_num(snap.get("blocks_since_last_step"))
        if blocks_elapsed > 0:
            tao_per_day = (
                tmc_num(snap.get("pending_root_alpha_divs"))
                / RAO
                / blocks_elapsed
                * BLOCKS_PER_DAY
                * price
            )
            root_div_rows.append(
                {
                    "netuid": int(r["netuid"]),
                    "name": subnet_name(r),
                    "taoPerDayEstimate": tao_per_day,
                    "price": round(price, 6),
                    "poolTao": round(pool_tao, 0),
                }
            )

    root_div_rows.sort(key=lambda row: -row["taoPerDayEstimate"])
    estimate_sum = sum(row["taoPerDayEstimate"] for row in root_div_rows)
    top_rows = []
    for row in root_div_rows[:TOP_N]:
        share = row["taoPerDayEstimate"] / estimate_sum if estimate_sum else 0.0
        top_rows.append(
            {
                "netuid": row["netuid"],
                "name": row["name"],
                "shareOfRootDividends": round(share, 4),
                "price": row["price"],
                "poolTao": row["poolTao"],
            }
        )

    revenue = fetch_protocol_revenue()
    tao_usd = fetch_tao_usd()
    root_dividends_tao_per_day = revenue["sevenDayAvgTao"]
    root_stake = chain["rootStakeTao"]
    return {
        "fetchedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "network": "finney",
        "block": block,
        "dataSource": {
            "subnets": "taomarketcap.com",
            "chain": "finney",
            "tmcEndpoint": TMC_SUBNETS_URL,
            "tmcRevenueEndpoint": TMC_REVENUE_URL,
            "tmcTaoPriceEndpoint": TMC_TAO_PRICE_URL,
        },
        **chain,
        "taoUsd": tao_usd,
        "rootShareOfIssuance": round(root_stake / chain["totalIssuanceTao"], 4),
        "rootShareOfStake": round(root_stake / chain["totalStakeTao"], 4),
        "liveSubnets": len(live),
        "registeredSubnets": len(non_root),
        "taoInSubnetPools": round(tao_in_pools, 0),
        "emaPriceSum": round(ema_price_sum, 4),
        "rootDividendGateOpen": ema_price_sum > 1.0,
        "alphaMarketCapTao": round(alpha_mcap_tao, 0),
        "minersTaoPerDay": round(miners_tao_per_day, 0),
        "taoPerDayIntoPools": round(tao_per_day_into_pools, 0),
        "rootDividendsTaoPerDay": root_dividends_tao_per_day,
        "rootDividendsLastDayTao": revenue["lastDayTao"],
        "rootDividendsPctOfEmission": revenue["pctOfEmission"],
        "rootRevenueAsOfDay": revenue["asOfDay"],
        "cumulativeRootRevenueTao": revenue["cumulativeTao"],
        "rootRevenueSinceDay": revenue["sinceDay"],
        "rootYieldApr": round(root_dividends_tao_per_day * 365 / root_stake, 4),
        "topRootDividendSubnets": top_rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="exit 1 if output would change")
    args = parser.parse_args()

    tmc_rows = fetch_tmc_subnets()
    chain = asyncio.run(chain_fields())
    snapshot = build_snapshot(tmc_rows, chain)
    rendered = json.dumps(snapshot, indent=2) + "\n"

    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != rendered:
            print(f"{OUTPUT} is stale; run scripts/fetch-root-reborn-snapshot.py", file=sys.stderr)
            return 1
        print(f"{OUTPUT} is up to date.")
        return 0

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(rendered)
    print(f"Wrote {OUTPUT}")
    print(
        f"root stake = {snapshot['rootStakeTao']:,.0f} τ "
        f"(${snapshot['rootStakeTao'] * snapshot['taoUsd']:,.0f} @ ${snapshot['taoUsd']}/τ), "
        f"root dividends = {snapshot['rootDividendsTaoPerDay']:,.0f} τ/day"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
