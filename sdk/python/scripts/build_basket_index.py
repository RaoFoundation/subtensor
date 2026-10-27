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
from bittensor._generated import storage as st
from bittensor.basket_index_data import EPOCH_BLOCK, SAMPLE_STEP

DATA_MODULE = Path(__file__).resolve().parent.parent / "bittensor" / "basket_index_data.py"

# Funds below this realizable NAV are too shallow to price honestly; they are
# excluded from index chaining (their weight would be negligible anyway) but
# still get a baseline stamped so their display price is well-defined.
MIN_INDEX_NAV_RAO = 100_000_000  # τ0.1


def _i96f32_float(value) -> float:
    """Decode an I96F32 fixed-point chain value to a float, fraction kept."""
    bits = int(value.get("bits") or 0) if isinstance(value, dict) else int(value or 0)
    return bits / 2**32


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
    if out:
        hotkeys = list(out)
        rates = await snap.query_batch(
            st.SubtensorModule.BasketRate, [[hotkey] for hotkey in hotkeys]
        )
        for hotkey, rate in zip(hotkeys, rates):
            out[hotkey]["rate"] = _i96f32_float(rate)
    return out


async def collect_samples(cache_path: Path | None) -> list[dict]:
    """Basket state at every grid block: ``{"block", "funds": {hotkey: {nav, price}}}``."""
    cached: dict[int, dict[str, dict]] = {}
    if cache_path and cache_path.exists():
        for point in json.loads(cache_path.read_text())["points"]:
            block = int(point["block"])
            # Points cached before rates were sampled are refetched.
            if (block - EPOCH_BLOCK) % SAMPLE_STEP != 0 or "rates" not in point:
                continue
            cached[block] = {
                hotkey: {
                    "nav": int(point["navs"][hotkey]),
                    "price": float(price),
                    "rate": float(point["rates"][hotkey]),
                }
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
                            "rates": {hk: f["rate"] for hk, f in s["funds"].items()},
                        }
                        for s in samples
                    ]
                }
            )
        )
    return samples


Index = list[tuple[int, float]]
Baselines = dict[str, tuple[float, int, float]]


def chain_index(samples: list[dict]) -> tuple[Index, Index, Baselines]:
    """Chained NAV-weighted indexes plus per-fund baselines stamped at first sighting.

    Returns ``(index, tr_index, baselines)``. ``index`` chains bag price
    relatives (mix performance). ``tr_index`` chains staker total-return
    relatives, where a fund's total return since its first sighting is
    ``1 + (rate − rate0) × price`` — exactly the wealth of τ1 staked there
    and never claimed. Each baseline is ``(price_divisor, first_block,
    rate0)``: ``rate0`` is the fund's ``BasketRate`` at first sighting, so
    display yield subtracts the pre-period accumulator (for migrated funds,
    the seeded legacy history that no staker of this period earned).
    """
    index: Index = []
    tr_index: Index = []
    baselines: Baselines = {}
    level = 1.0
    tr_level = 1.0
    previous: dict[str, dict] = {}
    previous_tr: dict[str, float] = {}

    for sample in samples:
        block, funds = sample["block"], sample["funds"]

        # Staker total return of τ1 staked at the fund's first sighting and
        # held unclaimed: 1 + Δrate × current price. A fund not yet stamped
        # is first-sighted this sample, so it starts at exactly 1.0.
        total_returns = {
            hotkey: 1.0
            + max(fund["rate"] - baselines.get(hotkey, (0.0, 0, fund["rate"]))[2], 0.0)
            * fund["price"]
            for hotkey, fund in funds.items()
        }

        if index:
            # NAV-weighted mean of per-fund relatives across funds deep
            # enough to price at both ends; flows don't move it, only returns.
            weighted, weight = 0.0, 0
            tr_weighted = 0.0
            for hotkey, fund in funds.items():
                prior = previous.get(hotkey)
                if not prior or prior["nav"] < MIN_INDEX_NAV_RAO or fund["nav"] < MIN_INDEX_NAV_RAO:
                    continue
                weighted += prior["nav"] * (fund["price"] / prior["price"])
                tr_weighted += prior["nav"] * (total_returns[hotkey] / previous_tr[hotkey])
                weight += prior["nav"]
            if weight:
                level *= weighted / weight
                tr_level *= tr_weighted / weight
        index.append((block, level))
        tr_index.append((block, tr_level))

        for hotkey, fund in funds.items():
            if hotkey not in baselines:
                baselines[hotkey] = (fund["price"] / level, block, fund["rate"])
        previous = funds
        previous_tr = total_returns

    return index, tr_index, baselines


def write_data_module(index: Index, tr_index: Index, baselines: Baselines) -> None:
    lines = [
        '"""Frozen basket index and per-fund display baselines.',
        "",
        "Generated by ``scripts/build_basket_index.py`` — do not edit by hand.",
        "",
        "``INDEX`` is the chained NAV-weighted basket index (level 1.0 at the",
        "epoch block), sampled on a fixed half-daily block grid. ``TR_INDEX``",
        "is the matching staker total-return index: the NAV-weighted wealth",
        "of τ1 of root stake earning each fund's dividends (bag prices only",
        "enter through the value of accrued β). ``BASELINES`` maps each fund",
        "hotkey to ``(baseline, first_block, rate0)``: the divisor that",
        "splices the fund onto the index at its first sighting (so its",
        "display price starts exactly at the index level of that block), and",
        "the fund's ``BasketRate`` at that block (so display yield covers",
        "only the tracked period, not seeded legacy history).",
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
        "TR_INDEX: tuple[tuple[int, float], ...] = (",
        *[f"    ({block}, {level!r})," for block, level in tr_index],
        ")",
        "",
        "BASELINES: dict[str, tuple[float, int, float]] = {",
        *[
            # One field per line, matching ruff format so regeneration
            # leaves a format-clean file.
            line
            for hotkey, (baseline, first_block, rate0) in sorted(
                baselines.items(), key=lambda item: (item[1][1], item[0])
            )
            for line in (
                f'    "{hotkey}": (',
                f"        {baseline!r},",
                f"        {first_block},",
                f"        {rate0!r},",
                "    ),",
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
    index, tr_index, baselines = chain_index(samples)
    write_data_module(index, tr_index, baselines)
    print(
        f"wrote {DATA_MODULE.name}: {len(index)} index points, "
        f"{len(baselines)} baselines, latest level {index[-1][1]:.6f}, "
        f"total-return level {tr_index[-1][1]:.6f}"
    )


if __name__ == "__main__":
    asyncio.run(main())
