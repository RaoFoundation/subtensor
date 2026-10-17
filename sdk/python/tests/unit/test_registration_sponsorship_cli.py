"""The actual CLI requires separate consent before sponsoring a recipient."""

import json

import pytest
import typer
from typer.testing import CliRunner

from bittensor import Balance
from bittensor.cli import context
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.cli.registration import parse_registration_deposit
from bittensor.client import Client
from bittensor.hashed import descriptor_storage, descriptor_value
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from tests.unit import test_cli

fake = test_cli.fake
wallet_dir = test_cli.wallet_dir

runner = CliRunner()


@pytest.fixture(params=[4, 6, 7, 8])
def recipient(request, fake):
    key = Keypair.create_from_seed(bytes([67]) * 32, request.param)
    fake.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    fake.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    fake.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    fake.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 1_144_000_000)
    fake.seed_default("System", "Account", {"data": {"free": 10**13, "frozen": 0}})
    return key


def command(recipient, *extra, json_output=True):
    return [
        "--yes",
        *(["--json"] if json_output else []),
        "--network",
        "local",
        "wallet",
        "transfer",
        "--dest",
        receiving_address(recipient),
        "--amount",
        "1",
        "--no-mev-shield",
        *extra,
    ]


def amount(recipient):
    return "1.144" if recipient.crypto_type in (7, 8) else "0.2"


def test_yes_alone_does_not_consent(recipient, fake):
    result = runner.invoke(app, command(recipient))
    assert result.exit_code == 1, result.output
    assert "explicit registration sponsorship consent" in result.output
    assert amount(recipient) in result.output
    assert "--max-registration-deposit" in result.output
    assert not fake.submissions


@pytest.mark.parametrize("root_option", [False, True])
def test_explicit_limit_sponsors_exact_batch(recipient, fake, root_option):
    args = command(recipient)
    option = ["--max-registration-deposit", amount(recipient)]
    args = option + args if root_option else args + option
    result = runner.invoke(app, args)
    assert result.exit_code == 0, result.output
    assert len(fake.submissions) == 1
    call = fake.last_call
    assert (call.module, call.function) == ("Utility", "batch_all")
    registration, transfer = call.params["calls"]
    assert registration.function == "register"
    assert transfer.params["dest"] == recipient.ss58_address


def test_insufficient_limit_cannot_be_overridden(recipient, fake, monkeypatch):
    monkeypatch.setattr(Output, "confirm", lambda *a, **k: pytest.fail("must not prompt"))
    result = runner.invoke(app, command(recipient, "--max-registration-deposit", "0.000000001"))
    assert result.exit_code == 1, result.output
    assert "exceeding" in result.output
    assert not fake.submissions


def test_already_registered_needs_no_sponsorship(recipient, fake, monkeypatch):
    fake.seed(
        "HashedAccounts",
        descriptor_storage(bytes(recipient.hashed_descriptor)),
        [recipient.ss58_address],
        {"descriptor": descriptor_value(bytes(recipient.hashed_descriptor)), "generation": 0},
    )
    monkeypatch.setattr(Output, "confirm", lambda *a, **k: pytest.fail("must not prompt"))
    result = runner.invoke(app, command(recipient))
    assert result.exit_code == 0, result.output
    assert fake.last_call.params["calls"][0].function == "check_registered"


def test_dry_run_quotes_without_consent(recipient, fake, monkeypatch):
    monkeypatch.setattr(Output, "confirm", lambda *a, **k: pytest.fail("must not prompt"))
    result = runner.invoke(app, command(recipient, "--dry-run"))
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert "permanently" in str(plan)
    assert not fake.submissions


@pytest.mark.parametrize("answer,expected", [("y\n", 0), ("n\n", 1), ("\n", 1)])
def test_interactive_yes_no_and_default_no(recipient, fake, monkeypatch, answer, expected):
    monkeypatch.setattr("typer.testing._NamedTextIOWrapper.isatty", lambda _: True)
    result = runner.invoke(app, command(recipient, json_output=False), input=answer)
    assert result.exit_code == expected, result.output
    assert "permanently lock" in result.output
    assert amount(recipient) in result.output
    assert result.output.count("Register recipient account") == 1
    assert len(fake.submissions) == (1 if expected == 0 else 0)


def test_decline_happens_before_encrypted_wallet_unlock(recipient, fake, monkeypatch):
    monkeypatch.setattr(context, "_coldkey_encrypted", lambda _: True)
    monkeypatch.setattr(context.AppContext, "_unlock_coldkey", lambda *a: pytest.fail("unlocked"))
    result = runner.invoke(app, command(recipient))
    assert result.exit_code == 1, result.output
    assert "explicit registration sponsorship consent" in result.output
    assert not fake.submissions


def test_failed_sponsorship_preview_does_not_unlock(recipient, fake, monkeypatch):
    async def fail(*args, **kwargs):
        raise ValueError("registration preview unavailable")

    monkeypatch.setattr(Client, "preflight", fail)
    monkeypatch.setattr(context, "_coldkey_encrypted", lambda _: True)
    monkeypatch.setattr(context.AppContext, "_unlock_coldkey", lambda *a: pytest.fail("unlocked"))
    result = runner.invoke(app, command(recipient, "--max-registration-deposit", "2"))
    assert result.exit_code == 1, result.output
    assert "registration preview unavailable" in result.output
    assert not fake.submissions


def test_changed_quote_requires_new_confirmation(recipient, fake, monkeypatch):
    monkeypatch.setattr("typer.testing._NamedTextIOWrapper.isatty", lambda _: True)
    prompts = []

    def confirm(self, prompt, **kwargs):
        prompts.append(prompt)
        if len(prompts) == 1:
            constant = (
                "MlDsaRegistrationDeposit"
                if recipient.crypto_type in (7, 8)
                else "RegistrationDeposit"
            )
            fake.seed_constant("HashedAccounts", constant, 2_000_000_000)
            return True
        return False

    monkeypatch.setattr(Output, "confirm", confirm)
    result = runner.invoke(app, command(recipient, json_output=False))
    assert result.exit_code == 1, result.output
    assert len(prompts) == 2
    assert amount(recipient) in prompts[0]
    assert "2 TAO" in prompts[1]
    assert not fake.submissions


@pytest.mark.parametrize(
    "value",
    [
        "-1",
        "nan",
        "inf",
        "1e1000",
        "0.0000000001",
        "0.1999999999999999999999999999999999999999",
        "0.2000000000000000000000000000000000000001",
        "18446744073.709551616",
    ],
)
def test_invalid_cap_never_rounds_up(value):
    with pytest.raises(typer.BadParameter):
        parse_registration_deposit(value)


@pytest.mark.parametrize(
    "value,rao",
    [
        ("0", 0),
        (".2", 200_000_000),
        ("1.144", 1_144_000_000),
        ("0.000000001", 1),
        ("18446744073.709551615", 2**64 - 1),
    ],
)
def test_cap_is_exact(value, rao):
    assert parse_registration_deposit(value) == Balance.from_rao(rao)
