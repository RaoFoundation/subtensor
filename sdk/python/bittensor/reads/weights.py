"""Weight and bond matrix reads."""

from __future__ import annotations

from .. import timelock
from .._generated import storage as st
from ..hyperparams import ratio_fraction
from ..settings import GLOBAL_MAX_SUBNET_COUNT
from .base import Grouped, Matrix, read


def _mechanism_index(netuid: int, mechid: int) -> int:
    """Weights, bonds, and weight commits are stored under the mechanism index
    (``mechid * GLOBAL_MAX_SUBNET_COUNT + netuid``), which equals the netuid for
    mechanism 0 — the only mechanism on ordinary subnets."""
    return mechid * GLOBAL_MAX_SUBNET_COUNT + netuid


_MECHID_DOC = "Mechanism index within the subnet; 0 (the default) on ordinary subnets."


@read(
    "weights",
    {"netuid": "integer", "mechid": "integer"},
    category="Weights & bonds",
    param_docs={
        "netuid": "Subnet whose weight matrix to fetch.",
        "mechid": _MECHID_DOC,
    },
    render=Matrix("validator", "miner", "weight"),
)
async def weights(view, netuid: int, mechid: int = 0) -> dict[int, dict[int, float]]:
    """Validator weight rows as ``{validator_uid: {miner_uid: fraction}}``, each row summing to 1.

    The chain stores max-upscaled u16 values whose absolute scale carries no
    meaning — consensus only uses within-row proportions — so rows are
    normalized to fractions here. On commit-reveal subnets a validator's row
    updates only when its timelocked commit reveals (see
    `timelocked_weight_commits` for what is pending).
    """
    rows = await view.query_map(st.SubtensorModule.Weights, [_mechanism_index(netuid, mechid)])
    out: dict[int, dict[int, float]] = {}
    for uid, row in rows:
        total = sum(int(value) for _, value in row or [])
        out[int(uid)] = {int(target): int(value) / total for target, value in row} if total else {}
    return dict(sorted(out.items()))


@read(
    "bonds",
    {"netuid": "integer", "mechid": "integer"},
    category="Weights & bonds",
    param_docs={
        "netuid": "Subnet whose bond matrix to fetch.",
        "mechid": _MECHID_DOC,
    },
    render=Matrix("validator", "miner", "bond"),
)
async def bonds(view, netuid: int, mechid: int = 0) -> dict[int, dict[int, float]]:
    """Validator bond rows as ``{validator_uid: {miner_uid: bond}}``, scaled to 0..1.

    Bonds are the slow EMA of a validator's stake-weighted weights that pays
    its dividends; unlike weights their magnitude is meaningful, so values are
    scaled by the u16 maximum (1.0 = the largest bond the chain can store)
    rather than row-normalized.
    """
    rows = await view.query_map(st.SubtensorModule.Bonds, [_mechanism_index(netuid, mechid)])
    # Matrix elements, so the storage descriptor's value identity is the row
    # type, not the element's; each element is a u16 fraction over 65535
    # (PerU16 semantics).
    return {
        int(uid): {
            int(target): ratio_fraction("PerU16", int(value)) or 0.0 for target, value in row or []
        }
        for uid, row in sorted(rows, key=lambda kv: int(kv[0]))
    }


@read(
    "timelocked_weight_commits",
    {"netuid": "integer", "mechid": "integer"},
    category="Weights & bonds",
    param_docs={
        "netuid": "Subnet whose pending weight commits to list.",
        "mechid": _MECHID_DOC,
    },
    render=Grouped("epoch"),
)
async def timelocked_weight_commits(view, netuid: int, mechid: int = 0) -> dict[int, list[dict]]:
    """Pending (still-encrypted) commit-reveal weight commits, grouped by epoch.

    Each entry carries the committing `hotkey`, the `commit_block`, the drand
    `reveal_round` at which the chain auto-decrypts and applies it, `reveals_at`
    (that round's UTC time, computed locally), and the `ciphertext`. Entries
    disappear from this map once revealed — revealed weights show up in the
    `weights` read.
    """
    rows = await view.query_map(
        st.SubtensorModule.TimelockedWeightCommits, [_mechanism_index(netuid, mechid)]
    )
    out: dict[int, list[dict]] = {}
    for epoch, commits in rows:
        out[int(epoch)] = [
            {
                "hotkey": str(hotkey),
                "commit_block": int(commit_block),
                "reveal_round": int(reveal_round),
                "reveals_at": timelock.reveal_time(int(reveal_round)),
                "ciphertext": str(ciphertext),
            }
            for hotkey, commit_block, ciphertext, reveal_round in commits or []
        ]
    return dict(sorted(out.items()))
