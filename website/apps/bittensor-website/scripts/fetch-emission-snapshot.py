"""Refresh public/catalog/emission-snapshot.json from TaoMarketCap + Finney.

Subnet prices and EMA sums use TMC's public API. Eligibility, emission-enabled
flags, MinerBurned proportions, total issuance, and root pool TAO come from
Finney storage. The output models price EMA with miner-burn scaling and the
Hill gate. On spec 444 or later, the gate settings and cadence-held midpoint
come from chain storage. Before the upgrade, the output previews the v444 gate
settings while retaining miner-burn scaling. It does not use the deprecated
``BlockEmission`` storage item.

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

RAO = 1_000_000_000
TMC_SUBNETS_URL = "https://api.taomarketcap.com/public/v1/subnets/"
TOP_N = 12
DEFAULT_EMISSION_BAR_RANK = 32
DEFAULT_EMISSION_BAR_QUANTILE = 0.61
DEFAULT_EMISSION_GATE_EXPONENT = 3.0
WEBSITE_DIR = Path(__file__).resolve().parent.parent
OUTPUT = WEBSITE_DIR / "public" / "catalog" / "emission-snapshot.json"


def fixed_u64f64_num(value) -> float:
    """Convert the SDK's U64F64/FixedU128 representation to a float."""
    if isinstance(value, dict) and "bits" in value:
        return int(value["bits"]) / 2**64
    if isinstance(value, int):
        return value / 2**64
    return float(value)


def fixed_u96f32_num(value) -> float:
    """Convert the SDK's U96F32 representation to a float."""
    if isinstance(value, dict) and "bits" in value:
        return int(value["bits"]) / 2**32
    if isinstance(value, int):
        return value / 2**32
    return float(value)


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
                    headers={
                        "User-Agent": "bittensor-website/1.0",
                        "Accept": "application/json",
                    },
                )
                with urllib.request.urlopen(req, timeout=90) as response:
                    page = json.loads(response.read())
                break
            except (urllib.error.HTTPError, TimeoutError, urllib.error.URLError) as exc:
                if attempt + 1 == retries:
                    raise RuntimeError(
                        f"TMC fetch failed at offset {offset}: {exc}"
                    ) from exc
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


def row_from_tmc(row: dict, emission_enabled: bool, miner_burned: float) -> dict:
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
        "minerBurned": round(min(max(miner_burned, 0.0), 1.0), 8),
        "emissionEnabled": emission_enabled,
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
    spec_version = await client.spec_version()
    total_issuance = int(await view.query(st.SubtensorModule.TotalIssuance))
    root_tao = int(await view.query(st.SubtensorModule.SubnetTAO, [0]))
    tao_weight_raw = int(await view.query(st.SubtensorModule.TaoWeight))

    async def subnet_flags(netuid: int) -> tuple[int, dict[str, bool | float]]:
        (
            first_emission,
            subtoken_enabled,
            registration_allowed,
            emission_enabled,
            miner_burned,
        ) = await asyncio.gather(
            view.query(st.SubtensorModule.FirstEmissionBlockNumber, [netuid]),
            view.query(st.SubtensorModule.SubtokenEnabled, [netuid]),
            view.query(st.SubtensorModule.NetworkRegistrationAllowed, [netuid]),
            view.query(st.SubtensorModule.SubnetEmissionEnabled, [netuid]),
            view.query(st.SubtensorModule.MinerBurned, [netuid]),
        )
        return netuid, {
            "eligible": (
                first_emission is not None
                and bool(subtoken_enabled)
                and bool(registration_allowed)
            ),
            "emissionEnabled": bool(emission_enabled),
            "minerBurned": fixed_u96f32_num(miner_burned),
        }

    flags = dict(await asyncio.gather(*(subnet_flags(netuid) for netuid in netuids)))

    gate = None
    if spec_version >= 444:
        rank, quantile, exponent, bar = await asyncio.gather(
            view.query(st.SubtensorModule.EmissionBarRank),
            view.query(st.SubtensorModule.EmissionBarQuantile),
            view.query(st.SubtensorModule.EmissionGateExponent),
            view.query(st.SubtensorModule.EmissionGateBar),
        )
        gate = {
            "rank": int(rank),
            "quantile": fixed_u64f64_num(quantile),
            "exponent": fixed_u64f64_num(exponent),
            "bar": fixed_u64f64_num(bar),
            "source": "chain_storage",
        }

    issuance_tao = total_issuance / RAO
    return {
        "specVersion": spec_version,
        "totalIssuanceRao": total_issuance,
        "totalIssuanceTao": round(issuance_tao, 3),
        "blockEmissionTao": round(block_emission_calculated(issuance_tao), 6),
        "rootTao": round(root_tao / RAO, 2),
        "taoWeight": round(tao_weight_raw / (2**64 - 1), 6),
        "subnetFlags": flags,
        "emissionGate": gate,
    }


def select_emission_gate_bar(
    demand_shares: list[float],
    rank: int = DEFAULT_EMISSION_BAR_RANK,
    quantile: float = DEFAULT_EMISSION_BAR_QUANTILE,
) -> float:
    """Mirror ``maybe_update_emission_gate_bar`` for the v444 snapshot."""
    positive = sorted((share for share in demand_shares if share > 0), reverse=True)
    if not positive:
        return 0.0
    if rank > 0:
        return positive[min(rank, len(positive)) - 1]

    cumulative = 0.0
    for share in positive:
        cumulative += share
        if cumulative >= quantile:
            return share
    return positive[-1]


def apply_shares(
    subnets: list[dict],
    block_emission: float,
    *,
    gate_bar: float | None = None,
    gate_exponent: float = DEFAULT_EMISSION_GATE_EXPONENT,
) -> float:
    """Apply price shares, miner-burn scaling, the Hill gate, and enable flags."""
    price_sum = sum(max(subnet["emaPrice"], 0.0) for subnet in subnets)
    demand_shares = [
        max(subnet["emaPrice"], 0.0) / price_sum if price_sum > 0 else 0.0
        for subnet in subnets
    ]
    burn_weights = [
        share * (1.0 - min(max(subnet["minerBurned"], 0.0), 1.0))
        for subnet, share in zip(subnets, demand_shares)
    ]
    burn_weight_sum = sum(burn_weights)
    burn_adjusted_shares = (
        [weight / burn_weight_sum for weight in burn_weights]
        if burn_weight_sum > 0
        else demand_shares
    )
    if gate_bar is None:
        gate_bar = select_emission_gate_bar(burn_adjusted_shares)

    gate_factors = []
    gated_weights = []
    for share in burn_adjusted_shares:
        gate = (
            1.0 / (1.0 + (gate_bar / share) ** gate_exponent)
            if share > 0 and gate_bar > 0
            else (1.0 if share > 0 else 0.0)
        )
        gate_factors.append(gate)
        gated_weights.append(share * gate)

    if sum(gated_weights) == 0:
        gated_weights = burn_adjusted_shares

    enabled_total = sum(
        weight
        for subnet, weight in zip(subnets, gated_weights)
        if subnet["emissionEnabled"]
    )
    for subnet, demand_share, burn_adjusted_share, gate, weight in zip(
        subnets, demand_shares, burn_adjusted_shares, gate_factors, gated_weights
    ):
        share = (
            weight / enabled_total
            if subnet["emissionEnabled"] and enabled_total > 0
            else 0.0
        )
        subnet["demandShare"] = round(demand_share, 8)
        subnet["burnAdjustedShare"] = round(burn_adjusted_share, 8)
        subnet["gateFactor"] = round(gate, 8)
        subnet["taoShare"] = round(share, 8)
        subnet["taoPerBlock"] = round(block_emission * share, 8)

    return gate_bar


async def build_snapshot() -> dict:
    tmc_rows = fetch_tmc_subnets()
    non_root = [r for r in tmc_rows if int(r["netuid"]) != 0]

    async with bt.Subtensor() as client:
        chain = await chain_fields(client, [int(row["netuid"]) for row in non_root])

    eligible_rows = [
        row for row in non_root if chain["subnetFlags"][int(row["netuid"])]["eligible"]
    ]

    ema_price_sum = sum(
        tmc_num((r.get("latest_snapshot") or {}).get("subnet_moving_price"))
        for r in eligible_rows
    )

    subnets = [
        row_from_tmc(
            row,
            chain["subnetFlags"][int(row["netuid"])]["emissionEnabled"],
            chain["subnetFlags"][int(row["netuid"])]["minerBurned"],
        )
        for row in eligible_rows
    ]
    gate = chain["emissionGate"] or {
        "rank": DEFAULT_EMISSION_BAR_RANK,
        "quantile": DEFAULT_EMISSION_BAR_QUANTILE,
        "exponent": DEFAULT_EMISSION_GATE_EXPONENT,
        "bar": None,
        "source": "v444_defaults_recomputed",
    }
    gate_bar = apply_shares(
        subnets,
        chain["blockEmissionTao"],
        gate_bar=gate["bar"],
        gate_exponent=gate["exponent"],
    )

    subnets_by_netuid = {subnet["netuid"]: subnet for subnet in subnets}
    top_subnets = sorted(subnets, key=lambda subnet: subnet["taoShare"], reverse=True)[
        :TOP_N
    ]
    featured_netuid = 4 if 4 in subnets_by_netuid else top_subnets[0]["netuid"]

    featured = subnets_by_netuid[featured_netuid]

    return {
        "fetchedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "network": "finney",
        "chainSpecVersion": chain["specVersion"],
        "emissionMode": "price_ema_miner_burn_hill_gate",
        "emissionGateSource": gate["source"],
        "dataSource": {
            "subnets": "taomarketcap.com",
            "chain": "finney",
            "tmcEndpoint": TMC_SUBNETS_URL,
        },
        "blockEmissionTao": chain["blockEmissionTao"],
        "totalIssuanceTao": chain["totalIssuanceTao"],
        "totalIssuanceRao": chain["totalIssuanceRao"],
        "rootTao": chain["rootTao"],
        "emaPriceSum": round(ema_price_sum, 4),
        "rootDividendGateOpen": ema_price_sum > 1.0,
        "taoWeight": chain["taoWeight"],
        "emissionGateRank": gate["rank"],
        "emissionGateQuantile": round(gate["quantile"], 8),
        "emissionGateExponent": round(gate["exponent"], 8),
        "emissionGateBar": round(gate_bar, 8),
        "emissionInputs": [
            {
                "netuid": subnet["netuid"],
                "emaPrice": subnet["emaPrice"],
                "minerBurned": subnet["minerBurned"],
                "emissionEnabled": subnet["emissionEnabled"],
            }
            for subnet in subnets
        ],
        "featuredSubnet": featured,
        "topSubnets": top_subnets,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="exit 1 if output would change"
    )
    args = parser.parse_args()

    snapshot = asyncio.run(build_snapshot())
    rendered = json.dumps(snapshot, indent=2) + "\n"

    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != rendered:
            print(
                f"{OUTPUT} is stale; run scripts/fetch-emission-snapshot.py",
                file=sys.stderr,
            )
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
