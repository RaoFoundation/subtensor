"""Refresh public/catalog/emission-snapshot.json from TaoMarketCap + finney.

Subnet prices, EMA sums, and the root dividend gate use TMC's public API
(`subnet_moving_price` summed across all subnets). Miner-burn penalties,
total issuance, block emission, and root pool TAO come from finney storage.

Usage (from bittensor-website/, with the bittensor SDK venv active):

    python scripts/fetch-emission-snapshot.py
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
from bittensor._generated import storage as st

I96 = 2**32
RAO = 1_000_000_000
TMC_SUBNETS_URL = "https://api.taomarketcap.com/public/v1/subnets/"
TOP_N = 12
WEBSITE_DIR = Path(__file__).resolve().parent.parent
OUTPUT = WEBSITE_DIR / "public" / "catalog" / "emission-snapshot.json"


def fixed_to_float(val) -> float:
    if val is None:
        return 0.0
    if isinstance(val, (int, float)):
        return float(val)
    if isinstance(val, dict) and "bits" in val:
        return int(val["bits"]) / I96
    bits = getattr(val, "bits", None)
    if bits is not None:
        return int(bits) / I96
    return float(val)


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
        return int(val["bits"]) / I96
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
                time.sleep(2 ** attempt)
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


def row_from_tmc(row: dict, miner_burned: float) -> dict:
    snap = row.get("latest_snapshot") or {}
    netuid = int(row["netuid"])
    spot = tmc_num(snap.get("price"))
    ema = tmc_num(snap.get("subnet_moving_price"))
    tao_in = tmc_num(snap.get("subnet_tao")) / RAO
    alpha_in = tmc_num(snap.get("subnet_alpha_in")) / RAO
    alpha_out = tmc_num(snap.get("subnet_alpha_out")) / RAO
    return {
        "netuid": netuid,
        "name": subnet_name(row),
        "spotPrice": round(spot, 6),
        "emaPrice": round(ema, 6),
        "minerBurned": round(min(max(miner_burned, 0.0), 1.0), 4),
        "taoIn": round(tao_in, 2),
        "alphaIn": round(alpha_in, 2),
        "alphaOut": round(alpha_out, 2),
    }


def block_emission_calculated(issuance_tao: float) -> float:
    total_supply = 21_000_000
    halving_ref = 10_500_000
    if issuance_tao >= total_supply:
        return 0.0
    x = issuance_tao / (2 * halving_ref)
    if x >= 1:
        return 0.0
    import math

    k = math.floor(math.log2(1 / (1 - x)))
    return 1.0 / (2**k)


async def chain_fields(client: bt.Client, netuids: list[int]) -> dict:
    view = await client.at()
    total_issuance = int(await view.query(st.SubtensorModule.TotalIssuance))
    block_emission = int(await view.query(st.SubtensorModule.BlockEmission))
    root_tao = int(await view.query(st.SubtensorModule.SubnetTAO, [0]))
    tao_weight_raw = int(await view.query(st.SubtensorModule.TaoWeight))

    miner_burned: dict[int, float] = {}
    for netuid in netuids:
        burned = await view.query(st.SubtensorModule.MinerBurned, [netuid])
        miner_burned[netuid] = fixed_to_float(burned)

    issuance_tao = total_issuance / RAO
    return {
        "totalIssuanceRao": total_issuance,
        "totalIssuanceTao": round(issuance_tao, 3),
        "blockEmissionTao": round(block_emission / RAO, 6),
        "blockEmissionCalculatedTao": round(block_emission_calculated(issuance_tao), 6),
        "rootTao": round(root_tao / RAO, 2),
        "taoWeight": round(tao_weight_raw / (2**64 - 1), 6),
        "minerBurned": miner_burned,
    }


def apply_shares(subnets: list[dict], block_emission: float) -> None:
    weights = [s["emaPrice"] * (1 - s["minerBurned"]) for s in subnets]
    weight_sum = sum(weights) or 1.0
    for subnet, weight in zip(subnets, weights):
        share = weight / weight_sum
        subnet["taoShare"] = round(share, 6)
        subnet["taoPerBlock"] = round(block_emission * share, 6)


async def build_snapshot() -> dict:
    tmc_rows = fetch_tmc_subnets()
    non_root = [r for r in tmc_rows if int(r["netuid"]) != 0]

    ema_price_sum = sum(
        tmc_num((r.get("latest_snapshot") or {}).get("subnet_moving_price")) for r in non_root
    )

    top_rows = sorted(
        non_root,
        key=lambda r: tmc_num((r.get("latest_snapshot") or {}).get("price")),
        reverse=True,
    )[:TOP_N]
    top_netuids = [int(r["netuid"]) for r in top_rows]
    featured_netuid = 4 if 4 in top_netuids else top_netuids[0]

    async with bt.Subtensor() as client:
        chain = await chain_fields(client, top_netuids + [featured_netuid])

    subnets = [
        row_from_tmc(row, chain["minerBurned"].get(int(row["netuid"]), 0.0)) for row in top_rows
    ]
    apply_shares(subnets, chain["blockEmissionTao"])

    featured = next(s for s in subnets if s["netuid"] == featured_netuid)

    return {
        "fetchedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "network": "finney",
        "emissionMode": "price_ema",
        "dataSource": {
            "subnets": "taomarketcap.com",
            "chain": "finney",
            "tmcEndpoint": TMC_SUBNETS_URL,
        },
        "blockEmissionTao": chain["blockEmissionTao"],
        "blockEmissionCalculatedTao": chain["blockEmissionCalculatedTao"],
        "totalIssuanceTao": chain["totalIssuanceTao"],
        "totalIssuanceRao": chain["totalIssuanceRao"],
        "rootTao": chain["rootTao"],
        "emaPriceSum": round(ema_price_sum, 4),
        "rootDividendGateOpen": ema_price_sum > 1.0,
        "taoWeight": chain["taoWeight"],
        "featuredSubnet": featured,
        "topSubnets": subnets,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="exit 1 if output would change")
    args = parser.parse_args()

    snapshot = asyncio.run(build_snapshot())
    rendered = json.dumps(snapshot, indent=2) + "\n"

    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != rendered:
            print(f"{OUTPUT} is stale; run scripts/fetch-emission-snapshot.py", file=sys.stderr)
            return 1
        print(f"{OUTPUT} is up to date.")
        return 0

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(rendered)
    print(f"Wrote {OUTPUT}")
    print(
        f"Σ EMA = {snapshot['emaPriceSum']:.4f}, "
        f"root gate {'open' if snapshot['rootDividendGateOpen'] else 'closed'}, "
        f"issuance = {snapshot['totalIssuanceTao']:,.3f} τ"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
