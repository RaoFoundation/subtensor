"""Metagraph currency labels must describe the queried block, not cached metadata."""

from __future__ import annotations

import json

import pytest
from typer.testing import CliRunner

import bittensor.cli.context as cli_context
from bittensor import config
from bittensor.cli.main import app
from bittensor.client import Client
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT


@pytest.fixture()
def chain(tmp_path, monkeypatch):
    for env, name in [
        ("BTCLI_CONFIG", "config.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_MULTISIG_CACHE", "multisig.json"),
        ("BTCLI_SUBNET_NAMES_CACHE", "names.json"),
        ("BTCLI_TOKEN_SYMBOLS_CACHE", "symbols.json"),
    ]:
        monkeypatch.setenv(env, str(tmp_path / name))
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path / "wallets"))
    fake = FakeSubstrate()
    monkeypatch.setattr(
        cli_context, "Client", lambda network, **kwargs: Client(network, substrate=fake)
    )
    return fake


def graph_at(symbol: str, netuid: int = 92, block: int = 100) -> dict:
    graph = dict.fromkeys(
        [
            "rho",
            "kappa",
            "min_allowed_weights",
            "max_weights_limit",
            "weights_version",
            "weights_rate_limit",
            "activity_cutoff",
            "max_validators",
            "commit_reveal_weights_enabled",
            "commit_reveal_period",
            "liquid_alpha_enabled",
            "alpha_high",
            "alpha_low",
            "bonds_moving_avg",
            "registration_allowed",
            "pow_registration_allowed",
            "burn",
            "min_burn",
            "max_burn",
            "difficulty",
            "min_difficulty",
            "max_difficulty",
            "immunity_period",
            "adjustment_interval",
            "adjustment_alpha",
            "target_regs_per_interval",
            "max_regs_per_block",
            "serving_rate_limit",
            "subnet_volume",
            "subnet_emission",
            "tao_in_emission",
            "alpha_in_emission",
            "alpha_out_emission",
            "pending_alpha_emission",
            "pending_root_emission",
        ],
        0,
    )
    graph.update(
        netuid=netuid,
        name=list(b"subnet"),
        symbol=list(symbol.encode("utf-8")),
        block=block,
        tempo=360,
        last_step=90,
        blocks_since_last_step=block - 90,
        owner_hotkey=ALICE_HOT,
        owner_coldkey=ALICE,
        network_registered_at=1,
        hotkeys=[ALICE_HOT],
        coldkeys=[ALICE],
        num_uids=1,
        max_uids=256,
        tao_in=7_000_000_000,
        alpha_in=2_000_000_000,
        alpha_out=3_000_000_000,
        emission=[4_000_000_000],
        total_stake=[5_000_000_000],
        alpha_stake=[5_000_000_000],
        tao_stake=[6_000_000_000],
        collateral_locked=[1_000_000_000],
        collateral_min=[1_000_000_000],
        collateral_earned=[1_000_000_000],
    )
    return graph


@pytest.mark.parametrize(
    ("netuid", "symbol", "cached", "unit"),
    [
        (92, "र", "ᚂ", "र"),
        (92, "ᚂ", "र", "ᚂ"),
        (92, "ש", "ᚂ", "ש"),
        (92, "", "ᚂ", "α₉₂"),
        (0, "τ", "ᚂ", "τ"),
    ],
)
def test_cli_uses_metagraph_symbol_for_pool_stake_and_emission(chain, netuid, symbol, cached, unit):
    config.save_token_symbols("finney", {netuid: cached})
    graph = graph_at(symbol, netuid)
    chain.seed_runtime("SubnetInfoRuntimeApi", "get_metagraph", graph)

    result = CliRunner().invoke(app, ["subnets", "metagraph", str(netuid)])

    assert result.exit_code == 0, result.output
    for amount in (2, 3, 4, 5):
        expected = f"τ{amount}.000000000" if netuid == 0 else f"{amount}.000000000{unit}"
        assert expected in result.output
    assert "τ7.000000000" in result.output
    assert cached not in result.output
    # Historical reads must not overwrite another block's shared display metadata.
    assert config.load_token_symbols("finney") == {netuid: cached}


def test_cli_json_keeps_chain_symbol_and_raw_amounts(chain):
    config.save_token_symbols("finney", {92: "ᚂ"})
    chain.seed_runtime("SubnetInfoRuntimeApi", "get_metagraph", graph_at("र"))

    result = CliRunner().invoke(app, ["--json", "subnets", "metagraph", "92"])

    assert result.exit_code == 0, result.output
    graph = json.loads(result.output)
    assert graph["symbol"] == "र"
    assert graph["total_stake"] == [5_000_000_000]
    assert graph["emission"] == [4_000_000_000]


@pytest.mark.asyncio
async def test_sdk_balances_keep_each_snapshots_symbol(chain):
    client = Client("finney", substrate=chain)
    chain.token_symbols = {92: "stale"}
    graphs = []
    for block, symbol in [(99, "ᚂ"), (100, "र")]:
        chain.seed_runtime("SubnetInfoRuntimeApi", "get_metagraph", graph_at(symbol, block=block))
        snapshot = await client.at(block)
        graphs.append(await snapshot.subnets.metagraph(92, commitments=False))

    for graph, symbol in zip(graphs, ["ᚂ", "र"]):
        assert graph.symbol == symbol
        neuron = graph.neuron(0)
        for field in (
            "emission",
            "alpha_stake",
            "total_stake",
            "collateral_locked",
            "collateral_min",
            "collateral_earned",
        ):
            balance = getattr(neuron, field)
            assert balance.netuid == 92
            assert balance.unit == symbol
        assert neuron.tao_stake.unit == "τ"
    assert client.token_symbols == {92: "stale"}
