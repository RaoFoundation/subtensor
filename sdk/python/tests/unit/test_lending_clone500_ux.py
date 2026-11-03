"""Clone-spec-500 UX: U4/N16 CLI path, N17 deferral decode, N18 dual deferrals, F12/F15."""

from __future__ import annotations

import os
import re

import pytest
from typer.testing import CliRunner

import bittensor.cli.context as cli_context
from bittensor import wallets
from bittensor.balance import Balance
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
    env = {**os.environ, "COLUMNS": "80", "NO_COLOR": "1", "TERM": "dumb"}
    for args in (
        ["lending", "close", "--help"],
        ["lending", "repay", "--help"],
        ["query", "lending-close-quote", "--help"],
    ):
        result = runner.invoke(app, args, env=env)
        assert result.exit_code == 0, result.output
        flat = _strip_help(result.output)
        assert "repay_from_wal…" not in flat
        assert "from-wallet,…" not in flat
        assert "--principal,--a…" not in flat
        assert "force-proxy-ty…" not in flat
        assert "from-wallet" in flat, flat
        # Visible Options column uses short names; long form stays hidden (N21).
        assert "--repay-from-wallet/--no-repay-from-wallet" not in flat


def test_repay_from_wallet_hidden_alias_still_parses(fake, monkeypatch, wallet_dir):
    """N21: --repay-from-wallet remains accepted as a hidden alias."""
    fake.seed_default("Lending", "Positions", None)
    result = runner.invoke(
        app,
        [
            "--yes",
            "lending",
            "close",
            "--netuid",
            "1",
            "--repay-from-wallet",
            "--hotkey",
            wallets.open_wallet(_WALLET_NAME, "default", wallet_dir).hotkey.ss58_address,
        ],
        env={**os.environ, "NO_COLOR": "1"},
    )
    # No position → exit before submit; must not be "no such option".
    assert "no such option" not in result.output.lower()
    assert result.exit_code != 0


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


@pytest.mark.parametrize(
    "side,refund,top_up",
    [
        ({"Short": None}, 7 * 10**8, 10**8),
        ({"Long": None}, 3 * 10**8, 0),
    ],
)
def test_lending_outcome_data_full_repay_uses_closed_event(side, refund, top_up):
    """Full-principal repay emits Closed, not Repaid (Auditor fid:fe25eda6)."""
    events = [
        {
            "event": {
                "module_id": "Lending",
                "event_id": "Closed",
                "attributes": {
                    "netuid": 1,
                    "side": side,
                    "refund": refund,
                    "escrow_returned": 0,
                    "wallet_top_up": top_up,
                },
            }
        }
    ]
    data = lending_outcome_data(events, op="repay_loan")
    assert data["refund_rao"] == refund
    assert data["wallet_top_up_rao"] == top_up
    assert "principal_repaid" not in data


def test_docs_ref_documented_on_help():
    """N12: BITTENSOR_DOCS_REF appears on root / lending help."""
    root = runner.invoke(app, ["--help"])
    assert "BITTENSOR_DOCS_REF" in root.output
    lending = runner.invoke(app, ["lending", "--help"])
    assert "BITTENSOR_DOCS_REF" in lending.output


def test_open_amount_preserves_principal_exceeds_capacity(fake, monkeypatch, wallet_dir):
    """N20: PrincipalExceeds* must not become 'insufficient collateral'."""

    async def exceeds(view, netuid, side, amount, coldkey_ss58, hotkey_ss58=None, **_kw):
        err = ChainError(
            "exceeds pool capacity for borrow τ100000.000000000: "
            "max ~τ50000.000000000 (PrincipalExceedsTaoIn)",
            "PrincipalExceedsTaoIn",
        )
        raise err

    monkeypatch.setattr(REGISTRY["lending_required_collateral"], "fetch", exceeds)
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
            "100000",
            "--hotkey",
            hot,
        ],
    )
    assert result.exit_code != 0, result.output
    flat = " ".join(result.output.split())
    assert "insufficient collateral" not in flat.lower()
    assert "pool capacity" in flat.lower() or "PrincipalExceedsTaoIn" in flat


def test_open_amount_preserves_non_collateral_error(fake, monkeypatch, wallet_dir):
    """N20: SideCapacity (etc.) keeps its own cause, not collateral wording."""

    async def capped(view, netuid, side, amount, coldkey_ss58, hotkey_ss58=None, **_kw):
        raise ChainError("side cap has no headroom", "SideCapacity")

    monkeypatch.setattr(REGISTRY["lending_required_collateral"], "fetch", capped)
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
            "125",
            "--hotkey",
            hot,
        ],
    )
    assert result.exit_code != 0, result.output
    flat = " ".join(result.output.split())
    assert "insufficient collateral" not in flat.lower()
    assert "SideCapacity" in flat or "headroom" in flat.lower()


def test_repay_summary_full_vs_partial():
    """Full-principal repay must not say 'partially repay'."""
    from bittensor.intents.lending import RepayLoan

    partial = RepayLoan(netuid=1, principal_part="1", max_payment="2")
    assert "partially" in partial.summary()
    full = RepayLoan(netuid=1, principal_part="1", max_payment="2")
    full._closes_position = True
    assert "fully repay" in full.summary()
    assert "partially" not in full.summary()


def test_position_status_label_deferred():
    """Show/list status is deferred when a deferral is pending."""
    from bittensor.cli.commands.lending import _position_status_label

    assert (
        _position_status_label({"deferrals": [{"kind": "interest", "reason": "x"}]})
        == "deferred:interest"
    )
    assert _position_status_label({"deferred": {"kind": "settlement"}}) == "deferred:settlement"
    assert _position_status_label({"underwater": True}) == "underwater"
    assert _position_status_label({}) == "ok"


def test_lending_amount_uses_underscore_alpha():
    """Alpha display is α_N (not subscript α₁) in lending summaries/results."""
    from bittensor.intents.lending import OpenLoan, _fmt_lending_amount

    labeled = _fmt_lending_amount(Balance.from_rao(10**9, 1))
    assert labeled.endswith("α_1")
    assert "α₁" not in labeled
    intent = OpenLoan(
        netuid=1,
        side="Long",
        collateral="1",
        hotkey_ss58="5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        min_borrow="0",
    )
    assert "α_1" in intent.summary()
    assert "α₁" not in intent.summary()


@pytest.mark.asyncio
async def test_stake_unavailable_uses_availability_locked_not_stake_minus_available():
    """Auditor fid:6caee553 — conviction Lock is entry['locked'], not stake−available."""

    cold = "5Owner"
    hot = "5HotA"
    # Hotkey has 100α; 40α miner collateral; no conviction lock.
    # Coldkey available is 60α (100 − 0 lock − 40 collateral).
    # Bug would report conviction-locked = 100−60 = 40 (same as miner).

    class _View:
        async def runtime(self, method, params):
            name = str(method)
            if "get_stake_info_for_hotkey" in name:
                return {"stake": 100 * 10**9}
            if "get_stake_availability" in name:
                return {
                    cold: {
                        1: {
                            "total": 100 * 10**9,
                            "locked": 0,  # no conviction
                            "available": 60 * 10**9,
                        }
                    }
                }
            return None

        async def query(self, item, params):
            return {"locked": 40 * 10**9}

    err = await lending_reads.stake_unavailable_lending_detail(_View(), cold, hot, 1)
    text = str(err)
    assert "subnet conviction Lock 0.000000000α_1" in text
    assert "miner-collateral on hotkey 40.000000000α_1" in text
    assert "free 60.000000000α_1" in text
    # Must not double-count miner collateral as conviction.
    assert "subnet conviction Lock 40.000000000α_1" not in text


@pytest.mark.asyncio
async def test_stake_unavailable_reports_subnet_lock_with_sibling_hotkey():
    """Sibling hotkey free stake must not hide a real subnet conviction Lock."""

    cold = "5Owner"
    hot = "5HotA"

    class _View:
        async def runtime(self, method, params):
            name = str(method)
            if "get_stake_info_for_hotkey" in name:
                # This hotkey alone: 30α; sibling holds the rest.
                return {"stake": 30 * 10**9}
            if "get_stake_availability" in name:
                return {
                    cold: {
                        1: {
                            "total": 100 * 10**9,
                            "locked": 50 * 10**9,  # real conviction
                            "available": 50 * 10**9,
                        }
                    }
                }
            return None

        async def query(self, item, params):
            return {"locked": 0}

    err = await lending_reads.stake_unavailable_lending_detail(_View(), cold, hot, 1)
    text = str(err)
    assert "subnet conviction Lock 50.000000000α_1" in text
    assert "hotkey stake 30.000000000α_1" in text
    assert "free 30.000000000α_1" in text
