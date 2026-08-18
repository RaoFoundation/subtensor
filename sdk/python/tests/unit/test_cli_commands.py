"""CLI command table: every btcli function is wired and can run.

The public complaint was that a pile of commands "do not work." The SDK already
proves every intent plans and every read dispatches (``test_intents_table``,
``test_reads_table``). This module pins the CLI surface so a command that
exists in ``--help`` but crashes, or a registry entry with no command, fails CI.

Layers:
- every visible command/group ``--help`` exits 0 (catches broken signatures);
- every intent is a ``tx`` command and every read is a ``query`` command;
- every ``tx`` dry-runs from its sample args against FakeSubstrate;
- every ``query`` runs from its sample params against FakeSubstrate.
"""

from __future__ import annotations

import json

import pytest
from typer._click.core import Context
from typer.main import get_command
from typer.testing import CliRunner

import bittensor.cli.context as cli_context
from bittensor import wallets
from bittensor.cli.context import address_cli_name
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.intents import REGISTRY as INTENT_REGISTRY
from bittensor.reads import REGISTRY as READ_REGISTRY
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import INTENT_SAMPLES, READ_SAMPLES
from tests.unit.test_reads_table import seeded_substrate

runner = CliRunner()

_WALLET_NAME = "testwallet"


def _iter_command_paths(click_cmd, path: tuple[str, ...] = ()) -> list[tuple[str, ...]]:
    """Visible Click nodes: groups and leaves, skipping hidden aliases."""
    paths = [path]
    if not hasattr(click_cmd, "list_commands"):
        return paths
    ctx = Context(click_cmd)
    for name in click_cmd.list_commands(ctx):
        sub = click_cmd.get_command(ctx, name)
        if sub is None or getattr(sub, "hidden", False):
            continue
        paths.extend(_iter_command_paths(sub, (*path, name)))
    return paths


COMMAND_PATHS = _iter_command_paths(get_command(app))
COMMAND_IDS = [" ".join(path) or "root" for path in COMMAND_PATHS]


def _flag(field: str) -> str:
    if field.endswith("_ss58") or "_ss58" in field:
        return address_cli_name(field)
    return "--" + field.replace("_", "-")


def argv_from_sample(args: dict, *, lists: str) -> list[str]:
    """Turn a registry sample dict into CLI flag tokens.

    ``lists="json"`` is the generated ``tx`` shape (``_coerce``). ``lists="csv"``
    is the generated ``query`` shape (comma-separated arrays).
    """
    argv: list[str] = []
    for key, value in args.items():
        if value is None:
            continue
        flag = _flag(key)
        if isinstance(value, bool):
            argv.append(flag if value else f"--no-{flag.lstrip('-')}")
            continue
        if isinstance(value, (list, dict)):
            text = (
                json.dumps(value)
                if lists == "json" or isinstance(value, dict)
                else ",".join(str(part) for part in value)
            )
            argv.extend([flag, text])
            continue
        argv.extend([flag, str(value)])
    return argv


def invoke(*args: str):
    return runner.invoke(app, list(args))


@pytest.fixture(scope="session")
def wallet_dir(tmp_path_factory) -> str:
    path = str(tmp_path_factory.mktemp("wallets"))
    wallets.create(name=_WALLET_NAME, hotkey="default", path=path, use_password=False)
    return path


@pytest.fixture()
def isolated_cli(tmp_path, monkeypatch, wallet_dir) -> None:
    for var, filename in [
        ("BTCLI_CONFIG", "btcli.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_MULTISIG_CACHE", "multisig_cache.json"),
        ("BTCLI_SUBNET_NAMES_CACHE", "subnet_names.json"),
        ("BTCLI_TOKEN_SYMBOLS_CACHE", "token_symbols.json"),
    ]:
        monkeypatch.setenv(var, str(tmp_path / filename))
    monkeypatch.setenv("BT_WALLET_PATH", wallet_dir)
    monkeypatch.setenv("BT_WALLET", _WALLET_NAME)


def _seed_tx_preview(substrate: FakeSubstrate) -> None:
    """Enough chain state for CLI dry-run of every intent.

    Required-shield ops preflight free TAO for the outer carrier fee even on
    ``--dry-run``. ``register_subnet`` also quotes the live registration cost
    before it plans.
    """
    substrate.seed_default(
        "System", "Account", {"data": {"free": 10**18, "reserved": 0, "frozen": 0}}
    )
    substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_network_registration_cost", 10**9)
    substrate.seed("SubtensorModule", "SubnetLimit", [], 16)
    substrate.seed("SubtensorModule", "DissolveCleanupQueue", [], [])
    substrate.seed("SubtensorModule", "NetworkRegistrationQueue", [], [])
    substrate.seed_map("SubtensorModule", "NetworksAdded", [(0, True)])


@pytest.fixture()
def fake(isolated_cli, monkeypatch) -> FakeSubstrate:
    substrate = FakeSubstrate()
    _seed_tx_preview(substrate)

    def make_client(network, **kwargs):
        return Client(network, substrate=substrate)

    monkeypatch.setattr(cli_context, "Client", make_client)
    return substrate


@pytest.fixture()
def fake_reads(isolated_cli, monkeypatch) -> FakeSubstrate:
    substrate = seeded_substrate()

    def make_client(network, **kwargs):
        return Client(network, substrate=substrate)

    monkeypatch.setattr(cli_context, "Client", make_client)
    return substrate


def test_every_intent_has_a_tx_command():
    leaves = {path[1] for path in COMMAND_PATHS if path[:1] == ("tx",) and len(path) == 2}
    expected = {op.replace("_", "-") for op in INTENT_REGISTRY}
    assert leaves == expected, (
        f"tx commands missing {sorted(expected - leaves)}; extra {sorted(leaves - expected)}"
    )


def test_every_read_has_a_query_command():
    leaves = {path[1] for path in COMMAND_PATHS if path[:1] == ("query",) and len(path) == 2}
    expected = {name.replace("_", "-") for name in READ_REGISTRY}
    assert leaves == expected, (
        f"query commands missing {sorted(expected - leaves)}; extra {sorted(leaves - expected)}"
    )


@pytest.mark.parametrize("path", COMMAND_PATHS, ids=COMMAND_IDS)
def test_help_exits_zero(path: tuple[str, ...]):
    result = invoke(*path, "--help")
    assert result.exception is None, result.exception
    assert result.exit_code == 0, result.output
    assert "Traceback" not in result.output


@pytest.mark.parametrize("op", sorted(INTENT_REGISTRY))
def test_tx_dry_run_from_sample(op: str, fake: FakeSubstrate):
    result = invoke(
        "--json",
        "--dry-run",
        "tx",
        op.replace("_", "-"),
        *argv_from_sample(INTENT_SAMPLES[op], lists="json"),
    )
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert plan["op"] == op
    assert plan["ok"] is True, plan
    assert fake.submissions == []


@pytest.mark.parametrize("name", sorted(READ_REGISTRY))
def test_query_from_sample(name: str, fake_reads: FakeSubstrate):
    result = invoke(
        "--json",
        "query",
        name.replace("_", "-"),
        *argv_from_sample(READ_SAMPLES[name], lists="csv"),
    )
    assert result.exit_code == 0, result.output
    assert "Traceback" not in result.output
    json.loads(result.output)
