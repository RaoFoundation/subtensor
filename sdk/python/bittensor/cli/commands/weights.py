"""`btcli misc weights`: commit-reveal weight commands for subnet validators."""

from __future__ import annotations

import json
from pathlib import Path

import typer

from ...intents import (
    CommitWeightHash,
    CommitWeights,
    RevealWeights,
    SetMechWeightDelegate,
    SetWeights,
)
from ...settings import guide_docs_url
from ..context import AppContext, ctx_of
from ..globals import with_globals, with_tx_globals

app = typer.Typer(
    no_args_is_help=True,
    help=f"Validator weight commands.\n\nGuide: {guide_docs_url('validating')}",
)
delegate_app = typer.Typer(
    no_args_is_help=True,
    help="Authorize a hotkey to submit weights for one validator mechanism.",
)
app.add_typer(delegate_app, name="delegate")


def _parse_int_list(raw: str) -> list[int]:
    return [int(part.strip()) for part in raw.split(",") if part.strip()]


def _parse_float_list(raw: str) -> list[float]:
    return [float(part.strip()) for part in raw.split(",") if part.strip()]


def _weight_input(uids: str | None, weights: str | None, weights_file: Path | None, raw_u16: bool):
    if weights_file is not None:
        if uids is not None or weights is not None:
            raise typer.BadParameter("Use --weights-file or --uids/--weights, not both.")
        try:
            data = json.loads(weights_file.read_text())
        except (OSError, ValueError) as exc:
            raise typer.BadParameter(f"Cannot read weights file: {exc}") from exc
        if not isinstance(data, dict):
            raise typer.BadParameter("Weights file must contain a UID-to-weight JSON object.")
        return None, data
    if uids is None or weights is None:
        raise typer.BadParameter("Provide --weights-file or both --uids and --weights.")
    try:
        return _parse_int_list(uids), (
            _parse_int_list(weights) if raw_u16 else _parse_float_list(weights)
        )
    except ValueError as exc:
        raise typer.BadParameter("Invalid comma-separated UIDs or weights.") from exc


@app.command(
    "set",
    epilog="Example: btcli misc weights set --netuid 1 --uids 0,1,2 --weights 0.5,0.3,0.2",
)
@with_tx_globals
def set_weights(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetWeights.field_help("netuid")),
    uids: str | None = typer.Option(
        None, "--uids", help="Comma-separated miner UIDs, parallel to --weights."
    ),
    weights: str | None = typer.Option(
        None,
        "--weights",
        help="Comma-separated relative weights, parallel to --uids. Clipped to the "
        "subnet's max-weight limit, normalized, and quantized before submission.",
    ),
    raw_u16: bool = typer.Option(
        False, "--raw-u16", help="Preserve exact integer weights (Null consensus only)."
    ),
    weights_file: Path | None = typer.Option(
        None,
        "--weights-file",
        help="JSON object mapping UID to weight; supports full Null rows "
        "(2,500 UIDs with one mechanism, 2500 // mechanism count with more).",
    ),
    mechid: int = typer.Option(0, "--mechid", help=SetWeights.field_help("mechid")),
    version_key: int = typer.Option(0, "--version-key", help=SetWeights.field_help("version_key")),
    for_hotkey: str | None = typer.Option(
        None, "--for-hotkey", help=SetWeights.field_help("for_hotkey")
    ),
):
    """Set validator weights (auto-selects plaintext or commit-reveal).

    Signed by the hotkey, which must be registered on the subnet. Weights are
    conformed to the subnet's hyperparameters, and the submission path
    (plaintext or timelocked commit) follows the subnet's on-chain
    configuration; registration and rate limits are checked before signing.
    """
    parsed_uids, parsed_weights = _weight_input(uids, weights, weights_file, raw_u16)
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        SetWeights(
            netuid=netuid,
            uids=parsed_uids,
            weights=parsed_weights,
            raw_u16=raw_u16,
            mechid=mechid,
            version_key=version_key,
            for_hotkey=app_ctx.resolve_address("for_hotkey", for_hotkey),
        )
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
    uids: str | None = typer.Option(
        None, "--uids", help="Comma-separated miner UIDs, parallel to --weights."
    ),
    weights: str | None = typer.Option(
        None, "--weights", help="Comma-separated relative weights, parallel to --uids."
    ),
    raw_u16: bool = typer.Option(
        False, "--raw-u16", help="Preserve exact integer weights (Null consensus only)."
    ),
    weights_file: Path | None = typer.Option(
        None,
        "--weights-file",
        help="JSON object mapping UID to weight; supports full Null rows "
        "(2,500 UIDs with one mechanism, 2500 // mechanism count with more).",
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
    for_hotkey: str | None = typer.Option(
        None, "--for-hotkey", help=CommitWeights.field_help("for_hotkey")
    ),
):
    """Commit timelock-encrypted weights (forces the commit-reveal path).

    Unlike `weights set`, this always submits a timelocked commit even if the
    subnet runs plaintext weights. The chain auto-reveals the commit at the
    drand reveal round; no manual reveal is needed.
    """
    parsed_uids, parsed_weights = _weight_input(uids, weights, weights_file, raw_u16)
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        CommitWeights(
            netuid=netuid,
            uids=parsed_uids,
            weights=parsed_weights,
            raw_u16=raw_u16,
            mechid=mechid,
            version_key=version_key,
            for_hotkey=app_ctx.resolve_address("for_hotkey", for_hotkey),
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
    mechid: int = typer.Option(0, "--mechid", help=RevealWeights.field_help("mechid")),
    for_hotkey: str | None = typer.Option(
        None, "--for-hotkey", help=RevealWeights.field_help("for_hotkey")
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
            mechid=mechid,
            for_hotkey=app_ctx.resolve_address("for_hotkey", for_hotkey),
        )
    )


@app.command("hash")
@with_tx_globals
def commit_weight_hash(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=CommitWeightHash.field_help("netuid")),
    commit_hash: str = typer.Option(..., "--hash", help=CommitWeightHash.field_help("commit_hash")),
    mechid: int = typer.Option(0, "--mechid", help=CommitWeightHash.field_help("mechid")),
    for_hotkey: str | None = typer.Option(
        None, "--for-hotkey", help=CommitWeightHash.field_help("for_hotkey")
    ),
):
    """Commit an explicit legacy weight hash (timelock users need only `commit`)."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        CommitWeightHash(
            netuid=netuid,
            commit_hash=commit_hash,
            mechid=mechid,
            for_hotkey=app_ctx.resolve_address("for_hotkey", for_hotkey),
        )
    )


@delegate_app.command("set")
@with_tx_globals
def set_delegate(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", help="Subnet containing the signing validator hotkey."
    ),
    delegate: str = typer.Option(
        ..., "--delegate", help="Account authorized to submit weights for this hotkey."
    ),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism on which to authorize the delegate."),
):
    """Authorize a weight delegate for the signing hotkey on one mechanism."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        SetMechWeightDelegate(
            netuid=netuid,
            mechid=mechid,
            delegate=app_ctx.resolve_address("delegate", delegate),
        )
    )


@delegate_app.command("revoke")
@delegate_app.command("clear", hidden=True)
@with_tx_globals
def clear_delegate(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", help="Subnet containing the signing validator hotkey."
    ),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism whose delegate to revoke."),
):
    """Revoke the signing hotkey's weight delegate on one mechanism."""
    ctx_of(ctx).submit(SetMechWeightDelegate(netuid=netuid, mechid=mechid, delegate=None))


@delegate_app.command("show")
@with_globals
def show_delegate(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet containing the validator hotkey."),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism whose delegate to inspect."),
    hotkey: str | None = typer.Option(
        None, "--hotkey", help="Validator hotkey; defaults to the configured wallet hotkey."
    ),
):
    """Show the weight delegate for a validator hotkey and mechanism."""
    app_ctx: AppContext = ctx_of(ctx)
    address = app_ctx.resolve_address("hotkey_ss58", hotkey)
    result = app_ctx.run(
        lambda client: client.read(
            "mech_weight_delegate", netuid=netuid, mechid=mechid, hotkey_ss58=address
        )
    )
    app_ctx.output.detail(
        "mechanism weight delegate",
        {"netuid": netuid, "mechid": mechid, "hotkey": address, "delegate": result},
    )
