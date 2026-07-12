"""`btcli evm`: the Bittensor EVM, end to end, without leaving the terminal.

Subtensor runs a full EVM as an application layer. Its accounts (h160,
MetaMask-style) and native accounts (ss58) are disjoint signing domains on
the same chain; this group owns the seam: key storage, address math, money
movement in both directions, hotkey association, and precompile access.

Two execution paths, one discipline. `fund`, `claim-deposit`, and `associate`
are substrate extrinsics and ride the intent plan/confirm/execute flow;
`send`, `send-to-ss58`, and `call` on non-view functions are EVM-side
transactions signed with a stored EVM key and submitted over JSON-RPC — with
the same --dry-run / confirm / --json conventions.
"""

from __future__ import annotations

import typer

from ...tx import intent_command

# keys and stake register their commands on key_app/stake_app at import time.
from . import (  # noqa: F401
    address,
    association,
    contracts_cmd,
    keys,
    money,
    precompiles_cmd,
    setup,
    stake,
)
from ._shared import PANEL_CHAIN, PANEL_KEYS, PANEL_MONEY, key_app, stake_app

app = typer.Typer(
    no_args_is_help=True,
    help="Bittensor EVM: keys, funding, transfers, association, and precompiles.",
)

app.add_typer(key_app, name="key", rich_help_panel=PANEL_KEYS)
app.add_typer(stake_app, name="stake", rich_help_panel=PANEL_CHAIN)

address.register(app)
money.register(app)
association.register(app)
precompiles_cmd.register(app)
contracts_cmd.register(app)
setup.register(app)

app.command("claim-deposit", rich_help_panel=PANEL_MONEY)(intent_command("evm_withdraw"))

__all__ = ["app"]
