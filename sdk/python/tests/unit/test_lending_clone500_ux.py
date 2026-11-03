"""Clone-spec-500 UX: U4/N16 CLI path, N17 deferral decode, N18 dual deferrals, F12/F15."""

from __future__ import annotations

import os
import re

import pytest
from typer.testing import CliRunner

import bittensor.cli.context as cli_context
from bittensor import wallets
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.intents.lending import lending_outcome_data
from bittensor.reads import lending as lending_reads
from bittensor.reads.base import REGISTRY
from bittensor.result import BittensorError, ChainError, ErrorCode
from tests.harness.fake_substrate import FakeSubstrate

runner = CliRunner()
_WALLET_NAME = "clone500wallet"


@pytest.fixture(scope="module")
def wallet_dir(tmp_path_factory) -> str:
    path = str(tmp_path_factory.mktemp("wallets-clone500"))
    wallets.create(name=_WALLET_NAME, hotkey="default", path=path, use_password=False)
    return path


@pytest.fixture()
def fake(tmp_path, monkeypatch, wallet_dir) -> FakeSubstrate:
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

    substrate = FakeSubstrate()

    def make_client(network, **kwargs):
        return Client(network, substrate=substrate)

    monkeypatch.setattr(cli_context, "Client", make_client)
    return substrate


def test_open_amount_need_vs_available_via_cli_entrypoint(fake, monkeypatch, wallet_dir):
    """U4/N16: enrichment must survive AppContext.run (not a dead except)."""

    async def exceed(view, netuid, side, amount, coldkey_ss58, hotkey_ss58=None, **_kw):
        raise BittensorError(
            "insufficient collateral for borrow τ1.000000000: "
            f"needed more than unlocked alpha 0.100000000α_1 on {hotkey_ss58}"
        )

    monkeypatch.setattr(REGISTRY["lending_required_collateral"], "fetch", exceed)
    # No existing position.
    fake.seed_default("Lending", "Positions", None)

    hot = wallets.open_wallet(_WALLET_NAME, "default", wallet_dir).hotkey.ss58_address
    result = runner.invoke(
        app,
        [
            "--yes",
            "lending",
            "open",
            "--side",
            "long",
            "--netuid",
            "1",
            "--amount",
            "1",
            "--hotkey",
            hot,
        ],
    )
    assert result.exit_code != 0, result.output
    flat = " ".join(result.output.split())
    assert "insufficient collateral for borrow" in flat
    assert "needed more than unlocked alpha" in flat
    assert hot in result.output


def test_decode_deferral_reason_module_dump_and_compact():
    """N17: Module dumps and Pallet::{idx} expand to Pallet::ErrorName."""
    dump = "Module { index: 7, error: [98, 0, 0, 0] }"
    assert lending_reads._decode_deferral_reason(dump) == "SubtensorModule::InsufficientTaoBalance"
    assert (
        lending_reads._decode_deferral_reason(b"SubtensorModule::98")
        == "SubtensorModule::InsufficientTaoBalance"
    )
    assert lending_reads._decode_deferral_reason("SlippageExceeded") == "SlippageExceeded"


@pytest.mark.asyncio
async def test_annotate_deferral_keeps_both_kinds():
    """N18: interest + settlement pending → both visible."""

    class _View:
        async def query(self, item, params):
            name = str(item)
            if "LastDeferral" in name:
                return (1, 500, b"SubtensorModule::98")
            if "InterestRetry" in name:
                return ()
            if "ExpiryRetry" in name:
                return ()
            return None

    record = {
        "coldkey": "5Owner",
        "netuid": 1,
        "due": 400,
        "expiry_due": 450,
    }
    out = await lending_reads._annotate_deferral(_View(), record)
    kinds = {d["kind"] for d in out["deferrals"]}
    assert kinds == {"interest", "settlement"}
    assert out["deferred"]["kind"] in kinds
    assert "SubtensorModule::InsufficientTaoBalance" in out["deferrals"][1]["reason"]


def test_slippage_exceeded_help_is_not_generic_pool_line():
    """F12: SlippageExceeded must not use the INSUFFICIENT_LIQUIDITY pool line."""
    err = ChainError("forced trade missed limit", name="SlippageExceeded")
    assert err.code == ErrorCode.INSUFFICIENT_LIQUIDITY
    assert "pool cannot absorb" not in err.remediation
    assert "max-slippage" in err.remediation.lower() or "max_payment" in err.remediation


def _strip_help(text: str) -> str:
    """Remove ANSI / OSC-8 and normalize dashes so help asserts stay stable on CI."""
    text = re.sub(r"\x1b\]8;;.*?\x1b\\", "", text)
    text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", text)
    text = text.replace("\u2011", "-").replace("\u2010", "-").replace("\u00ad", "")
    return text


def test_lending_close_help_fits_80_columns(monkeypatch):
    """F15: option names / hotkey help must not ellipsize at COLUMNS=80."""
    monkeypatch.setenv("COLUMNS", "80")
    monkeypatch.setenv("NO_COLOR", "1")
    result = runner.invoke(
        app,
        ["lending", "close", "--help"],
        env={**os.environ, "COLUMNS": "80", "NO_COLOR": "1", "TERM": "dumb"},
    )
    assert result.exit_code == 0, result.output
    flat = _strip_help(result.output)
    assert "repay_from_wal…" not in flat
    assert "from-wallet,…" not in flat
    assert "from-wallet" in flat, flat
    # Long alias pair removed so the Options column fits.
    assert "--repay-from-wallet/--no-repay-from-wallet" not in flat


def test_lending_outcome_data_raise_uses_received():
    """N19: raise outcome labels sale TAO as received (not payment)."""
    events = [
        {
            "event": {
                "module_id": "Lending",
                "event_id": "Raised",
                "attributes": {
                    "netuid": 1,
                    "collateral_alpha": 10**9,
                    "principal_tao": 2 * 10**9,
                    "escrow_alpha": 10**9,
                    "sell_alpha": 5 * 10**8,
                    "free_tao_out": 4 * 10**8,
                },
            }
        }
    ]
    data = lending_outcome_data(events, op="raise_loan")
    assert "received" in data
    assert "payment" not in data
    assert data["received_rao"] == 4 * 10**8


def test_docs_ref_documented_on_help():
    """N12: BITTENSOR_DOCS_REF appears on root / lending help."""
    root = runner.invoke(app, ["--help"])
    assert "BITTENSOR_DOCS_REF" in root.output
    lending = runner.invoke(app, ["lending", "--help"])
    assert "BITTENSOR_DOCS_REF" in lending.output
