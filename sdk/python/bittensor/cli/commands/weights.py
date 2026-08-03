"""`btcli weights`: commit-reveal weight commands and root dividend weights."""

from __future__ import annotations

from typing import Optional

import typer

from ...intents import CommitWeights, RevealWeights, SetRootWeights, SetWeights
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals

app = typer.Typer(no_args_is_help=True, help="Validator weight commands.")


def _parse_int_list(raw: str) -> list[int]:
    return [int(part.strip()) for part in raw.split(",") if part.strip()]


def _parse_float_list(raw: str) -> list[float]:
    return [float(part.strip()) for part in raw.split(",") if part.strip()]


def _parse_weight_pairs(raw: str) -> dict[int, float]:
    """Parse `netuid:weight` pairs: `"0:0.2,4:0.3,8:0.5"`."""
    pairs: dict[int, float] = {}
    for part in raw.split(","):
        part = part.strip()
        if not part:
            continue
        netuid_text, _, weight_text = part.partition(":")
        if not weight_text:
            raise typer.BadParameter(
                f"expected netuid:weight pairs like '0:0.2,4:0.3', got {part!r}"
            )
        pairs[int(netuid_text.strip())] = float(weight_text.strip())
    if not pairs:
        raise typer.BadParameter("no netuid:weight pairs given")
    return pairs


@app.command(
    "set",
    epilog="Example: btcli weights set --netuid 1 --uids 0,1,2 --weights 0.5,0.3,0.2",
)
@with_tx_globals
def set_weights(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetWeights.field_help("netuid")),
    uids: str = typer.Option(
        ..., "--uids", help="Comma-separated miner UIDs, parallel to --weights."
    ),
    weights: str = typer.Option(
        ...,
        "--weights",
        help="Comma-separated relative weights, parallel to --uids. Clipped to the "
        "subnet's max-weight limit, normalized, and quantized before submission.",
    ),
    mechid: int = typer.Option(0, "--mechid", help=SetWeights.field_help("mechid")),
    version_key: int = typer.Option(0, "--version-key", help=SetWeights.field_help("version_key")),
):
    """Set validator weights (auto-selects plaintext or commit-reveal).

    Signed by the hotkey, which must be registered on the subnet. Weights are
    conformed to the subnet's hyperparameters, and the submission path
    (plaintext or timelocked commit) follows the subnet's on-chain
    configuration; registration and rate limits are checked before signing.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        SetWeights(
            netuid=netuid,
            uids=_parse_int_list(uids),
            weights=_parse_float_list(weights),
            mechid=mechid,
            version_key=version_key,
        )
    )


@app.command(
    "set-root",
    hidden=True,
    epilog='Moved to `btcli root set-weights`. Example: --weights "0:0.2,4:0.3,8:0.5"',
)
@with_tx_globals
def set_root_weights(
    ctx: typer.Context,
    weights: str = typer.Option(
        ...,
        "--weights",
        help="Comma-separated netuid:weight pairs (e.g. '0:0.2,4:0.3,8:0.5'). "
        "Weights are relative and normalized before submission; netuid 0 means "
        "hold that share as TAO (root stake) instead of subnet alpha.",
    ),
):
    """Deprecated: use ``btcli root set-weights``."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.message("[dim]deprecated: use `btcli root set-weights`[/dim]")
    pairs = _parse_weight_pairs(weights)
    app_ctx.submit(
        SetRootWeights(
            netuids=sorted(pairs),
            weights=[pairs[netuid] for netuid in sorted(pairs)],
        )
    )


@app.command("get-root", hidden=True)
@with_globals
def get_root_weights(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Deprecated: use ``btcli root get-weights``."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.output.message("[dim]deprecated: use `btcli root get-weights`[/dim]")
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    rows = app_ctx.run(lambda c: c.read("validator_root_weights", hotkey_ss58=hotkey))
    if not rows:
        app_ctx.output.detail("root weights", {"hotkey": hotkey, "weights": []})
        app_ctx.output.message(
            "no custom root weights set: dividends accumulate in place on their origin subnet"
        )
        return
    table_rows = [[r["netuid"], f"{r['share']:.2%}", r["weight"]] for r in rows]
    app_ctx.output.table(
        f"root weights of {hotkey}", ["netuid", "share", "weight (u16)"], table_rows, rows
    )


@app.command("commit")
@with_tx_globals
def commit_weights(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ...,
        "--netuid",
        help=CommitWeights.field_help("netuid") or "Subnet whose miners the weights score.",
    ),
    uids: str = typer.Option(
        ..., "--uids", help="Comma-separated miner UIDs, parallel to --weights."
    ),
    weights: str = typer.Option(
        ..., "--weights", help="Comma-separated relative weights, parallel to --uids."
    ),
    mechid: int = typer.Option(
        0,
        "--mechid",
        help=CommitWeights.field_help("mechid")
        or "Mechanism index within the subnet; 0 is the default.",
    ),
    version_key: int = typer.Option(
        0,
        "--version-key",
        help=CommitWeights.field_help("version_key")
        or "Weights version key; leave 0 unless the subnet owner requires a value.",
    ),
):
    """Commit timelock-encrypted weights (forces the commit-reveal path).

    Unlike `weights set`, this always submits a timelocked commit even if the
    subnet runs plaintext weights. The chain auto-reveals the commit at the
    drand reveal round; no manual reveal is needed.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        CommitWeights(
            netuid=netuid,
            uids=_parse_int_list(uids),
            weights=_parse_float_list(weights),
            mechid=mechid,
            version_key=version_key,
        )
    )


@app.command("reveal")
@with_tx_globals
def reveal_weights(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ...,
        "--netuid",
        help=RevealWeights.field_help("netuid") or "Subnet the commit was made on.",
    ),
    uids: str = typer.Option(
        ..., "--uids", help="Comma-separated miner UIDs, exactly as committed."
    ),
    weights: str = typer.Option(
        ..., "--weights", help="Comma-separated weights, exactly as committed."
    ),
    salt: str = typer.Option(
        ..., "--salt", help="Comma-separated salt values used at commit time."
    ),
    version_key: int = typer.Option(
        0,
        "--version-key",
        help=RevealWeights.field_help("version_key") or "Weights version key used at commit time.",
    ),
):
    """Reveal previously committed weights.

    Legacy salt-based commit-reveal: the uids, weights, salt, and version key
    must reproduce the earlier commit exactly or the reveal fails. Timelocked
    commits made by `weights set`/`weights commit` reveal automatically and do
    not need this command.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        RevealWeights(
            netuid=netuid,
            uids=_parse_int_list(uids),
            weights=_parse_float_list(weights),
            salt=_parse_int_list(salt),
            version_key=version_key,
        )
    )
