"""`btcli axon`: neuron serving endpoints."""

from __future__ import annotations

import typer

from ...intents import ResetAxon, ServeAxon, ServeAxonTls
from ..context import AppContext, ctx_of
from ..globals import with_tx_globals

app = typer.Typer(no_args_is_help=True, help="Axon serving commands.")


@app.command("set")
@with_tx_globals
def axon_set(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ...,
        "--netuid",
        help=ServeAxon.field_help("netuid") or "Subnet to publish the endpoint on.",
    ),
    ip: str = typer.Option(
        ...,
        "--ip",
        help=ServeAxon.field_help("ip") or "Public IPv4 or IPv6 address of the axon.",
    ),
    port: int = typer.Option(
        ..., "--port", help=ServeAxon.field_help("port") or "Port the axon listens on."
    ),
    tls: bool = typer.Option(
        False, "--tls", help="Also publish a TLS certificate for inter-neuron TLS."
    ),
    certificate: str = typer.Option(
        "",
        "--certificate",
        help=ServeAxonTls.field_help("certificate") or "0x-hex TLS certificate bytes (with --tls).",
    ),
):
    """Publish an axon endpoint for the wallet hotkey.

    This records connection info as chain data only; it does not start a
    server. Signed by the hotkey, which must be registered on the subnet.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if tls:
        app_ctx.submit(ServeAxonTls(netuid=netuid, ip=ip, port=port, certificate=certificate))
    else:
        app_ctx.submit(ServeAxon(netuid=netuid, ip=ip, port=port))


@app.command("reset")
@with_tx_globals
def axon_reset(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ...,
        "--netuid",
        help=ResetAxon.field_help("netuid") or "Subnet to stop serving on.",
    ),
):
    """Reset (stop serving) the wallet hotkey's axon on a subnet."""
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(ResetAxon(netuid=netuid))
