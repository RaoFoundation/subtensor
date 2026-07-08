"""`btcli proxy`: on-chain proxy account management and the local proxy book."""

from __future__ import annotations

import json
from typing import Any, Optional

import typer

from ... import config as cfg
from ...intents import (
    AddProxy,
    CreatePureProxy,
    ExecuteProxyAnnounced,
    KillPureProxy,
    RemoveProxies,
    RemoveProxy,
)
from ...intents.proxy import DELAY_HELP, PROXY_TYPE_HELP, ProxyTypeChoice
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..helpers import list_coldkeys
from ..prompt import interactive

app = typer.Typer(no_args_is_help=True, help="On-chain proxy management and the local proxy book.")

book_app = typer.Typer(
    no_args_is_help=True,
    help="Local proxy address book (named pure/delegate proxies).",
)


def _proxy_type_option() -> Any:
    """A fresh --proxy-type option per command (Typer reads these per-command)."""
    return typer.Option("Staking", "--proxy-type", help=PROXY_TYPE_HELP)


def _register_book_names(app_ctx: AppContext) -> None:
    """Teach the renderer local wallet, address-book, and proxy-book names."""
    for name, ss58 in list_coldkeys(app_ctx.wallet_path):
        app_ctx.output.name_address(ss58, name)
        app_ctx.output.classify_address(ss58, "coldkey")
    for entry in cfg.load_addresses() + cfg.load_proxies():
        app_ctx.output.name_address(entry.get("address"), entry.get("name"))


@app.command("create")
@with_tx_globals
def create_proxy(
    ctx: typer.Context,
    proxy_type: ProxyTypeChoice = _proxy_type_option(),
    delay: int = typer.Option(0, "--delay", help=DELAY_HELP),
    index: int = typer.Option(0, "--index"),
    name: Optional[str] = typer.Option(
        None,
        "--name",
        help="Save the spawned pure proxy to the local proxy book under this name "
        "(records the address plus the creation height/ext-index that `kill` needs).",
    ),
):
    """Create a pure proxy account.

    Spawns a fresh keyless account with your coldkey registered as its proxy.
    The spawned address (from the chain's PureCreated event) and the creation
    block height / extrinsic index are shown in the result; pass --name to
    record them in the local proxy book so `--proxy-for NAME` and
    `btcli proxy kill NAME` work later.
    """
    app_ctx: AppContext = ctx_of(ctx)
    result = app_ctx.submit(CreatePureProxy(proxy_type=proxy_type.value, delay=delay, index=index))
    if result is None or not result.success:
        return
    pure = result.data.get("pure_proxy")
    if not pure:
        if name:
            app_ctx.output.message(
                "could not read the spawned address from the PureCreated event — "
                "add it manually with `btcli proxy book add`"
            )
        return
    if name:
        entry: dict[str, Any] = {
            "name": name,
            "address": pure,
            "spawner": result.data.get("spawner", ""),
            "proxy_type": proxy_type.value,
            "delay": delay,
            "index": index,
            "note": "",
        }
        for key in ("height", "ext_index"):
            if result.data.get(key) is not None:
                entry[key] = result.data[key]
        cfg.add_proxy(entry)
        app_ctx.output.message(
            f"saved to proxy book as {name!r} ({cfg.proxies_path()}) — "
            f"use it with `--proxy-for {name}`"
        )
    else:
        app_ctx.output.message(
            "[dim]tip: save this pure proxy for later with "
            f"`btcli proxy book add --name NAME --address {pure}` "
            "(or pass --name on create to do it automatically)[/dim]"
        )


@app.command("add")
@with_tx_globals
def add_proxy(
    ctx: typer.Context,
    delegate_ss58: str = typer.Option(
        ..., address_cli_name("delegate_ss58"), help=ss58_param_help("delegate_ss58")
    ),
    proxy_type: ProxyTypeChoice = _proxy_type_option(),
    delay: int = typer.Option(0, "--delay", help=DELAY_HELP),
):
    """Add a proxy delegate."""
    app_ctx: AppContext = ctx_of(ctx)
    delegate = app_ctx.resolve_address("coldkey_ss58", delegate_ss58)
    app_ctx.submit(AddProxy(delegate_ss58=delegate, proxy_type=proxy_type.value, delay=delay))


@app.command("remove")
@with_tx_globals
def remove_proxy(
    ctx: typer.Context,
    delegate_ss58: Optional[str] = typer.Option(
        None, address_cli_name("delegate_ss58"), help=ss58_param_help("delegate_ss58")
    ),
    proxy_type: ProxyTypeChoice = _proxy_type_option(),
    delay: int = typer.Option(0, "--delay", help=DELAY_HELP),
    all_proxies: bool = typer.Option(False, "--all", help="Remove every proxy at once."),
):
    """Remove a proxy delegate."""
    app_ctx: AppContext = ctx_of(ctx)
    if all_proxies:
        # Removing every delegation strands pure proxies forever (nobody holds
        # their keys), so this asks for typed confirmation on top of the usual
        # y/n prompt. --yes skips both.
        if not app_ctx.assume_yes and interactive(app_ctx):
            app_ctx.output.message(
                "[bold]removing ALL proxies[/bold]: any pure proxy spawned by this "
                "account becomes permanently inaccessible, along with its funds"
            )
            answer = typer.prompt('type "remove all" to continue', err=True)
            if answer.strip().lower() != "remove all":
                app_ctx.output.message("aborted.")
                raise typer.Exit(1)
        app_ctx.submit(RemoveProxies())
        return
    if delegate_ss58 is None:
        app_ctx.output.error(
            "missing required option: `--delegate`",
            help="pass the delegate to revoke, or `--all` to remove every proxy",
        )
        raise typer.Exit(2)
    delegate = app_ctx.resolve_address("coldkey_ss58", delegate_ss58)
    app_ctx.submit(RemoveProxy(delegate_ss58=delegate, proxy_type=proxy_type.value, delay=delay))


@app.command("kill")
@with_tx_globals
def kill_proxy(
    ctx: typer.Context,
    name: Optional[str] = typer.Argument(
        None,
        help="Proxy book entry to kill (fills the spawner, type, index, height, and "
        "ext-index recorded at create time). Flags override individual values.",
    ),
    spawner_ss58: Optional[str] = typer.Option(
        None, address_cli_name("spawner_ss58"), help=ss58_param_help("spawner_ss58")
    ),
    proxy_type: Optional[ProxyTypeChoice] = typer.Option(
        None, "--proxy-type", help="Type the pure proxy was created with (must match exactly)."
    ),
    index: Optional[int] = typer.Option(
        None, "--index", help="Index the pure proxy was created with (must match exactly)."
    ),
    height: Optional[int] = typer.Option(
        None, "--height", help="Block number of the creating create_pure_proxy call."
    ),
    ext_index: Optional[int] = typer.Option(
        None, "--ext-index", help="Extrinsic index of the creating call within that block."
    ),
    proxy_for: Optional[str] = typer.Option(
        None,
        "--proxy-for",
        help="Pure proxy account to dispatch the kill through (the chain requires the "
        "pure account itself as origin). Defaults to the book entry's address.",
    ),
):
    """Kill a pure proxy account.

    Irreversible: any funds left in the account become permanently
    inaccessible, so empty it first. The chain requires the exact creation
    parameters (spawner, type, index, block height, extrinsic index) — with a
    proxy book entry created via `btcli proxy create --name`, all of them
    are filled in automatically.
    """
    app_ctx: AppContext = ctx_of(ctx)
    entry = cfg.get_proxy(name) if name else None
    if name and entry is None:
        app_ctx.output.error(
            f"proxy {name!r} not found in the proxy book",
            help="`btcli proxy book list` shows saved entries",
        )
        raise typer.Exit(1)

    def from_entry(key: str) -> Any:
        return entry.get(key) if entry else None

    spawner_value = spawner_ss58 or from_entry("spawner")
    if not spawner_value:
        app_ctx.output.error(
            "no spawner account given",
            help="pass `--spawner`, or use a proxy book entry that records one",
        )
        raise typer.Exit(2)
    spawner = app_ctx.resolve_address("coldkey_ss58", spawner_value)

    type_value = proxy_type.value if proxy_type else (from_entry("proxy_type") or "Staking")
    index_value = index if index is not None else int(from_entry("index") or 0)
    height_value = height if height is not None else from_entry("height")
    ext_value = ext_index if ext_index is not None else from_entry("ext_index")
    if height_value is None or ext_value is None:
        app_ctx.output.error(
            "the creation block height and extrinsic index are required",
            note="the chain identifies a pure proxy by the exact create_pure call that spawned it",
            help="create with `btcli proxy create --name NAME` to record them, or "
            "find the create_pure extrinsic in an explorer and pass "
            "`--height`/`--ext-index`",
        )
        raise typer.Exit(2)

    target = proxy_for or from_entry("address")
    if target:
        target = app_ctx.resolve_address("proxy_for", target)
    else:
        app_ctx.output.message(
            "[dim]note: kill_pure must be dispatched by the pure account itself; "
            "without `--proxy-for` the call is signed directly and will likely "
            "fail with NoPermission[/dim]"
        )
    result = app_ctx.submit(
        KillPureProxy(
            spawner_ss58=spawner,
            proxy_type=type_value,
            index=int(index_value),
            height=int(height_value),
            ext_index=int(ext_value),
        ),
        proxy_for=target,
    )
    if result is not None and result.success and entry is not None:
        cfg.remove_proxy(entry["name"])
        app_ctx.output.message(f"removed {entry['name']!r} from the proxy book")


@app.command("list")
@with_globals
def list_proxies(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Show an account's on-chain proxy delegations.

    Lists who may sign on the account's behalf (delegate, proxy type, delay)
    and the deposit reserved for them, straight from chain state. For a pure
    proxy, pass the pure account itself — its spawner shows up as a delegate.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    _register_book_names(app_ctx)
    data = app_ctx.run(lambda client: client.read("proxies", coldkey_ss58=owner))
    records = data["proxies"]
    out = app_ctx.output
    if out.json_mode:
        out.value({"account": owner, "proxies": records, "deposit": str(data["deposit"])})
        return
    owner_name = out.address_names.get(owner)
    title = f"on-chain proxies — {f'{owner_name} ({owner})' if owner_name else owner}"

    def label(ss58: str) -> str:
        proxy_name = out.address_names.get(ss58)
        return f"{proxy_name} ({ss58})" if proxy_name else ss58

    rows = [[label(r["delegate"]), r["proxy_type"], r["delay"]] for r in records]
    footer = f"[dim]deposit {data['deposit']}[/dim]" if records else None
    out.columns(title, ["delegate", "type", "delay"], rows, records, footer=footer)


@app.command("execute")
@with_tx_globals
def execute_announced(
    ctx: typer.Context,
    delegate_ss58: str = typer.Option(
        ..., address_cli_name("delegate_ss58"), help=ss58_param_help("delegate_ss58")
    ),
    real_ss58: str = typer.Option(
        ..., address_cli_name("real_ss58"), help=ss58_param_help("real_ss58")
    ),
    inner_op: str = typer.Option(..., "--inner-op", help="Intent op name for the inner call."),
    inner_args: str = typer.Option("{}", "--inner-args", help="JSON object of inner intent args."),
    force_proxy_type: Optional[ProxyTypeChoice] = typer.Option(None, "--force-proxy-type"),
):
    """Execute a previously announced proxy call."""
    app_ctx: AppContext = ctx_of(ctx)
    delegate = app_ctx.resolve_address("coldkey_ss58", delegate_ss58)
    real = app_ctx.resolve_address("coldkey_ss58", real_ss58)
    try:
        args = json.loads(inner_args)
    except json.JSONDecodeError as error:
        app_ctx.output.error(f"invalid --inner-args JSON: {error}")
        raise typer.Exit(1)
    app_ctx.submit(
        ExecuteProxyAnnounced(
            delegate_ss58=delegate,
            real_ss58=real,
            inner_op=inner_op,
            inner_args=args,
            force_proxy_type=force_proxy_type.value if force_proxy_type else None,
        )
    )


@book_app.command("add")
@with_globals
def book_add(
    ctx: typer.Context,
    name: str = typer.Option(..., "--name"),
    address: str = typer.Option(..., "--address", help="Pure or delegate proxy ss58."),
    spawner: str = typer.Option("", "--spawner"),
    proxy_type: ProxyTypeChoice = _proxy_type_option(),
    delay: int = typer.Option(0, "--delay"),
    index: int = typer.Option(0, "--index"),
    height: Optional[int] = typer.Option(
        None, "--height", help="Block number of the creating create_pure_proxy call."
    ),
    ext_index: Optional[int] = typer.Option(
        None, "--ext-index", help="Extrinsic index of the creating call within that block."
    ),
    note: str = typer.Option("", "--note"),
):
    """Add an entry to the local proxy address book."""
    app_ctx: AppContext = ctx_of(ctx)
    entry: dict[str, Any] = {
        "name": name,
        "address": address,
        "spawner": spawner,
        "proxy_type": proxy_type.value,
        "delay": delay,
        "index": index,
        "note": note,
    }
    if height is not None:
        entry["height"] = height
    if ext_index is not None:
        entry["ext_index"] = ext_index
    try:
        stored = cfg.add_proxy(entry)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail("added proxy", {"entry": stored, "path": str(cfg.proxies_path())})


async def _entry_status(client, entry: dict[str, Any]) -> str:
    """One proxy-book entry checked against live chain state."""
    address = entry.get("address")
    if not address:
        return "no address recorded"
    data = await client.read("proxies", coldkey_ss58=address)
    delegations = data["proxies"]
    if not delegations:
        return "stale: account has no on-chain delegations"
    spawner = entry.get("spawner")
    if not spawner:
        count = len(delegations)
        return f"ok: {count} on-chain delegation{'s' if count != 1 else ''}"
    wanted = entry.get("proxy_type") or "Staking"
    for delegation in delegations:
        if delegation["delegate"] == spawner and delegation["proxy_type"] == wanted:
            return "ok"
    if any(d["delegate"] == spawner for d in delegations):
        return "mismatch: spawner is a delegate but with a different proxy type"
    return "stale: spawner is not a delegate of this account"


@book_app.command("list")
@with_globals
def book_list(
    ctx: typer.Context,
    verify: bool = typer.Option(
        False,
        "--verify",
        help="Check each entry against on-chain proxy state (needs a connection).",
    ),
):
    """List entries in the local proxy address book."""
    app_ctx: AppContext = ctx_of(ctx)
    entries = cfg.load_proxies()
    if verify and entries:

        async def _verify(client):
            return [await _entry_status(client, entry) for entry in entries]

        statuses = app_ctx.run(_verify)
        entries = [{**entry, "status": status} for entry, status in zip(entries, statuses)]
    out = app_ctx.output
    if out.json_mode:
        out.value({"path": str(cfg.proxies_path()), "proxies": entries})
        return
    nodes = []
    for entry in entries:
        meta = [str(entry.get("proxy_type") or "Staking")]
        if entry.get("delay"):
            meta.append(f"delay {entry['delay']}")
        if entry.get("spawner"):
            spawner_name = out.address_names.get(entry["spawner"])
            meta.append(f"pure, spawned by {spawner_name or entry['spawner']}")
        leaves = [f"[dim]{entry.get('address') or '—'}[/dim]", f"[dim]{' · '.join(meta)}[/dim]"]
        if entry.get("note"):
            leaves.append(f"[dim italic]{entry['note']}[/dim italic]")
        status = entry.get("status")
        if status:
            style = "green" if status.startswith("ok") else "red"
            leaves.append(f"[{style}]{status}[/{style}]")
        nodes.append((str(entry.get("name", "")), leaves))
    out.tree(f"proxy book ({cfg.proxies_path()})", nodes, records=entries)


@book_app.command("remove")
@with_globals
def book_remove(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Proxy book entry name."),
):
    """Remove a proxy book entry."""
    app_ctx: AppContext = ctx_of(ctx)
    existed = cfg.remove_proxy(name)
    app_ctx.output.detail("removed proxy", {"name": name, "existed": existed})


@book_app.command("update")
@with_globals
def book_update(
    ctx: typer.Context,
    name: str = typer.Argument(...),
    address: Optional[str] = typer.Option(None, "--address"),
    note: Optional[str] = typer.Option(None, "--note"),
):
    """Update a proxy book entry."""
    app_ctx: AppContext = ctx_of(ctx)
    updates = {k: v for k, v in {"address": address, "note": note}.items() if v is not None}
    updated = cfg.update_proxy(name, updates)
    if updated is None:
        app_ctx.output.error(f"proxy {name!r} not found")
        raise typer.Exit(1)
    app_ctx.output.detail("updated proxy", updated)


@book_app.command("clear")
@with_globals
def book_clear(ctx: typer.Context):
    """Clear the local proxy address book."""
    app_ctx: AppContext = ctx_of(ctx)
    count = cfg.clear_proxies()
    app_ctx.output.detail("cleared proxies", {"removed": count, "path": str(cfg.proxies_path())})


app.add_typer(book_app, name="book")
