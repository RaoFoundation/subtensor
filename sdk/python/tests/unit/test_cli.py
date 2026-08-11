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

import bittensor.cli.commands.root as root_commands
import bittensor.cli.context as cli_context
from bittensor import RpcConnectionError, RpcPolicyError, __version__, config, wallets
from bittensor.balance import Balance
from bittensor.cli.call_names import resolve_builder_params
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.cli.root_helpers import RootPosition, position_columns, position_rows
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


def seed_root_validator_summary(fake: FakeSubstrate) -> None:
    fake.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_validator_basket_summary",
        {
            "hotkey": BOB,
            "nav_tao": 1_250_000_000,
            "spot_nav_tao": 1_500_000_000,
            "deposited_tao": 1_000_000_000,
            "redeemed_tao": 0,
            "weights": [(1, 65535)],
            "holdings": [
                {
                    "netuid": 1,
                    "alpha": 2_000_000_000,
                    "spot_tao": 1_500_000_000,
                    "realizable_tao": 1_250_000_000,
                }
            ],
        },
    )


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
    def test_rpc_policy_refusal_is_actionable(self, monkeypatch):
        class RefusedClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                raise RpcPolicyError(
                    "RPC endpoint rate limited this source (HTTP 429)",
                    retry_after="60",
                )

            async def __aexit__(self, *args):
                pass

        monkeypatch.setattr(cli_context, "Client", RefusedClient)
        result = invoke("query", "tx-rate-limit")
        assert result.exit_code == 1
        assert "rate limited this source (HTTP 429)" in result.output
        assert "wait 60 seconds, then retry" in result.output

    def test_rpc_policy_refusal_without_retry_after_suggests_reducing_rate(self, monkeypatch):
        class RefusedClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                raise RpcPolicyError("RPC endpoint rate limited this source (HTTP 429)")

            async def __aexit__(self, *args):
                pass

        monkeypatch.setattr(cli_context, "Client", RefusedClient)
        result = invoke("query", "tx-rate-limit")
        assert result.exit_code == 1
        assert "reduce the request or connection rate, then retry" in result.output

    def test_rpc_policy_http_date_retry_after_is_preserved(self, monkeypatch):
        class RefusedClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                raise RpcPolicyError(
                    "RPC endpoint rate limited this source (HTTP 429)",
                    retry_after="Wed, 21 Oct 2026 07:28:00 GMT",
                )

            async def __aexit__(self, *args):
                pass

        monkeypatch.setattr(cli_context, "Client", RefusedClient)
        result = invoke("query", "tx-rate-limit")
        assert result.exit_code == 1
        assert "retry after Wed, 21 Oct 2026 07:28:00 GMT" in result.output

    def test_public_rpc_connection_error_is_actionable(self, monkeypatch):
        class FailedClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                raise RpcConnectionError("all configured endpoints refused the connection")

            async def __aexit__(self, *args):
                pass

        monkeypatch.setattr(cli_context, "Client", FailedClient)
        result = invoke("query", "tx-rate-limit")
        assert result.exit_code == 1
        assert "could not reach finney: all configured endpoints refused" in result.output
        assert "check the endpoint and network connection, then retry" in result.output

    def test_bare_connection_timeout_is_not_blank(self, monkeypatch):
        class TimeoutClient:
            def __init__(self, *args, **kwargs):
                pass

            async def __aenter__(self):
                raise TimeoutError

            async def __aexit__(self, *args):
                pass

        monkeypatch.setattr(cli_context, "Client", TimeoutClient)
        result = invoke("query", "tx-rate-limit")
        assert result.exit_code == 1
        assert (
            "could not reach finney: the connection timed out without a response" in result.output
        )

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


class TestAddressResolution:
    @staticmethod
    def app_context(wallet_dir: str) -> cli_context.AppContext:
        return cli_context.AppContext(
            network="finney",
            wallet_name=_WALLET_NAME,
            hotkey_name="default",
            wallet_path=wallet_dir,
            assume_yes=True,
            dry_run=False,
            output=Output(json_mode=True),
        )

    def test_raw_call_uses_canonical_saved_multisig_resolution(self, fake, wallet_dir):
        config.add_multisig({"name": "treasury", "threshold": 1, "signatories": [BOB]})
        app_ctx = self.app_context(wallet_dir)
        expected = app_ctx.resolve_address_ref("new_coldkey", "treasury")

        params = resolve_builder_params(
            app_ctx,
            "SubtensorModule.schedule_swap_coldkey",
            {"new_coldkey": "treasury"},
        )

        assert params["new_coldkey"] == expected.address
        assert expected.source == "saved multisig 'treasury'"

    def test_raw_call_does_not_guess_arbitrary_string_lists_are_accounts(self, fake, wallet_dir):
        app_ctx = self.app_context(wallet_dir)
        params = {"remark": [BOB, "ordinary memo text"]}

        assert resolve_builder_params(app_ctx, "System.remark", params) == params


class TestRoot:
    @pytest.mark.parametrize("all_wallets", [False, True])
    def test_position_rows_match_columns(self, all_wallets):
        position = RootPosition(
            hotkey=BOB,
            staked=Balance.from_tao(1),
            accrued=Balance.from_tao("0.25"),
            wallet=_WALLET_NAME if all_wallets else None,
        )

        rows = position_rows([position], all_wallets)

        assert all(len(row) == len(position_columns(all_wallets)) for row in rows)

    def test_list_single_coldkey_renders_human_table(self, fake: FakeSubstrate, monkeypatch):
        async def root_positions(_client, _coldkey_ss58):
            return [
                RootPosition(
                    hotkey=BOB,
                    staked=Balance.from_tao(1),
                    accrued=Balance.from_tao("0.25"),
                )
            ]

        monkeypatch.setattr(root_commands, "fetch_root_positions", root_positions)

        result = invoke("root", "list", "--coldkey", BOB)

        assert result.exit_code == 0, result.exception
        assert "root positions of" in result.output
        assert "staked (τ)" in result.output
        assert "τ1.250000000" in result.output

    def test_list_all_wallets_renders_wallet_column(self, fake: FakeSubstrate, monkeypatch):
        async def all_root_positions(_client, _coldkeys):
            return [
                RootPosition(
                    hotkey=BOB,
                    staked=Balance.from_tao(1),
                    accrued=Balance.from_tao("0.25"),
                    wallet=_WALLET_NAME,
                    coldkey=BOB,
                )
            ]

        monkeypatch.setattr(root_commands, "list_coldkeys", lambda _path: [(_WALLET_NAME, BOB)])
        monkeypatch.setattr(root_commands, "fetch_all_root_positions", all_root_positions)

        result = invoke("root", "list", "--all")

        assert result.exit_code == 0, result.exception
        assert "wallet" in result.output
        assert _WALLET_NAME in result.output
        assert "τ1.250000000" in result.output

    def test_show_explicit_hotkey_renders_human_detail(self, fake: FakeSubstrate, monkeypatch):
        async def root_positions(_client, _coldkey_ss58):
            return []

        monkeypatch.setattr(root_commands, "fetch_root_positions", root_positions)
        seed_root_validator_summary(fake)

        result = invoke("root", "show", "--hotkey", BOB, "--coldkey", BOB)

        assert result.exit_code == 0, result.exception
        assert "weights of" in result.output
        assert "fund holdings of" in result.output
        assert "fund nav: τ1.250000000" in result.output

    def test_show_explicit_hotkey_json_emits_one_document(self, fake: FakeSubstrate, monkeypatch):
        async def root_positions(_client, _coldkey_ss58):
            return []

        monkeypatch.setattr(root_commands, "fetch_root_positions", root_positions)
        seed_root_validator_summary(fake)

        result = invoke("--json", "root", "show", "--hotkey", BOB, "--coldkey", BOB)

        assert result.exit_code == 0, result.exception
        payload = json.loads(result.output)
        assert payload["hotkey"] == BOB
        assert payload["nav_tao"] == "τ1.250000000"
        assert payload["weights"] == [{"netuid": 1, "weight": 65535, "share": 1.0}]


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


def test_local_hotkey_name_is_not_valid_ss58():
    """Local wallet names must be resolved before ss58 decode.

    Passing a name like ``hotkey1`` straight into the codec raises Substrate's
    opaque ``Length is bad`` — the failure mode ``btcli s register --hotkey
    hotkey1`` hit before CLI call sites resolved ``*_ss58`` options.
    """
    from bittensor._transport.codec import ss58_decode

    with pytest.raises(ValueError, match="Length is bad"):
        ss58_decode("hotkey1")


class TestResolveHotkeySs58:
    """Hand-written CLI commands must resolve ``--hotkey`` like generated ``tx``."""

    @pytest.fixture()
    def alt_hotkey(self, wallet_dir) -> str:
        wallets.new_hotkey(name=_WALLET_NAME, hotkey="hotkey1", path=wallet_dir, overwrite=True)
        return wallets.open_wallet(_WALLET_NAME, "hotkey1", wallet_dir).hotkey.ss58_address

    def test_subnets_register_resolves_local_hotkey_name(
        self, fake: FakeSubstrate, monkeypatch, alt_hotkey: str
    ):
        captured: list = []

        def capture(self, intent, **kwargs):
            captured.append(intent)
            return None

        monkeypatch.setattr(cli_context.AppContext, "submit", capture)
        result = invoke("subnets", "register", "--netuid", "18", "--hotkey", "hotkey1")
        assert result.exit_code == 0, result.output
        assert len(captured) == 1
        assert captured[0].op == "burned_register"
        assert captured[0].hotkey_ss58 == alt_hotkey

    def test_set_auto_stake_resolves_local_hotkey_name(
        self, fake: FakeSubstrate, monkeypatch, alt_hotkey: str
    ):
        captured: list = []

        def capture(self, intent, **kwargs):
            captured.append(intent)
            return None

        monkeypatch.setattr(cli_context.AppContext, "submit", capture)
        result = invoke("stake", "set-auto", "--netuid", "1", "--hotkey", "hotkey1")
        assert result.exit_code == 0, result.output
        assert captured[0].hotkey_ss58 == alt_hotkey

    def test_lock_add_resolves_local_hotkey_name(
        self, fake: FakeSubstrate, monkeypatch, alt_hotkey: str
    ):
        captured: list = []

        def capture(self, intent, **kwargs):
            captured.append(intent)
            return None

        monkeypatch.setattr(cli_context.AppContext, "submit", capture)
        result = invoke(
            "lock", "add", "--netuid", "1", "--amount-alpha", "1", "--hotkey", "hotkey1"
        )
        assert result.exit_code == 0, result.output
        assert captured[0].hotkey_ss58 == alt_hotkey

    def test_stake_burn_resolves_local_hotkey_name(
        self, fake: FakeSubstrate, monkeypatch, alt_hotkey: str
    ):
        captured: list = []

        def capture(self, intent, **kwargs):
            captured.append(intent)
            return None

        monkeypatch.setattr(cli_context.AppContext, "submit", capture)
        result = invoke(
            "sudo",
            "stake-burn",
            "--netuid",
            "1",
            "--amount-tao",
            "1",
            "--limit-price",
            "1000",
            "--hotkey",
            "hotkey1",
        )
        assert result.exit_code == 0, result.output
        assert captured[0].hotkey_ss58 == alt_hotkey

    def test_child_revoke_resolves_local_hotkey_name(
        self, fake: FakeSubstrate, monkeypatch, alt_hotkey: str
    ):
        captured: list = []

        def capture(self, intent, **kwargs):
            captured.append(intent)
            return None

        monkeypatch.setattr(cli_context.AppContext, "submit", capture)
        result = invoke("stake", "child", "revoke", "--netuid", "1", "--hotkey", "hotkey1")
        assert result.exit_code == 0, result.output
        assert captured[0].hotkey_ss58 == alt_hotkey
