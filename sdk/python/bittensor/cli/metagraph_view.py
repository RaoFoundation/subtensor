"""Metagraph view: the raw per-uid arrays reshaped for humans.

Subnet-level data renders as aligned key/value sections (raw chain values
primary, dim readings beside them — the hyperparameters convention); the
per-uid arrays collapse into one tree branch per neuron, largest stake first,
hotkeys labeled with their local wallet name when one exists, else their
on-chain identity name. JSON carries the runtime record as the read returns
it (untouched except `name`/`symbol`, which the read decodes to text).
"""

from __future__ import annotations

import ipaddress
from typing import Any, Optional

import typer

from .. import hyperparams as hp
from ..balance import Balance
from ..settings import U16_MAX
from .context import AppContext
from .helpers import local_address_names

# I96F32 fixed point: 32 fractional bits.
_I96F32_ONE = 2**32


def show_metagraph(app_ctx: AppContext, netuid: int, graph: Any) -> None:
    if not isinstance(graph, dict):
        app_ctx.output.error(f"subnet {netuid} does not exist")
        raise typer.Exit(1)
    sections = _sections(app_ctx, graph, netuid)
    neurons = _neurons(app_ctx, graph, netuid)
    num_uids = int(graph.get("num_uids") or len(neurons))
    max_uids = int(graph.get("max_uids") or 0)
    validators = sum(1 for n in neurons if n["validator"])
    active = sum(1 for n in neurons if n["active"])
    app_ctx.output.metagraph(
        f"metagraph netuid {netuid}",
        sections,
        f"neurons {num_uids}/{max_uids}" if max_uids else f"neurons {num_uids}",
        neurons,
        graph,
        f"{num_uids} uids · {validators} validator permits · {active} active",
        hint=(
            "`--json` carries every per-uid field (trust, consensus, rank, axons); "
            f"`btcli subnets hyperparameters {netuid}` explains each parameter"
        ),
    )


Row = tuple[str, str, Optional[str]]


def _sections(
    app_ctx: AppContext, graph: dict, netuid: int
) -> list[tuple[Optional[str], list[Row]]]:
    def tao(key: str) -> str:
        return str(Balance(int(graph[key])))

    def alpha(key: str) -> str:
        return str(app_ctx.output.balance(int(graph[key]), netuid))

    def raw(key: str) -> Row:
        value = graph[key]
        return key, str(value), hp.annotate(key, value)

    block = int(graph["block"])

    overview: list[Row] = [
        ("owner_hotkey", str(graph["owner_hotkey"]), None),
        ("owner_coldkey", str(graph["owner_coldkey"]), None),
        ("registered_at", str(graph["network_registered_at"]), None),
        ("block", str(block), None),
        raw("tempo"),
        (
            "last_step",
            str(graph["last_step"]),
            f"{graph['blocks_since_last_step']} blocks ago",
        ),
    ]

    identity = graph.get("identity")
    identity_rows: list[Row] = (
        [(key, str(value), None) for key, value in identity.items() if value]
        if isinstance(identity, dict)
        else []
    )

    tao_in = int(graph["tao_in"])
    alpha_in = int(graph["alpha_in"])
    pool: list[Row] = []
    if alpha_in:
        pool.append(("price", f"{tao_in / alpha_in:.6f}", "τ per alpha (spot)"))
    bits = (graph.get("moving_price") or {}).get("bits")
    if bits is not None:
        pool.append(("moving_price", f"{int(bits) / _I96F32_ONE:.6f}", None))
    pool += [
        ("tao_in", tao("tao_in"), None),
        ("alpha_in", alpha("alpha_in"), None),
        ("alpha_out", alpha("alpha_out"), None),
        ("volume", tao("subnet_volume"), None),
        ("subnet_emission", tao("subnet_emission"), "per block"),
        ("tao_in_emission", tao("tao_in_emission"), "per block"),
        ("alpha_in_emission", alpha("alpha_in_emission"), "per block"),
        ("alpha_out_emission", alpha("alpha_out_emission"), "per block"),
        ("pending_alpha_emission", alpha("pending_alpha_emission"), "undistributed"),
        ("pending_root_emission", tao("pending_root_emission"), "undistributed"),
    ]

    registration: list[Row] = [
        raw("registration_allowed"),
        raw("pow_registration_allowed"),
        ("burn", str(graph["burn"]), f"= {tao('burn')}"),
        ("min_burn", str(graph["min_burn"]), f"= {tao('min_burn')}"),
        ("max_burn", str(graph["max_burn"]), f"= {tao('max_burn')}"),
        raw("difficulty"),
        raw("min_difficulty"),
        raw("max_difficulty"),
        raw("immunity_period"),
        raw("adjustment_interval"),
        raw("adjustment_alpha"),
        raw("target_regs_per_interval"),
        raw("max_regs_per_block"),
        raw("serving_rate_limit"),
    ]

    weights: list[Row] = [
        raw("rho"),
        raw("kappa"),
        raw("min_allowed_weights"),
        raw("max_weights_limit"),
        raw("weights_version"),
        raw("weights_rate_limit"),
        raw("activity_cutoff"),
        raw("max_validators"),
        raw("commit_reveal_weights_enabled"),
        raw("commit_reveal_period"),
        raw("liquid_alpha_enabled"),
        raw("alpha_high"),
        raw("alpha_low"),
        raw("bonds_moving_avg"),
    ]

    return [
        (None, overview),
        ("identity", identity_rows),
        ("pool & emission", pool),
        ("registration", registration),
        ("weights & bonds", weights),
    ]


def _neurons(app_ctx: AppContext, graph: dict, netuid: int) -> list[dict[str, Any]]:
    hotkeys = [str(hk) for hk in graph.get("hotkeys") or []]
    if not hotkeys:
        return []
    names = local_address_names(app_ctx.wallet_path)
    block = int(graph["block"])

    def column(key: str) -> list:
        values = graph.get(key) or []
        return list(values) + [None] * (len(hotkeys) - len(values))

    identities = column("identities")
    axons = column("axons")
    active = column("active")
    permits = column("validator_permit")
    last_update = column("last_update")
    emission = column("emission")
    dividends = column("dividends")
    incentives = column("incentives")
    total_stake = column("total_stake")

    neurons: list[dict[str, Any]] = []
    for uid, hotkey in enumerate(hotkeys):
        identity = identities[uid] if isinstance(identities[uid], dict) else {}
        identity_name = str(identity.get("name") or "") or None
        updated = int(last_update[uid] or 0)
        neurons.append(
            {
                "uid": uid,
                "hotkey": hotkey,
                "label": names.get(hotkey) or identity_name or hotkey,
                "named": hotkey in names,
                "identity": hotkey not in names and identity_name is not None,
                "stake": Balance(int(total_stake[uid] or 0)),
                "emission": app_ctx.output.balance(int(emission[uid] or 0), netuid),
                "incentive": int(incentives[uid] or 0) / U16_MAX,
                "dividends": int(dividends[uid] or 0) / U16_MAX,
                "validator": bool(permits[uid]),
                "active": bool(active[uid]),
                "updated": block - updated if updated else None,
                "axon": _axon_endpoint(axons[uid]),
            }
        )
    neurons.sort(key=lambda n: -n["stake"].rao)
    return neurons


def _axon_endpoint(axon: Any) -> Optional[str]:
    """``ip:port`` for a served axon, or None when nothing is served."""
    if not isinstance(axon, dict):
        return None
    ip = int(axon.get("ip") or 0)
    if not ip:
        return None
    port = int(axon.get("port") or 0)
    if int(axon.get("ip_type") or 4) == 6:
        return f"[{ipaddress.IPv6Address(ip)}]:{port}"
    return f"{ipaddress.IPv4Address(ip)}:{port}"
