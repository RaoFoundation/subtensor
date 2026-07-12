"""`btcli utils`: conversion and connection helpers."""

from __future__ import annotations

import asyncio
import sys
import time

import typer
from rich.console import Console, Group
from rich.live import Live
from rich.spinner import Spinner
from rich.text import Text

from ...balance import Balance
from ...client import Client
from ...settings import NETWORKS, resolve_endpoint
from ..context import AppContext, ctx_of
from ..globals import with_globals
from ..output import GLYPH_FAIL, GLYPH_OK, PASTEL_RED, STYLE_SUCCESS

app = typer.Typer(no_args_is_help=True, help="Utility commands.")


@app.command("convert")
@with_globals
def convert(
    ctx: typer.Context,
    amount: float = typer.Argument(
        ..., help="Amount to convert: a display amount, or a rao value with --rao."
    ),
    from_rao: bool = typer.Option(
        False, "--rao", help="Treat the amount as rao and output the display amount."
    ),
    netuid: int = typer.Option(
        0,
        "--netuid",
        help="Denomination subnet: 0 = TAO (default), else that subnet's alpha token.",
    ),
):
    """Convert between a display amount and rao (TAO when netuid is 0, else subnet alpha)."""
    app_ctx: AppContext = ctx_of(ctx)
    currency = "TAO" if netuid == 0 else f"alpha (netuid {netuid})"
    if from_rao:
        value = app_ctx.output.balance(int(amount), netuid)
        app_ctx.output.detail(
            None,
            {
                "rao": int(amount),
                "amount": value.amount,
                "currency": currency,
                "display": str(value),
            },
        )
    else:
        value = app_ctx.output.balance(Balance.from_amount(amount, netuid).rao, netuid)
        app_ctx.output.detail(
            None,
            {"amount": amount, "currency": currency, "rao": value.rao, "display": str(value)},
        )


@app.command("latency")
@with_globals
def latency(
    ctx: typer.Context,
    networks: str = typer.Option(
        "",
        "--networks",
        help="Comma-separated network preset names to test. Defaults to all presets.",
    ),
):
    """Measure websocket connection latency to network endpoints."""
    app_ctx: AppContext = ctx_of(ctx)
    names = [n.strip() for n in networks.split(",") if n.strip()] or sorted(NETWORKS)
    targets = [resolve_endpoint(name) for name in names]
    label_width = max(len(label) for label, _ in targets)

    started: dict[str, float] = {}
    finished: dict[str, dict] = {}

    async def _probe(label: str, endpoint: str) -> dict:
        started[label] = time.perf_counter()
        try:
            async with Client(label) as client:
                await client.block()
            ms = (time.perf_counter() - started[label]) * 1000
            result = {
                "network": label,
                "endpoint": endpoint,
                "latency_ms": round(ms, 1),
                "ok": True,
            }
        except Exception as error:
            ms = (time.perf_counter() - started[label]) * 1000
            result = {
                "network": label,
                "endpoint": endpoint,
                "latency_ms": round(ms, 1),
                "ok": False,
                "error": str(error),
            }
        finished[label] = result
        return result

    async def _all():
        return await asyncio.gather(*[_probe(label, endpoint) for label, endpoint in targets])

    def _live_view() -> Group:
        """One line per endpoint: spinner + ticking elapsed while probing,
        state glyph + result once done (uv-style concurrent progress)."""
        lines: list = []
        for label, _endpoint in targets:
            result = finished.get(label)
            if result is None:
                elapsed = (time.perf_counter() - started.get(label, time.perf_counter())) * 1000
                text = Text()
                text.append(label.ljust(label_width))
                text.append(f"  {elapsed:,.0f} ms", style="dim")
                lines.append(Spinner("dots", text=text, style="dim"))
                continue
            line = Text()
            if result["ok"]:
                line.append(f"{GLYPH_OK} ", style=STYLE_SUCCESS)
                line.append(label.ljust(label_width))
                line.append(f"  {result['latency_ms']:,.1f} ms", style="dim")
            else:
                line.append(f"{GLYPH_FAIL} ", style=PASTEL_RED)
                line.append(label.ljust(label_width))
                line.append("  unreachable", style="dim")
            lines.append(line)
        return Group(*lines)

    animate = not app_ctx.output.json_mode and not app_ctx.output.quiet and sys.stderr.isatty()
    if animate:
        # Progress is chatter, so it lives on stderr and clears itself
        # (transient) once the real output below takes over.
        with Live(
            get_renderable=_live_view,
            console=Console(stderr=True, highlight=False),
            refresh_per_second=12.5,
            transient=True,
        ):
            results = asyncio.run(_all())
    else:
        results = asyncio.run(_all())

    results.sort(key=lambda r: (not r["ok"], r["latency_ms"]))
    rows = [
        [
            r["network"],
            f"{r['latency_ms']:,.1f}" if r["ok"] else "—",
            r["endpoint"],
            GLYPH_OK if r["ok"] else f"{GLYPH_FAIL} {r.get('error') or 'unreachable'}",
        ]
        for r in results
    ]
    reachable = [r for r in results if r["ok"]]
    if reachable:
        fastest = reachable[0]
        footer = (
            f"[bold]fastest[/bold] {fastest['network']}  "
            f"[dim]{fastest['latency_ms']:,.1f} ms · "
            f"{len(reachable)}/{len(results)} reachable[/dim]"
        )
    else:
        footer = f"[dim]0/{len(results)} reachable[/dim]"
    app_ctx.output.columns(
        "latency (websocket connect + one block query)",
        ["network", "ms", "endpoint", "status"],
        rows,
        results,
        right_align={1},
        footer=footer,
    )
