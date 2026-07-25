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
from rich.text import Text
from typer.testing import CliRunner

import bittensor.cli.context as cli_context
from bittensor import __version__, wallets
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.intents import REGISTRY
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE_HOT, BOB, BOB_HOT

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
        for op in (
            "add-stake",
            "transfer",
            "set-weights",
            "create-crowdloan",
        ):
            assert op in result.output

    def test_stake_remove_help_keeps_one_command_for_single_and_multiple(self):
        result = invoke("stake", "remove", "--help")
        assert result.exit_code == 0
        help_text = Text.from_ansi(result.output).plain
        assert "--hotkey" in help_text
        assert "-in" in help_text
        assert "--all-hotkeys" in help_text
        assert "--exclude-hotkeys" in help_text
        assert "--positions" not in help_text

        group = invoke("stake", "--help")
        assert group.exit_code == 0
        assert "remove-many" not in group.output

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

    @pytest.mark.parametrize("amount_flag", ["--amount-alpha", "--amount", "-a"])
    def test_stake_remove_singular_keeps_the_existing_call_shape(
        self, fake: FakeSubstrate, amount_flag: str
    ):
        result = invoke(
            "--json",
            "--yes",
            "--no-mev-shield",
            "stake",
            "remove",
            "--hotkey",
            BOB,
            "--netuid",
            "1",
            amount_flag,
            "1.25",
            "--no-slippage-protection",
        )

        assert result.exit_code == 0, result.output
        call = fake.last_call
        assert (call.module, call.function) == ("SubtensorModule", "remove_stake")
        assert call.params == {
            "hotkey": BOB,
            "netuid": 1,
            "amount_unstaked": 1_250_000_000,
        }

    @pytest.mark.parametrize("selector_flag", ["--include-hotkeys", "-in"])
    def test_stake_remove_resolves_multiple_hotkeys_and_submits_atomic_batch(
        self, fake: FakeSubstrate, selector_flag: str
    ):
        saved = invoke("--json", "addresses", "add", "validator-two", BOB)
        assert saved.exit_code == 0, saved.output

        result = invoke(
            "--json",
            "--yes",
            "--no-mev-shield",
            "stake",
            "remove",
            selector_flag,
            f"validator-two,{BOB_HOT}",
            "--netuid",
            "1",
            "--amount-alpha",
            "1.25",
            "--no-slippage-protection",
        )

        assert result.exit_code == 0, result.output
        payload = json.loads(result.output)
        assert payload["success"] is True
        call = fake.last_call
        assert (call.module, call.function) == ("Utility", "batch_all")
        assert [child.params["hotkey"] for child in call.params["calls"]] == [BOB, BOB_HOT]
        assert [child.params["netuid"] for child in call.params["calls"]] == [1, 1]

    def test_stake_remove_all_hotkeys_uses_live_positions_and_exclusions(
        self, fake: FakeSubstrate, wallet_dir: str
    ):
        coldkey = wallets.open_wallet(_WALLET_NAME, "default", wallet_dir).coldkeypub.ss58_address
        fake.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_coldkey",
            [
                {
                    "hotkey": BOB,
                    "coldkey": coldkey,
                    "netuid": 1,
                    "stake": 1_000_000_000,
                    "is_registered": True,
                },
                {
                    "hotkey": BOB_HOT,
                    "coldkey": coldkey,
                    "netuid": 1,
                    "stake": 2_000_000_000,
                    "is_registered": True,
                },
                {
                    "hotkey": ALICE_HOT,
                    "coldkey": coldkey,
                    "netuid": 1,
                    "stake": 3_000_000_000,
                    "is_registered": True,
                },
                {
                    "hotkey": ALICE_HOT,
                    "coldkey": coldkey,
                    "netuid": 2,
                    "stake": 4_000_000_000,
                    "is_registered": True,
                },
            ],
        )

        result = invoke(
            "--json",
            "--yes",
            "--no-mev-shield",
            "stake",
            "remove",
            "--all-hotkeys",
            "-ex",
            BOB_HOT,
            "--netuid",
            "1",
            "--amount-alpha",
            "1",
            "--no-slippage-protection",
        )

        assert result.exit_code == 0, result.output
        call = fake.last_call
        assert (call.module, call.function) == ("Utility", "batch_all")
        assert [child.params["hotkey"] for child in call.params["calls"]] == [BOB, ALICE_HOT]

    def test_stake_remove_without_netuid_uses_every_matching_position(
        self, fake: FakeSubstrate, wallet_dir: str
    ):
        coldkey = wallets.open_wallet(_WALLET_NAME, "default", wallet_dir).coldkeypub.ss58_address
        fake.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_coldkey",
            [
                {
                    "hotkey": BOB,
                    "coldkey": coldkey,
                    "netuid": 1,
                    "stake": 1_000_000_000,
                    "is_registered": True,
                },
                {
                    "hotkey": BOB,
                    "coldkey": coldkey,
                    "netuid": 2,
                    "stake": 2_000_000_000,
                    "is_registered": True,
                },
                {
                    "hotkey": BOB_HOT,
                    "coldkey": coldkey,
                    "netuid": 2,
                    "stake": 3_000_000_000,
                    "is_registered": True,
                },
            ],
        )

        result = invoke(
            "--json",
            "--yes",
            "--no-mev-shield",
            "stake",
            "remove",
            "--include-hotkeys",
            f"{BOB},{BOB_HOT}",
            "--amount",
            "0.5",
            "--no-slippage-protection",
        )

        assert result.exit_code == 0, result.output
        call = fake.last_call
        assert (call.module, call.function) == ("Utility", "batch_all")
        assert [
            (child.params["hotkey"], child.params["netuid"]) for child in call.params["calls"]
        ] == [(BOB, 1), (BOB, 2), (BOB_HOT, 2)]

    def test_stake_remove_rejects_conflicting_selection_flags(self, fake: FakeSubstrate):
        result = invoke(
            "--json",
            "--yes",
            "stake",
            "remove",
            "--hotkey",
            BOB,
            "--include-hotkeys",
            BOB_HOT,
            "--netuid",
            "1",
            "--amount-alpha",
            "all",
        )

        assert result.exit_code == 2
        assert "choose only one" in result.output
        assert fake.submissions == []

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
