"""Rebuild the frozen basket index and per-fund baselines.

Samples every validator basket at a fixed half-daily block grid from finney
archive state, computes the chained NAV-weighted basket index, stamps each
fund's display baseline at its first sighting, and rewrites
``bittensor/basket_index_data.py``.

The grid is anchored at the epoch block with a fixed step, so reruns are
deterministic: historical index values and already-stamped baselines never
change, new samples and newly sighted funds are appended.

Usage (from ``sdk/python``):

    uv run --no-sync python scripts/build_basket_index.py
    uv run --no-sync python scripts/build_basket_index.py --cache /tmp/basket_samples.json
"""

from __future__ import annotations

import argparse
import asyncio
import json
from pathlib import Path

import bittensor as bt
from bittensor._generated import runtime_apis as api
from bittensor.basket_index_data import EPOCH_BLOCK, SAMPLE_STEP

DATA_MODULE = Path(__file__).resolve().parent.parent / "bittensor" / "basket_index_data.py"

# Funds below this realizable NAV are too shallow to price honestly; they are
# excluded from index chaining (their weight would be negligible anyway) but
# still get a baseline stamped so their display price is well-defined.
MIN_INDEX_NAV_RAO = 100_000_000  # τ0.1


async def fetch_sample(snapshot_factory, block: int) -> dict[str, dict]:
    snap = await snapshot_factory(block)
    rows = await snap.runtime(api.BetaBasketRuntimeApi.get_all_validator_baskets, [])
    out: dict[str, dict] = {}
    for row in rows or []:
        shares = int(row.get("shares") or 0)
        nav = int(row.get("nav_tao") or 0)
        if shares == 0 or nav == 0:
            continue
        out[str(row["hotkey"])] = {"nav": nav, "price": nav / shares}
    return out


async def collect_samples(cache_path: Path | None) -> list[dict]:
    """Basket state at every grid block: ``{"block", "funds": {hotkey: {nav, price}}}``."""
    cached: dict[int, dict[str, dict]] = {}
    if cache_path and cache_path.exists():
        for point in json.loads(cache_path.read_text())["points"]:
            block = int(point["block"])
            if (block - EPOCH_BLOCK) % SAMPLE_STEP != 0:
                continue
            cached[block] = {
                hotkey: {"nav": int(point["navs"][hotkey]), "price": float(price)}
                for hotkey, price in point["prices"].items()
            }

    async with bt.Subtensor(network="finney") as client:
        now = await client.at()
        blocks = list(range(EPOCH_BLOCK, now.block + 1, SAMPLE_STEP))
        samples: list[dict] = []
        for block in blocks:
            funds = cached.get(block)
            if funds is None:
                try:
                    funds = await fetch_sample(now.at, block)
                except Exception as error:
                    print(f"skip {block}: {error}", flush=True)
                    continue
                print(f"sampled {block}: {len(funds)} funds", flush=True)
            samples.append({"block": block, "funds": funds})

    if cache_path:
        cache_path.write_text(
            json.dumps(
                {
                    "points": [
                        {
                            "block": s["block"],
                            "prices": {hk: f["price"] for hk, f in s["funds"].items()},
                            "navs": {hk: f["nav"] for hk, f in s["funds"].items()},
                        }
                        for s in samples
                    ]
                }
            )
        )
    return samples


Index = list[tuple[int, float]]
Baselines = dict[str, tuple[float, int]]


def chain_index(samples: list[dict]) -> tuple[Index, Baselines]:
    """Chained NAV-weighted index plus per-fund baselines stamped at first sighting."""
    index: Index = []
    baselines: Baselines = {}
    level = 1.0
    previous: dict[str, dict] = {}

    for sample in samples:
        block, funds = sample["block"], sample["funds"]
        if index:
            # NAV-weighted mean of per-fund price relatives across funds deep
            # enough to price at both ends; flows don't move it, only returns.
            weighted, weight = 0.0, 0
            for hotkey, fund in funds.items():
                prior = previous.get(hotkey)
                if not prior or prior["nav"] < MIN_INDEX_NAV_RAO or fund["nav"] < MIN_INDEX_NAV_RAO:
                    continue
                weighted += prior["nav"] * (fund["price"] / prior["price"])
                weight += prior["nav"]
            if weight:
                level *= weighted / weight
        index.append((block, level))

        for hotkey, fund in funds.items():
            if hotkey not in baselines:
                baselines[hotkey] = (fund["price"] / level, block)
        previous = funds

    return index, baselines


def write_data_module(index: Index, baselines: Baselines) -> None:
    lines = [
        '"""Frozen basket index and per-fund display baselines.',
        "",
        "Generated by ``scripts/build_basket_index.py`` — do not edit by hand.",
        "",
        "``INDEX`` is the chained NAV-weighted basket index (level 1.0 at the",
        "epoch block), sampled on a fixed half-daily block grid. ``BASELINES``",
        "maps each fund hotkey to ``(baseline, first_block)``: the divisor that",
        "splices the fund onto the index at its first sighting, so its display",
        "price starts exactly at the index level of that block.",
        '"""',
        "",
        "from __future__ import annotations",
        "",
        f"EPOCH_BLOCK = {EPOCH_BLOCK}",
        f"SAMPLE_STEP = {SAMPLE_STEP}",
        f"GENERATED_BLOCK = {index[-1][0]}",
        "",
        "INDEX: tuple[tuple[int, float], ...] = (",
        *[f"    ({block}, {level!r})," for block, level in index],
        ")",
        "",
        "BASELINES: dict[str, tuple[float, int]] = {",
        *[
            f'    "{hotkey}": ({baseline!r}, {first_block}),'
            for hotkey, (baseline, first_block) in sorted(
                baselines.items(), key=lambda item: (item[1][1], item[0])
            )
        ],
        "}",
        "",
    ]
    DATA_MODULE.write_text("\n".join(lines))


async def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--cache", type=Path, default=None, help="Sample cache JSON (read + refreshed)"
    )
    args = parser.parse_args()

    samples = await collect_samples(args.cache)
    index, baselines = chain_index(samples)
    write_data_module(index, baselines)
    print(
        f"wrote {DATA_MODULE.name}: {len(index)} index points, "
        f"{len(baselines)} baselines, latest level {index[-1][1]:.6f}"
    )


if __name__ == "__main__":
    asyncio.run(main())
