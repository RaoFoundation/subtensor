"""Registration routing follows the actual call and respects explicit overrides."""

import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from typer.testing import CliRunner

from bittensor import Balance, Client, calls, config
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.executor import Executor
from bittensor.hashed import descriptor_storage, descriptor_value, registration_descriptor_value
from bittensor.intents import Transfer
from bittensor.receiving import receiving_address
from bittensor.shielding import contains_account_registration
from bittensor.sp_core import Keypair
from tests.conftest import golden_codec
from tests.harness.fake_substrate import FakeSubstrate, success_result
from tests.harness.samples import ALICE, BOB, dev_wallet
from tests.unit import test_cli

fake = test_cli.fake
wallet_dir = test_cli.wallet_dir
runner = CliRunner()


def registration():
    key = Keypair.create_from_seed(bytes([91]) * 32, 7)
    return calls.HashedAccounts.register(
        descriptor=registration_descriptor_value(bytes(key.hashed_descriptor))
    )


@pytest.mark.parametrize(
    "wrap",
    [
        lambda c: c,
        lambda c: calls.Utility.batch_all(calls=[c]),
        lambda c: calls.Proxy.proxy(real=ALICE, force_proxy_type=None, call=c),
        lambda c: calls.Sudo.sudo(call=c),
        lambda c: calls.Multisig.as_multi_threshold_1(other_signatories=[BOB], call=c),
        lambda c: calls.Utility.if_else(main=calls.System.remark(remark=b""), fallback=c),
        lambda c: calls.Utility.dispatch_as_fallible(as_origin={"system": "Root"}, call=c),
    ],
)
async def test_real_metadata_and_builder_registration_detection(wrap, monkeypatch):
    chain = FakeSubstrate()
    call = wrap(registration())
    assert await contains_account_registration(chain, call)
    codec = golden_codec()
    # This historical metadata predates HashedAccounts. Use a known leaf to
    # exercise real SCALE wrapper traversal; the exact registration targets
    # above and below are checked independently against current builders.
    monkeypatch.setattr(
        "bittensor.shielding.ACCOUNT_REGISTRATION_CALLS", frozenset({("System", "remark")})
    )
    call = wrap(calls.System.remark(remark=b"registration sentinel"))

    # Real composed calls contain only bytes, with decoded nested RuntimeCalls.
    # Encode builders as enums to exercise the transport representation too.
    def variant(value):
        if isinstance(value, calls.Call):
            return {value.module: {value.function: variant(value.params)}}
        if isinstance(value, dict):
            return {key: variant(item) for key, item in value.items()}
        if isinstance(value, list):
            return [variant(item) for item in value]
        return value

    composed = codec.compose_call(call.module, call.function, variant(call.params))
    chain.decode_scale = AsyncMock(side_effect=lambda _typ, data: codec.decode_call(data))
    assert await contains_account_registration(chain, composed)
    chain.decode_scale.assert_awaited_once()


@pytest.mark.parametrize(
    "call,expected",
    [
        (calls.HashedAccounts.register_derived(descriptor={}), True),
        (calls.HashedAccounts.register_derived_sponsored(descriptor={}, deposit_payer=ALICE), True),
        (calls.Proxy.create_protected_pure(proxy_type="Any", delay=0, index=0), True),
        (
            calls.Utility.as_protected_derivative(index=0, call=calls.System.remark(remark=b"")),
            True,
        ),
        (calls.HashedAccounts.check_registered(descriptor={}), False),
        (calls.Balances.transfer_keep_alive(dest=BOB, value=1), False),
        (calls.System.remark(remark={"HashedAccounts": {"register": {}}}), False),
    ],
)
async def test_only_account_setup_changes_the_default(call, expected):
    assert await contains_account_registration(FakeSubstrate(), call) is expected


@pytest.mark.parametrize("shielded,expected", [(None, True), (False, False), (True, True)])
async def test_raw_sdk_registration_default_and_overrides(shielded, expected):
    chain = FakeSubstrate()
    chain.mev_key = bytes(1184)
    executor = Executor(chain)
    executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    await executor.submit_call(registration(), dev_wallet(), shielded=shielded)
    assert bool(executor._submit_encrypted_call.await_count) is expected
    assert bool(chain.submissions) is not expected


async def test_sdk_default_unavailable_warns_but_explicit_shield_does_not_fallback():
    chain = FakeSubstrate()
    executor = Executor(chain)
    with pytest.warns(UserWarning, match="submitting account registration unshielded"):
        await executor.submit_call(registration(), dev_wallet())
    assert len(chain.submissions) == 1
    executor._submit_encrypted_call = AsyncMock(side_effect=ValueError("Shield unavailable"))
    with pytest.raises(ValueError, match="Shield unavailable"):
        await executor.submit_call(registration(), dev_wallet(), shielded=True)
    assert len(chain.submissions) == 1


@pytest.mark.parametrize("registered", [False, True])
async def test_sponsored_registration_routes_once_without_rebuilding_or_extra_deposit(registered):
    chain = FakeSubstrate()
    chain.mev_key = bytes(1184)
    chain.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    chain.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    chain.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 200_000_000)
    chain.seed("System", "Account", [ALICE], {"data": {"free": 10**12, "frozen": 0}})
    key = Keypair.create_from_seed(bytes([92]) * 32, 7)
    descriptor = bytes(key.hashed_descriptor)
    if registered:
        chain.seed(
            "HashedAccounts",
            descriptor_storage(descriptor),
            [key.ss58_address],
            {"descriptor": descriptor_value(descriptor)},
        )
    executor = Executor(chain)
    executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    executor.plan = AsyncMock(wraps=executor.plan)
    approve = AsyncMock(return_value=True)
    preview = await executor.preflight(Transfer(receiving_address(key), 1), dev_wallet())
    assert preview.account_registration is not registered
    await executor.execute(
        Transfer(receiving_address(key), 1),
        dev_wallet(),
        on_registration_sponsorship=approve,
        period=16,
    )
    executor.plan.assert_awaited_once()
    assert executor._submit_encrypted_call.await_count == (0 if registered else 1)
    assert approve.await_count == (0 if registered else 1)
    if not registered:
        assert approve.await_args.args[0].deposit == Balance.from_tao("0.2")
        assert executor._submit_encrypted_call.await_args.kwargs["period"] == 16


async def test_execute_explicit_shield_preserves_custom_period():
    executor = Executor(FakeSubstrate())
    executor.submit_shielded = AsyncMock(return_value=success_result())
    await executor.execute(Transfer(BOB, 1), dev_wallet(), shielded=True, period=16)
    assert executor.submit_shielded.await_args.kwargs["period"] == 16


@pytest.mark.parametrize("override,expected", [(None, True), (False, False), (True, True)])
async def test_multisig_sdk_registration_default(override, expected):
    chain = FakeSubstrate()
    chain.seed("System", "Account", [ALICE], {"data": {"free": 10**9}})
    chain.mev_key = bytes(1184)
    client = Client(substrate=chain)
    group = await client.multisig([ALICE, BOB], 2)
    client._executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    await group.approve(registration(), dev_wallet(), shielded=override)
    assert bool(client._executor._submit_encrypted_call.await_count) is expected


@pytest.mark.parametrize("override,expected", [(None, True), (False, False), (True, True)])
@pytest.mark.parametrize("nested", [False, True])
def test_raw_cli_preview_and_submission_agree(fake, monkeypatch, override, expected, nested):
    fake.mev_key = bytes(1184)
    fake.seed("MevShield", "NextKey", [], fake.mev_key)
    encrypt = AsyncMock(return_value=success_result())
    monkeypatch.setattr(Executor, "_submit_encrypted_call", encrypt)
    call = registration()
    target = "HashedAccounts.register"
    params = call.params
    if nested:
        target = "Utility.batch_all"
        params = {"calls": [{"HashedAccounts": {"register": params}}]}
    args = ["--yes", "--json", "--network", "local", "call", target, "--args", json.dumps(params)]
    if override is not None:
        args += ["--mev-shield" if override else "--no-mev-shield"]
    preview = runner.invoke(app, [*args, "--dry-run"])
    assert preview.exit_code == 0, preview.output
    assert json.loads(preview.output).get("mev_shield", False) is expected
    result = runner.invoke(app, args)
    assert result.exit_code == 0, result.output
    assert bool(encrypt.await_count) is expected


@pytest.mark.parametrize("override", ["default", "flag", "config"])
@pytest.mark.parametrize("external", [False, True])
def test_cli_automatic_sponsorship_uses_registration_default(fake, monkeypatch, override, external):
    fake.mev_key = bytes(1184)
    fake.seed("MevShield", "NextKey", [], fake.mev_key)
    fake.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    fake.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    fake.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    fake.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 200_000_000)
    fake.seed_default("System", "Account", {"data": {"free": 10**13, "frozen": 0}})
    key = Keypair.create_from_seed(bytes([93]) * 32, 7)
    encrypt = AsyncMock(return_value=success_result())
    monkeypatch.setattr(Executor, "_submit_encrypted_call", encrypt)
    prepare_signer = AsyncMock()
    monkeypatch.setattr(AppContext, "_prepare_two_stage_signer", prepare_signer)
    if external:
        monkeypatch.setattr(
            AppContext, "resolve_signing_wallet", AsyncMock(return_value=dev_wallet())
        )
    cards = []
    monkeypatch.setattr(AppContext, "_transaction_card", lambda *a, **kw: cards.append(kw))
    if override == "config":
        config.set_value("mev_shield", "false")
    args = [
        "--yes",
        "--json",
        "--network",
        "local",
        "wallet",
        "transfer",
        "--dest",
        receiving_address(key),
        "--amount",
        "1",
        "--max-registration-deposit",
        "0.2",
    ]
    if override == "flag":
        args += ["--no-mev-shield"]
    if external:
        args += ["--signer", "ledger"]
    result = runner.invoke(app, args)
    assert result.exit_code == 0, result.output
    assert cards[0]["shield"] is (override == "default")
    assert bool(encrypt.await_count) is (override == "default")
    assert bool(prepare_signer.await_count) is (override == "default")


def test_registration_added_after_preview_still_uses_shield(fake, monkeypatch):
    fake.mev_key = bytes(1184)
    fake.seed("MevShield", "NextKey", [], fake.mev_key)
    fake.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    fake.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    fake.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    fake.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 200_000_000)
    fake.seed_default("System", "Account", {"data": {"free": 10**13, "frozen": 0}})
    key = Keypair.create_from_seed(bytes([94]) * 32, 7)
    descriptor = bytes(key.hashed_descriptor)
    fake.seed(
        "HashedAccounts",
        descriptor_storage(descriptor),
        [key.ss58_address],
        {"descriptor": descriptor_value(descriptor)},
    )
    encrypt = AsyncMock(return_value=success_result())
    monkeypatch.setattr(Executor, "_submit_encrypted_call", encrypt)

    def reorg_after_preview(*args, **kwargs):
        assert not kwargs["shield"]  # The preview only contains check_registered.
        fake.seed("HashedAccounts", descriptor_storage(descriptor), [key.ss58_address], None)

    monkeypatch.setattr(AppContext, "_transaction_card", reorg_after_preview)
    result = runner.invoke(
        app,
        [
            "--yes",
            "--json",
            "--network",
            "local",
            "wallet",
            "transfer",
            "--dest",
            receiving_address(key),
            "--amount",
            "1",
            "--max-registration-deposit",
            "0.2",
        ],
    )
    assert result.exit_code == 0, result.output
    encrypt.assert_awaited_once()


@pytest.mark.parametrize("shielded", [True, False])
def test_extension_registration_keeps_one_selected_account_through_execution(
    fake, monkeypatch, shielded
):
    from bittensor.cli import context

    fake.mev_key = bytes(1184)
    fake.seed("MevShield", "NextKey", [], fake.mev_key)
    fake.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    fake.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    fake.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    fake.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 200_000_000)
    fake.seed_default("System", "Account", {"data": {"free": 10**13, "frozen": 0}})
    recipient = Keypair.create_from_seed(bytes([95]) * 32, 7)
    selected = []
    opened = []

    async def select(_url, **options):
        # A second picker could select a different payer after sponsorship review.
        address = ALICE if not selected else BOB
        account = SimpleNamespace(address=address, name="Account", source="test")
        selected.append(account)
        options["on_picked"](account)
        return SimpleNamespace(account=account)

    async def open_signer(_url, selection):
        key = Keypair.create_from_uri("//Alice" if selection.account.address == ALICE else "//Bob")
        signer = SimpleNamespace(
            ss58_address=key.ss58_address,
            public_key=key.public_key,
            crypto_type=key.crypto_type,
            sign=key.sign,
            close=AsyncMock(),
        )
        opened.append(signer)
        return signer

    ensure = AsyncMock(return_value="ws://test.invalid")
    monkeypatch.setattr("bittensor.extension.ensure_bridge", ensure)
    monkeypatch.setattr("bittensor.extension.select_extension_account", select)
    monkeypatch.setattr("bittensor.extension.open_extension_signer", open_signer)
    encrypt = AsyncMock(return_value=success_result())
    monkeypatch.setattr(Executor, "_submit_encrypted_call", encrypt)
    monkeypatch.setattr(AppContext, "_prepare_two_stage_signer", AsyncMock())
    cards = []
    quotes = []
    confirm = context.confirm_registration_sponsorship

    def capture_quote(app_ctx, quote):
        quotes.append(quote)
        return confirm(app_ctx, quote)

    def capture_card(app_ctx, intent, wallet, *args, **kwargs):
        cards.append((app_ctx.external_signer_address(), kwargs["shield"]))

    monkeypatch.setattr(context, "confirm_registration_sponsorship", capture_quote)
    monkeypatch.setattr(AppContext, "_transaction_card", capture_card)
    result = runner.invoke(
        app,
        [
            "--yes",
            "--json",
            "--network",
            "local",
            "wallet",
            "transfer",
            "--signer",
            "extension",
            "--dest",
            receiving_address(recipient),
            "--amount",
            "1",
            "--max-registration-deposit",
            "0.2",
            *([] if shielded else ["--no-mev-shield"]),
        ],
    )
    assert result.exit_code == 0, result.output
    assert [account.address for account in selected] == [ALICE]
    ensure.assert_awaited_once()
    assert cards == [(ALICE, shielded)]
    assert len(quotes) == 1 and quotes[0].payer_account == ALICE
    assert [signer.ss58_address for signer in opened] == [ALICE, ALICE]
    for signer in opened:
        signer.close.assert_awaited_once()
    if shielded:
        assert encrypt.await_args.args[1].ss58_address == ALICE
    else:
        assert fake.submissions[0][1] == ALICE
