"""`btcli crowd`: crowdloan commands."""

from __future__ import annotations

import typer

from ..context import AppContext, ctx_of
from ..globals import with_globals

app = typer.Typer(no_args_is_help=True, help="Crowdloan queries (mutations use `btcli tx`).")


@app.command("list")
@with_globals
def list_crowdloans(ctx: typer.Context):
    """List all crowdloans."""
    app_ctx: AppContext = ctx_of(ctx)
    rows = app_ctx.run(lambda c: c.read("crowdloans"))
    if not rows:
        app_ctx.output.detail("crowdloans", {"count": 0})
        return
    table_rows = [
        [r["id"], r["creator"], r["raised_tao"], r["cap_tao"], r["finalized"]] for r in rows
    ]
    app_ctx.output.table(
        "crowdloans", ["id", "creator", "raised", "cap", "finalized"], table_rows, rows
    )


@app.command("info")
@with_globals
def crowdloan_info(
    ctx: typer.Context,
    crowdloan_id: int = typer.Argument(..., help="Crowdloan ID."),
):
    """Show details for one crowdloan."""
    app_ctx: AppContext = ctx_of(ctx)
    info = app_ctx.run(lambda c: c.read("crowdloan", crowdloan_id=crowdloan_id))
    if info is None:
        app_ctx.output.error(f"crowdloan {crowdloan_id} not found")
        raise typer.Exit(1)
    app_ctx.output.detail(f"crowdloan {crowdloan_id}", info)


@app.command("contributors")
@with_globals
def crowdloan_contributors(
    ctx: typer.Context,
    crowdloan_id: int = typer.Argument(..., help="Crowdloan ID."),
):
    """List contributors to a crowdloan."""
    app_ctx: AppContext = ctx_of(ctx)
    rows = app_ctx.run(lambda c: c.read("crowdloan_contributors", crowdloan_id=crowdloan_id))
    if not rows:
        app_ctx.output.detail("contributors", {"crowdloan_id": crowdloan_id, "contributors": []})
        return
    table_rows = [[r["contributor"], r["amount_tao"]] for r in rows]
    app_ctx.output.table("contributors", ["contributor", "amount_tao"], table_rows, rows)
