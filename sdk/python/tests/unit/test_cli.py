"""CLI tests: the ``btcli`` Typer app driven in-process via CliRunner.

Chain access is swapped for the in-memory ``FakeSubstrate`` by patching the
``Client`` factory that ``AppContext.run`` uses, and all local state
(config, address book, caches, wallets) is isolated to a temp directory via
the ``BTCLI_*`` env vars — a test can never touch the real ``~/.bittensor``.
"""

from __future__ import annotations

import contextlib
import json

import pytest
from typer.testing import CliRunner

import bittensor.cli.context as cli_context
from bittensor import __version__, wallets
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.intents import REGISTRY
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import BOB

runner = CliRunner()

# One wallet on disk for the whole session; creating sr25519 keys is slow
# enough to matter per-test. State-isolation env vars are function-scoped.
_WALLET_NAME = "testwallet"


@pytest.fixture(scope="session")
def wallet_dir(tmp_path_factory) -> str:
    path = str(tmp_path_factory.mktemp("wallets"))
    wallets.create(name=_WALLET_NAME, hotkey="default", path=path, use_password=False)
    return path


@pytest.fixture()
def fake(tmp_path, monkeypatch, wallet_dir) -> FakeSubstrate:
    """Isolate all local state and route Client construction to a FakeSubstrate."""
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


def invoke(*args: str):
    return runner.invoke(app, list(args))


class TestOffline:
    """Commands that never open a connection."""

    def test_help(self):
        result = invoke("--help")
        assert result.exit_code == 0
        for group in ("wallet", "stake", "subnets", "query", "tx"):
            assert group in result.output

    def test_version(self):
        result = invoke("--version")
        assert result.exit_code == 0
        assert result.output.strip() == __version__

    def test_tools_catalog(self):
        result = invoke("tools")
        assert result.exit_code == 0
        tools = json.loads(result.output)
        assert {t["name"] for t in tools} == set(REGISTRY)

    def test_explain_semantic_code(self):
        result = invoke("explain", "insufficient_balance")
        assert result.exit_code == 0
        assert "insufficient_balance" in result.output

    def test_explain_unknown_code_fails(self):
        result = invoke("explain", "not_a_real_code")
        assert result.exit_code == 1

    def test_tx_group_lists_every_intent(self):
        result = invoke("tx", "--help")
        assert result.exit_code == 0
        for op in ("add-stake", "transfer", "set-weights", "create-crowdloan"):
            assert op in result.output

    def test_query_group_help(self):
        result = invoke("query", "--help")
        assert result.exit_code == 0
        for name in ("metagraph", "balance", "tx-rate-limit"):
            assert name in result.output


class TestQueries:
    def test_scalar_query_json(self, fake: FakeSubstrate):
        fake.seed("SubtensorModule", "TxRateLimit", None, 1234)
        result = invoke("--json", "query", "tx-rate-limit")
        assert result.exit_code == 0, result.output
        assert json.loads(result.output) == 1234

    def test_query_with_param(self, fake: FakeSubstrate):
        fake.seed("SubtensorModule", "ImmunityPeriod", [3], 7200)
        result = invoke("--json", "query", "immunity-period", "--netuid", "3")
        assert result.exit_code == 0, result.output
        assert json.loads(result.output) == 7200

    def test_wallet_balance_by_address(self, fake: FakeSubstrate):
        fake.seed("System", "Account", [BOB], {"data": {"free": 2_500_000_000}})
        result = invoke("--json", "wallet", "balance", BOB)
        assert result.exit_code == 0, result.output
        payload = json.loads(result.output)
        assert payload["coldkey"] == BOB
        assert payload["free_tao"] == pytest.approx(2.5)


class TestTransactions:
    def test_dry_run_renders_plan_without_submitting(self, fake: FakeSubstrate):
        result = invoke(
            "--json",
            "--dry-run",
            "tx",
            "transfer",
            "--dest",
            BOB,
            "--amount-tao",
            "1.5",
        )
        assert result.exit_code == 0, result.output
        plan = json.loads(result.output)
        assert plan["op"] == "transfer"
        assert plan["ok"] is True
        assert fake.submissions == []

    def test_noninteractive_without_yes_is_refused(self, fake: FakeSubstrate):
        result = invoke("--json", "tx", "transfer", "--dest", BOB, "--amount-tao", "1.5")
        assert result.exit_code == 1
        assert "refusing to submit" in result.output
        assert fake.submissions == []

    def test_yes_submits_and_reports_success(self, fake: FakeSubstrate):
        result = invoke(
            "--json",
            "--yes",
            "tx",
            "transfer",
            "--dest",
            BOB,
            "--amount-tao",
            "1.5",
        )
        assert result.exit_code == 0, result.output
        payload = json.loads(result.output)
        assert payload["success"] is True
        call, _signer, _ = fake.submissions[-1]
        assert (call.module, call.function) == ("Balances", "transfer_keep_alive")
        assert call.params["value"] == 1_500_000_000

    def test_failed_extrinsic_exits_nonzero(self, fake: FakeSubstrate):
        from bittensor.result import ChainError, ExtrinsicResult

        fake.queue_result(
            ExtrinsicResult(
                success=False,
                message="Insufficient balance",
                error=ChainError("Insufficient balance", "InsufficientBalance"),
            )
        )
        result = invoke(
            "--json",
            "--yes",
            "tx",
            "transfer",
            "--dest",
            BOB,
            "--amount-tao",
            "1.5",
        )
        assert result.exit_code == 1
        payload = json.loads(result.output)
        assert payload["success"] is False

    def test_subnet_create_json_reports_immediate_registration(self, fake: FakeSubstrate):
        from dataclasses import replace

        from tests.harness.fake_substrate import success_result

        event = {
            "event": {
                "module_id": "SubtensorModule",
                "event_id": "NetworkAdded",
                "attributes": [8, 1],
            }
        }
        fake.queue_result(replace(success_result(), events=[event]))

        result = invoke("--json", "--yes", "subnets", "create")

        assert result.exit_code == 0, result.output
        payload = json.loads(result.output)
        assert payload["data"]["netuid"] == 8
        assert payload["data"]["registration_mode"] == "immediate"

    def test_subnet_create_human_output_explains_immediate_flow(self, fake: FakeSubstrate):
        from dataclasses import replace

        from tests.harness.fake_substrate import success_result

        event = {
            "event": {
                "module_id": "SubtensorModule",
                "event_id": "NetworkAdded",
                "attributes": [8, 1],
            }
        }
        fake.queue_result(replace(success_result(), events=[event]))
        fake.seed("SubtensorModule", "SubnetLocked", [8], 2_500_000_000)

        result = invoke("--yes", "subnets", "create")

        assert result.exit_code == 0, result.output
        assert "subnet 8 registered" in result.output
        assert "immediate · no deregistration needed" in result.output
        assert "price  τ2.500000000" in result.output

    @pytest.mark.parametrize(
        ("subnet_limit", "prune_netuid", "expected_flow"),
        [
            (2, None, "immediate · no deregistration needed"),
            (1, 1, "queued · deregisters subnet 1 before registration"),
        ],
    )
    def test_subnet_create_confirmation_includes_live_price_and_flow(
        self,
        fake: FakeSubstrate,
        monkeypatch,
        subnet_limit,
        prune_netuid,
        expected_flow,
    ):
        from dataclasses import replace

        from tests.harness.fake_substrate import success_result

        fake.seed_runtime(
            "SubnetRegistrationRuntimeApi", "get_network_registration_cost", 7_998_452_462_874
        )
        fake.seed_runtime("SubnetInfoRuntimeApi", "get_subnet_to_prune", prune_netuid)
        fake.seed("SubtensorModule", "SubnetLocked", [8], 7_998_452_462_874)
        fake.seed_map("SubtensorModule", "NetworksAdded", [(0, True), (1, True)])
        fake.seed("SubtensorModule", "SubnetLimit", [], subnet_limit)
        fake.seed("SubtensorModule", "DissolveCleanupQueue", [], [])
        fake.seed("SubtensorModule", "NetworkRegistrationQueue", [], [])
        fake.queue_result(
            replace(
                success_result(),
                events=[
                    {
                        "event": {
                            "module_id": "SubtensorModule",
                            "event_id": "NetworkAdded",
                            "attributes": [8, 1],
                        }
                    }
                ],
            )
        )

        prompts = []

        def tracked_confirm(self, prompt):
            prompts.append(prompt)

        monkeypatch.setattr(cli_context.AppContext, "confirm", tracked_confirm)
        result = invoke("subnets", "create")

        assert result.exit_code == 0, result.output
        assert prompts == ["register a new subnet for 7,998.452462874 TAO?"]
        assert expected_flow in result.output
        assert "price  τ7,998.452462874" in result.output

    def test_subnet_create_unlocks_before_starting_activity(self, fake: FakeSubstrate, monkeypatch):
        from dataclasses import replace

        from bittensor.cli.output import Output
        from tests.harness.fake_substrate import success_result

        fake.queue_result(
            replace(
                success_result(),
                events=[
                    {
                        "event": {
                            "module_id": "SubtensorModule",
                            "event_id": "NetworkAdded",
                            "attributes": [8, 1],
                        }
                    }
                ],
            )
        )
        order = []
        real_signing_keypair = wallets.signing_keypair
        real_activity = Output.activity

        def tracked_signing_keypair(*args, **kwargs):
            order.append("unlock")
            return real_signing_keypair(*args, **kwargs)

        @contextlib.contextmanager
        def tracked_activity(self, initial):
            order.append("activity")
            with real_activity(self, initial) as update:
                yield update

        monkeypatch.setattr(wallets, "signing_keypair", tracked_signing_keypair)
        monkeypatch.setattr(Output, "activity", tracked_activity)

        result = invoke("--yes", "subnets", "create")

        assert result.exit_code == 0, result.output
        assert order == ["unlock", "activity"]

    def test_subnet_create_waits_and_explains_deregistration(
        self, fake: FakeSubstrate, wallet_dir: str, monkeypatch
    ):
        from dataclasses import replace

        from bittensor.cli.output import Output
        from tests.harness.fake_substrate import success_result

        wallet = wallets.open_wallet(_WALLET_NAME, "default", wallet_dir)
        coldkey = wallet.coldkeypub.ss58_address
        hotkey = wallet.hotkey.ss58_address
        fake.queue_result(
            replace(
                success_result(),
                events=[
                    {
                        "extrinsic_idx": 1,
                        "event": {
                            "module_id": "SubtensorModule",
                            "event_id": "NetworkRemoved",
                            "attributes": 6,
                        },
                    },
                    {
                        "extrinsic_idx": 1,
                        "event": {
                            "module_id": "SubtensorModule",
                            "event_id": "NetworkRegistrationQueued",
                            "attributes": {
                                "coldkey": coldkey,
                                "hotkey": hotkey,
                                "registration_block": 100,
                            },
                        },
                    },
                ],
            )
        )
        fake.seed("SubtensorModule", "SubnetOwner", [6], coldkey)
        fake.seed("SubtensorModule", "SubnetOwnerHotkey", [6], hotkey)
        fake.seed_events(
            102,
            [
                {
                    "phase": "Finalization",
                    "extrinsic_idx": None,
                    "event": {
                        "module_id": "SubtensorModule",
                        "event_id": "NetworkAdded",
                        "attributes": [6, 1],
                    },
                }
            ],
        )

        updates = []

        @contextlib.contextmanager
        def tracked_activity(_self, _initial):
            def update(text, announce=False):
                updates.append((text, announce))

            yield update

        monkeypatch.setattr(Output, "activity", tracked_activity)

        result = invoke("--yes", "subnets", "create")

        assert result.exit_code == 0, result.output
        assert (
            "capacity is full · deregistering subnet 6 before registration",
            True,
        ) in updates
        assert (
            "subnet 6 cleanup · 1 block since call · waiting for NetworkAdded",
            False,
        ) in updates
        assert "subnet 6 registered" in result.output
        assert "queued · registered after deregistration of subnet 6" in result.output
