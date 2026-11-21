"""`btcli commitments`: independent hotkey channels and their burn prices."""

from __future__ import annotations

from dataclasses import asdict, is_dataclass

import typer

from ...intents import SetChannelCommitment, SetCommitBurn
from ...settings import guide_docs_url
from ..context import AppContext, ctx_of
from ..globals import with_globals, with_tx_globals
from ..output import STYLE_WARNING

app = typer.Typer(
    no_args_is_help=True,
    help="Publish independent channel commitments.\n\n"
    f"Guide: {guide_docs_url('channel-commitments')}",
)


@app.command("commit")
@with_tx_globals
def commit(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet containing the target hotkey."),
    data: str = typer.Option(
        ..., "--data", help="UTF-8 text, hex with --hex, or empty text to clear."
    ),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism whose channel to publish on."),
    channel: int = typer.Option(0, "--channel", help="Independent channel index (0..255)."),
    hotkey: str | None = typer.Option(
        None, "--hotkey", help="Hotkey publishing the commitment; defaults to the wallet hotkey."
    ),
    max_burn: str = typer.Option(
        "0", "--max-burn", help="Maximum current TAO burn accepted (exact decimal)."
    ),
    hex_data: bool = typer.Option(
        False, "--hex", help="Interpret --data as hexadecimal instead of UTF-8."
    ),
    reveal_round: int | None = typer.Option(
        None, "--reveal-round", help="Future drand round for a timelocked commitment."
    ),
):
    """Publish a channel commitment signed and paid for by the coldkey."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        SetChannelCommitment(
            netuid=netuid,
            mechid=mechid,
            channel=channel,
            hotkey_ss58=app_ctx.resolve_address("hotkey_ss58", hotkey),
            data=data,
            encoding="hex" if hex_data else "utf8",
            max_burn_tao=max_burn,
            reveal_round=reveal_round,
        )
    )


@app.command("price")
@with_globals
def price(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet whose commitment channel to inspect."),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism whose channel to inspect."),
    channel: int = typer.Option(0, "--channel", help="Independent channel index (0..255)."),
):
    """Show the live decayed burn price and stored channel configuration."""
    app_ctx: AppContext = ctx_of(ctx)
    result = app_ctx.run(
        lambda client: client.read("commit_burn", netuid=netuid, mechid=mechid, channel=channel)
    )
    app_ctx.output.detail("channel commitment burn", result or {})
    if result is not None and not result["price_is_decayed"]:
        app_ctx.output.message(
            f"[{STYLE_WARNING}]warning:[/] burn is disabled or absent at the runtime quote; "
            "stored parameters are shown. Stored prices can be stale because decay is lazy."
        )


@app.command("show")
@with_globals
def show(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet containing the target hotkey."),
    hotkey: str | None = typer.Option(
        None, "--hotkey", help="Hotkey whose commitment to show; defaults to the wallet hotkey."
    ),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism whose channel to inspect."),
    channel: int = typer.Option(0, "--channel", help="Independent channel index (0..255)."),
):
    """Show a hotkey's independent channel commitment."""
    app_ctx: AppContext = ctx_of(ctx)
    address = app_ctx.resolve_address("hotkey_ss58", hotkey)
    result = app_ctx.run(
        lambda client: client.read(
            "channel_commitment",
            netuid=netuid,
            mechid=mechid,
            channel=channel,
            hotkey_ss58=address,
        )
    )
    app_ctx.output.detail(
        "channel commitment", asdict(result) if is_dataclass(result) else result or {}
    )


@app.command("burn-config")
@with_tx_globals
def burn_config(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet owned by the signing coldkey."),
    name: str = typer.Option(..., "--name", help=SetCommitBurn.field_help("name")),
    value: str = typer.Option(
        ..., "--value", help="Enabled flag, TAO min/max, half-life in blocks, or decimal in 1..3."
    ),
    mechid: int = typer.Option(0, "--mechid", help="Mechanism whose channel to configure."),
    channel: int = typer.Option(0, "--channel", help="Independent channel index (0..255)."),
):
    """Set one owner-controlled commitment burn parameter."""
    ctx_of(ctx).submit(
        SetCommitBurn(netuid=netuid, name=name, value=value, mechid=mechid, channel=channel)
    )
