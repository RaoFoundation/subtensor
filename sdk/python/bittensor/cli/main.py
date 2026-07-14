"""btcli: the command line for the Bittensor chain.

Global options (network, wallet identity, output mode) are declared on the root
callback (with env defaults) and *also* on subcommands as overrides, so they
show up in each command's --help and can be passed either before or after the
subcommand. Each command only carries the options it can act on — mutations get
the signing/confirmation flags, reads don't (see cli/globals.py). Writes confirm
before submitting unless ``--yes`` is given.
"""

from __future__ import annotations

import copy
import importlib.metadata
import json
import sys
from typing import Optional

import typer
from typer.core import TyperGroup

from .. import __version__, wallets
from .._generated.errors import ERRORS
from ..config import get as config_default
from ..error_descriptions import DESCRIPTIONS
from ..intents import list_tools
from ..result import EXPLANATIONS, REMEDIATION, ChainError, ErrorCode, classify_error
from ..settings import DEFAULT_NETWORK, chain_error_docs_url, error_docs_url
from . import globals as g
from . import help_theme  # noqa: F401  (restyles typer's --help at import)
from .call import call as call_command
from .commands import (
    addresses,
    axon,
    config,
    crowd,
    evm,
    extension,
    lock,
    multisig,
    proxy,
    stake,
    subnets,
    sudo,
    timelock,
    upgrade,
    utils,
    wallet,
    weights,
)
from .context import AppContext
from .logs import setup_logging
from .output import Output
from .prompt import run_app
from .query import build_query_app
from .tx import build_tx_app


class _GroupsFirstTyperGroup(TyperGroup):
    """List subcommand groups before plain commands (keeping registration
    order otherwise). Typer prints help panels in first-occurrence order of
    the command list, and plain commands always list before groups — without
    this, `call`/`tools`/`explain` would drag the escape-hatch panel to the
    top instead of last."""

    def list_commands(self, ctx) -> list[str]:
        names = super().list_commands(ctx)
        return sorted(names, key=lambda name: not isinstance(self.commands[name], TyperGroup))


app = typer.Typer(
    cls=_GroupsFirstTyperGroup,
    no_args_is_help=True,
    add_completion=True,
    help="btcli - a lean command line for the Bittensor chain.",
    context_settings={"help_option_names": ["-h", "--help"]},
)

# Root --help is grouped into panels so it reads as a map, not a linear dump.
# The unnamed (default) panel renders first and mirrors btcli's familiar top
# level; everything else is themed. Registration order fixes the row order.
PANEL_STAKING = "Staking & markets"
PANEL_OPERATORS = "Validating & mining"
PANEL_ACCOUNTS = "Accounts & signing"
PANEL_PROXY_MULTISIG = "Proxy & multisig"
PANEL_CHAIN = "Raw chain access & agents"

# Core commands (the btcli-familiar set), most-used first: everyday wallet and
# staking operations, then subnet inspection, then owner/admin and housekeeping.
app.add_typer(wallet.app, name="wallet")
app.add_typer(stake.app, name="stake")
app.add_typer(subnets.app, name="subnets")
app.add_typer(sudo.app, name="sudo")
app.add_typer(config.app, name="config")
app.add_typer(utils.app, name="utils")

app.add_typer(lock.app, name="lock", rich_help_panel=PANEL_STAKING)
app.add_typer(crowd.app, name="crowd", rich_help_panel=PANEL_STAKING)

app.add_typer(weights.app, name="weights", rich_help_panel=PANEL_OPERATORS)
app.add_typer(axon.app, name="axon", rich_help_panel=PANEL_OPERATORS)
app.add_typer(timelock.app, name="timelock", rich_help_panel=PANEL_OPERATORS)

app.add_typer(addresses.app, name="addresses", rich_help_panel=PANEL_ACCOUNTS)
app.add_typer(evm.app, name="evm", rich_help_panel=PANEL_ACCOUNTS)
app.add_typer(extension.app, name="extension", rich_help_panel=PANEL_ACCOUNTS)

app.add_typer(proxy.app, name="proxy", rich_help_panel=PANEL_PROXY_MULTISIG)
app.add_typer(multisig.app, name="multisig", rich_help_panel=PANEL_PROXY_MULTISIG)
app.add_typer(upgrade.app, name="upgrade", rich_help_panel=PANEL_PROXY_MULTISIG)

# Hidden group aliases carried over from the v9 btcli (`btcli w list`, etc.).
for _sub_app, _aliases in (
    (wallet.app, ("w", "wallets")),
    (stake.app, ("st",)),
    (subnets.app, ("s", "subnet")),
    (sudo.app, ("su",)),
    (config.app, ("c", "conf")),
    (weights.app, ("wt", "weight")),
    (crowd.app, ("cr", "crowdloan")),
):
    for _alias in _aliases:
        app.add_typer(_sub_app, name=_alias, hidden=True)

# Generated from registries, plus escape hatches / agent tooling
app.add_typer(build_query_app(), name="query", rich_help_panel=PANEL_CHAIN)
app.add_typer(build_tx_app(), name="tx", rich_help_panel=PANEL_CHAIN)
app.command("call", rich_help_panel=PANEL_CHAIN)(call_command)


def _version(show: bool) -> None:
    if show:
        typer.echo(__version__)
        raise typer.Exit()


@app.callback()
def main_callback(
    ctx: typer.Context,
    network: str = typer.Option(
        config_default("network", DEFAULT_NETWORK),
        "--network",
        "-n",
        envvar="BT_NETWORK",
        help="Network name (finney/test/local) or a ws:// endpoint.",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    fallback_endpoints: Optional[str] = typer.Option(
        config_default("fallback_endpoints"),
        "--fallback-endpoints",
        envvar="BT_FALLBACK_ENDPOINTS",
        help="Comma-separated ws:// endpoints tried when the primary is unreachable "
        "(default: the network's public pool; 'none' to disable).",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    archive_endpoints: Optional[str] = typer.Option(
        config_default("archive_endpoints"),
        "--archive-endpoints",
        envvar="BT_ARCHIVE_ENDPOINTS",
        help="Comma-separated archive ws:// endpoints used for reads older than the "
        "primary retains (default: the network's public pool; 'none' to disable).",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    retry_forever: bool = typer.Option(
        config_default("retry_forever", False),
        "--retry-forever",
        envvar="BT_RETRY_FOREVER",
        help="Never give up on connection failures; keep cycling the endpoint pool.",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    wallet_name: str = typer.Option(
        config_default("wallet", "default"),
        "--wallet",
        "-w",
        envvar="BT_WALLET",
        help="Coldkey wallet name.",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    hotkey_name: str = typer.Option(
        config_default("wallet_hotkey", "default"),
        "--wallet-hotkey",
        "-H",
        envvar="BT_WALLET_HOTKEY",
        help="Hotkey name within the wallet.",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    wallet_path: str = typer.Option(
        config_default("wallet_path", wallets.DEFAULT_WALLET_PATH),
        "--wallet-path",
        envvar="BT_WALLET_PATH",
        help="Wallet directory.",
        rich_help_panel=g.PANEL_NETWORK_WALLET,
    ),
    json_output: bool = typer.Option(
        config_default("json", False),
        "--json",
        help="Emit machine-readable JSON.",
        rich_help_panel=g.PANEL_OUTPUT,
    ),
    assume_yes: bool = typer.Option(
        False,
        "--yes",
        "-y",
        help="Skip confirmation prompts.",
        rich_help_panel=g.PANEL_EXECUTION,
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Preview mutations (fee, effects) without submitting.",
        rich_help_panel=g.PANEL_EXECUTION,
    ),
    mev_shield: Optional[bool] = typer.Option(
        None,
        "--mev-shield/--no-mev-shield",
        help="Encrypt extrinsics via the MevShield pallet so the mempool can't "
        "front-run them. Stake-trading commands shield by default; pass "
        "--no-mev-shield to submit in the clear.",
        rich_help_panel=g.PANEL_EXECUTION,
    ),
    quiet: bool = typer.Option(
        config_default("quiet", False),
        "--quiet",
        "-q",
        help="Suppress informational output.",
        rich_help_panel=g.PANEL_OUTPUT,
    ),
    verbosity: int = typer.Option(
        0,
        "--verbose",
        "-v",
        count=True,
        help="Diagnostic logging on stderr: -v connection lifecycle, "
        "-vv full debug, -vvv raw websocket frames.",
        rich_help_panel=g.PANEL_OUTPUT,
    ),
    signer_backend: Optional[str] = typer.Option(
        None,
        "--signer",
        help="Signing backend: wallet (default) or extension.",
        rich_help_panel=g.PANEL_EXTENSION,
    ),
    signer_address: Optional[str] = typer.Option(
        None,
        "--signer-address",
        envvar="BT_SIGNER_ADDRESS",
        help="Extension account ss58 address (optional; last-used account is the default).",
        rich_help_panel=g.PANEL_EXTENSION,
    ),
    extension_source: Optional[str] = typer.Option(
        None,
        "--extension-source",
        help="Filter extension accounts by source (e.g. talisman, polkadot-js).",
        rich_help_panel=g.PANEL_EXTENSION,
    ),
    extension_browser: Optional[str] = typer.Option(
        config_default("extension_browser"),
        "--extension-browser",
        envvar="BT_EXTENSION_BROWSER",
        help="Browser for the bridge page: firefox, chrome, or an app name.",
        rich_help_panel=g.PANEL_EXTENSION,
    ),
    extension_bridge_url: Optional[str] = typer.Option(
        None,
        "--extension-bridge",
        envvar="BT_EXTENSION_BRIDGE",
        help="Extension bridge WebSocket URL.",
        rich_help_panel=g.PANEL_EXTENSION,
    ),
    _v: Optional[bool] = typer.Option(
        None, "--version", callback=_version, is_eager=True, help="Show version and exit."
    ),
):
    setup_logging(verbosity=verbosity, quiet=quiet)
    wallet_source = ctx.get_parameter_source("wallet_name")
    hotkey_source = ctx.get_parameter_source("hotkey_name")
    ctx.obj = AppContext(
        network=network,
        fallback_endpoints=fallback_endpoints,
        archive_endpoints=archive_endpoints,
        retry_forever=retry_forever,
        wallet_name=wallet_name,
        hotkey_name=hotkey_name,
        wallet_path=wallet_path,
        wallet_given=wallet_source is not None and wallet_source.name != "DEFAULT",
        hotkey_given=hotkey_source is not None and hotkey_source.name != "DEFAULT",
        assume_yes=assume_yes,
        dry_run=dry_run,
        mev_shield=mev_shield,
        verbosity=verbosity,
        output=Output(json_mode=json_output, quiet=quiet, network=network),
        signer_backend=signer_backend,
        signer_address=signer_address,
        extension_source=extension_source,
        extension_browser=extension_browser,
        extension_bridge_url=extension_bridge_url,
    )


@app.command("tools", rich_help_panel=PANEL_CHAIN)
def tools():
    """Print the machine-readable operation catalog (JSON) for agents."""
    typer.echo(json.dumps(list_tools(), indent=2))


def _chain_error_matches(query: str) -> list[dict[str, str]]:
    """Catalog entries matching a chain error name, case-insensitively.

    Accepts bare names (``SettingWeightsTooFast``) and pallet-qualified forms
    (``SubtensorModule.SettingWeightsTooFast``). Names that exist in several
    pallets yield one entry per pallet.
    """
    pallet_filter, _, name = query.rpartition(".")
    matches = []
    for info in ERRORS.values():
        if info.name.lower() != name.lower():
            continue
        if pallet_filter and info.pallet.lower() != pallet_filter.lower():
            continue
        code = classify_error(info.docs, info.name)
        matches.append(
            {
                "pallet": info.pallet,
                "name": info.name,
                "code": code.value,
                "docs": info.docs,
                "description": DESCRIPTIONS.get(info.name, ""),
                # Same remediation a failure would print (per-name overrides
                # like SlippageTooHigh's tolerance flags included).
                "help": ChainError("", name=info.name).remediation,
                # The exact name's page when it is classified (and thus has
                # one), the semantic code's page otherwise.
                "docs_url": (
                    chain_error_docs_url(info.name)
                    if info.name in DESCRIPTIONS
                    else error_docs_url(code.value)
                ),
            }
        )
    return matches


def _chain_error_catalog(out: Output, pallet: Optional[str]) -> None:
    """Print the full chain error catalog, optionally filtered by pallet."""
    pallets = sorted({info.pallet for info in ERRORS.values()})
    if pallet is not None and pallet.lower() not in {p.lower() for p in pallets}:
        out.error(
            f"unknown pallet {pallet!r}",
            help="valid pallets: " + ", ".join(pallets),
        )
        raise typer.Exit(1)
    records = [
        {
            "pallet": info.pallet,
            "name": info.name,
            "code": classify_error(info.docs, info.name).value,
            "description": info.docs,
            "docs_url": (chain_error_docs_url(info.name) if info.name in DESCRIPTIONS else None),
        }
        for info in ERRORS.values()
        if pallet is None or info.pallet.lower() == pallet.lower()
    ]
    out.error_catalog("chain errors", records)


@app.command("explain", rich_help_panel=PANEL_CHAIN)
def explain(
    ctx: typer.Context,
    code: Optional[str] = typer.Argument(
        None,
        help="Error code from a diagnostic (e.g. insufficient_balance) or an exact "
        "chain error name (e.g. SettingWeightsTooFast). With --chain: a pallet filter.",
    ),
    chain: bool = typer.Option(
        False,
        "--chain",
        help="List every chain error with its code and description.",
    ),
):
    """Long-form explanation of an error code (like `rustc --explain`)."""
    out: Output = ctx.obj.output
    if chain:
        _chain_error_catalog(out, code)
        return
    if code is None:
        _chain_error_catalog(out, None)
        out.message(
            "explain one entry with `btcli explain <Name>` or "
            "`btcli explain <code>`; filter with "
            "`btcli explain --chain <pallet>`"
        )
        return
    query = code.strip()
    try:
        error_code = ErrorCode(query.lower())
    except ValueError:
        matches = _chain_error_matches(query)
        if not matches:
            out.error(
                f"unknown error code {query!r}",
                help="run `btcli explain` for the semantic codes, or "
                "`btcli explain --chain` for the chain error catalog",
            )
            raise typer.Exit(1)
        out.explain_chain(matches)
        return
    out.explain(error_code.value, EXPLANATIONS[error_code], REMEDIATION[error_code])


def _register_snake_case_aliases(root: typer.Typer) -> None:
    """Register hidden snake_case aliases for every hyphenated command.

    The v9 btcli accepted both spellings (`wallet new-hotkey` and
    `wallet new_hotkey`); keep the underscore forms working — including for
    the generated `query`/`tx` commands, whose registry names are snake_case
    — without cluttering --help.
    """
    seen: set[int] = set()

    def walk(t: typer.Typer) -> None:
        if id(t) in seen:
            return
        seen.add(id(t))
        for info in list(t.registered_commands):
            name = info.name or info.callback.__name__.replace("_", "-")
            if "-" not in name:
                continue
            alias = copy.copy(info)
            alias.name = name.replace("-", "_")
            alias.hidden = True
            t.registered_commands.append(alias)
        for group in t.registered_groups:
            walk(group.typer_instance)

    walk(root)


_register_snake_case_aliases(app)


def _warn_if_legacy_cli_installed() -> None:
    """One-line stderr warning when the v9 ``bittensor-cli`` package coexists.

    Both packages install a ``btcli`` console script. This (v11) one is
    currently winning, but pip still lists the script in bittensor-cli's
    RECORD, so ``pip uninstall bittensor-cli`` would delete it — and
    reinstalling bittensor-cli would clobber it with the v9 CLI. Warn until
    the stale package is gone.
    """
    try:
        importlib.metadata.distribution("bittensor-cli")
    except importlib.metadata.PackageNotFoundError:
        return
    print(
        "warning: the legacy bittensor-cli package (v9 btcli) is still installed "
        "and also owns the `btcli` command; uninstalling or reinstalling it will "
        "break this one. Fix:\n"
        "    pip uninstall bittensor-cli && "
        "pip install --force-reinstall --no-deps bittensor",
        file=sys.stderr,
    )


def main() -> None:
    from bittensor.wallets import is_bittensor_address

    _warn_if_legacy_cli_installed()
    argv = sys.argv[1:]
    if (
        len(argv) >= 3
        and argv[0] == "addresses"
        and argv[1] not in ("add", "save", "list", "show", "remove")
        and not argv[1].startswith("-")
        and not argv[2].startswith("-")
        and is_bittensor_address(argv[2])
    ):
        sys.argv.insert(2, "add")
    # run_app drives the app so missing required params prompt interactively
    # (see cli/prompt.py) instead of dying with click's "Missing option".
    run_app(app)


if __name__ == "__main__":
    main()
