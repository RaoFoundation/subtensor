"""Automatic registration must be explicitly paid for before any key is opened."""

from dataclasses import FrozenInstanceError
from unittest.mock import AsyncMock, Mock

import pytest

from bittensor import (
    Balance,
    Client,
    RegistrationSponsorship,
    RegistrationSponsorshipRequired,
    calls,
)
from bittensor.executor import Executor
from bittensor.hashed import descriptor_storage, descriptor_value, registration_descriptor_value
from bittensor.intents import Batch, Transfer
from bittensor.receiving import receiving_address
from bittensor.registration_sponsorship import registration_sponsorship_quote
from bittensor.sp_core import Keypair
from tests.harness.fake_substrate import FakeSubstrate, success_result
from tests.harness.samples import ALICE, BOB, dev_wallet

CLASSICAL_DEPOSIT = 200_000_000
MLDSA_DEPOSIT = 1_144_000_000


@pytest.fixture(params=[4, 6, 7, 8])
def setup(request):
    chain = FakeSubstrate()
    chain.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    chain.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", CLASSICAL_DEPOSIT)
    chain.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", MLDSA_DEPOSIT)
    chain.seed("System", "Account", [ALICE], {"data": {"free": 10**12, "frozen": 0}})
    key = Keypair.create_from_seed(bytes([22]) * 32, request.param)
    executor = Executor(chain)
    executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    deposit = MLDSA_DEPOSIT if request.param in (7, 8) else CLASSICAL_DEPOSIT
    return chain, executor, key, Transfer(receiving_address(key), 1), deposit


@pytest.mark.parametrize("method", ["execute", "submit_shielded"])
async def test_first_payment_requires_consent_before_unlock_or_any_submission(
    setup, monkeypatch, method
):
    chain, executor, key, intent, deposit = setup
    unlock = Mock(side_effect=AssertionError("private key accessed before consent"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    with pytest.raises(RegistrationSponsorshipRequired, match="requires consent") as caught:
        await getattr(executor, method)(intent, dev_wallet())
    quote = caught.value.quote
    assert quote.recipient_account == key.ss58_address
    assert quote.payer_account == ALICE
    assert quote.deposit == Balance.from_rao(deposit)
    assert "permanently reserves" in str(caught.value)
    unlock.assert_not_called()
    assert not chain.submissions
    executor._submit_encrypted_call.assert_not_awaited()


@pytest.mark.parametrize("method", ["execute", "submit_shielded"])
@pytest.mark.parametrize("headroom", [-1, 0, 1])
async def test_native_deposit_cap_authorizes_only_the_actual_composed_reserve(
    setup, method, headroom
):
    chain, executor, _, intent, deposit = setup
    kwargs = {"max_registration_deposit": Balance.from_rao(deposit + headroom)}
    if headroom < 0:
        with pytest.raises(RegistrationSponsorshipRequired, match="exceeds"):
            await getattr(executor, method)(intent, dev_wallet(), **kwargs)
        assert not chain.submissions
        executor._submit_encrypted_call.assert_not_awaited()
    else:
        assert (await getattr(executor, method)(intent, dev_wallet(), **kwargs)).success


@pytest.mark.parametrize("method", ["execute", "submit_shielded"])
@pytest.mark.parametrize("answer", [False, None, 1, "yes", True])
async def test_callback_requires_literal_true_for_the_current_quote(setup, method, answer):
    chain, executor, key, intent, deposit = setup
    callback = Mock(return_value=answer)
    if answer is True:
        assert (
            await getattr(executor, method)(
                intent, dev_wallet(), on_registration_sponsorship=callback
            )
        ).success
    else:
        with pytest.raises(RegistrationSponsorshipRequired, match="declined"):
            await getattr(executor, method)(
                intent, dev_wallet(), on_registration_sponsorship=callback
            )
        assert not chain.submissions
        executor._submit_encrypted_call.assert_not_awaited()
    callback.assert_called_once_with(
        RegistrationSponsorship(key.ss58_address, ALICE, Balance.from_rao(deposit))
    )


async def test_callback_cannot_override_explicit_cap(setup):
    _, executor, _, intent, deposit = setup
    callback = Mock(return_value=True)
    with pytest.raises(RegistrationSponsorshipRequired, match="exceeds"):
        await executor.execute(
            intent,
            dev_wallet(),
            max_registration_deposit=Balance.from_rao(deposit - 1),
            on_registration_sponsorship=callback,
        )
    callback.assert_not_called()


async def test_async_callback_and_quote_identity_are_supported(setup):
    _, executor, key, intent, deposit = setup
    callback = AsyncMock(return_value=True)
    assert (
        await executor.execute(intent, dev_wallet(), on_registration_sponsorship=callback)
    ).success
    quote = callback.await_args.args[0]
    assert quote == RegistrationSponsorship(key.ss58_address, ALICE, Balance.from_rao(deposit))
    with pytest.raises(FrozenInstanceError):
        quote.recipient_account = BOB


@pytest.mark.parametrize("method", ["execute", "submit_shielded"])
@pytest.mark.parametrize("cap", [0.2, "0.2", True, Balance.from_rao(-1), Balance.from_alpha(1, 2)])
async def test_deposit_cap_rejects_ambiguous_units_before_planning(setup, method, cap):
    _, executor, _, intent, _ = setup
    executor.plan = AsyncMock(side_effect=AssertionError("must validate cap before plan"))
    with pytest.raises((TypeError, ValueError), match="native TAO Balance"):
        await getattr(executor, method)(intent, dev_wallet(), max_registration_deposit=cap)
    executor.plan.assert_not_awaited()


@pytest.mark.parametrize("method", ["execute", "submit_shielded"])
@pytest.mark.parametrize("recipient_kind", ["registered", "classical"])
async def test_no_extra_confirmation_without_a_new_registration(setup, method, recipient_kind):
    chain, executor, key, intent, _ = setup
    if recipient_kind == "registered":
        chain.seed(
            "HashedAccounts",
            descriptor_storage(bytes(key.hashed_descriptor)),
            [key.ss58_address],
            {"descriptor": descriptor_value(bytes(key.hashed_descriptor))},
        )
    else:
        intent = Transfer(BOB, 1)
    callback = Mock(side_effect=AssertionError("no sponsorship required"))
    assert (
        await getattr(executor, method)(intent, dev_wallet(), on_registration_sponsorship=callback)
    ).success
    callback.assert_not_called()
    preview = await executor.preflight(intent, dev_wallet())
    assert preview.registration_sponsorship is None


async def test_plan_and_preflight_only_quote_and_do_not_authorize(setup, monkeypatch):
    chain, executor, key, intent, deposit = setup
    monkeypatch.setattr(
        "bittensor.executor.resolve_signer", Mock(side_effect=AssertionError("opened signer"))
    )
    plan = await executor.plan(intent, dev_wallet())
    preview = await executor.preflight(intent, dev_wallet())
    assert plan.ok
    assert preview.registration_sponsorship == RegistrationSponsorship(
        key.ss58_address, ALICE, Balance.from_rao(deposit)
    )
    assert not chain.submissions
    with pytest.raises(RegistrationSponsorshipRequired):
        await executor.execute(intent, dev_wallet())


async def test_changed_deposit_requires_new_approval_of_actual_plan(setup):
    chain, executor, _, intent, deposit = setup
    preview = await executor.preflight(intent, dev_wallet())
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", deposit + 1)
    chain.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", deposit + 1)
    callback = Mock(return_value=True)
    with pytest.raises(RegistrationSponsorshipRequired, match="exceeds"):
        await executor.execute(
            intent,
            dev_wallet(),
            max_registration_deposit=preview.registration_sponsorship.deposit,
            on_registration_sponsorship=callback,
        )
    callback.assert_not_called()


async def test_execute_tool_and_client_forward_the_bounded_consent(setup):
    chain, _, _, intent, deposit = setup
    client = Client(substrate=chain)
    assert (
        await client.execute_tool(
            "transfer",
            {"dest_ss58": intent.dest_ss58, "amount_tao": "1"},
            dev_wallet(),
            max_registration_deposit=Balance.from_rao(deposit),
        )
    ).success


async def test_required_shield_redirect_retains_both_authorizations(setup, monkeypatch):
    _, executor, _, intent, deposit = setup
    monkeypatch.setattr(Transfer, "mev_shield_required", True)
    executor.submit_shielded = AsyncMock(return_value=success_result())
    callback = Mock(return_value=True)
    cap = Balance.from_rao(deposit)
    await executor.execute(
        intent,
        dev_wallet(),
        max_registration_deposit=cap,
        on_registration_sponsorship=callback,
    )
    kwargs = executor.submit_shielded.await_args.kwargs
    assert kwargs["max_registration_deposit"] is cap
    assert kwargs["on_registration_sponsorship"] is callback


async def test_auto_registration_remains_forbidden_inside_wrapped_batch(setup):
    chain, executor, _, intent, deposit = setup
    callback = Mock(return_value=True)
    with pytest.raises(ValueError, match=r"first payment.*direct transfer"):
        await executor.execute(
            Batch([intent]),
            dev_wallet(),
            max_registration_deposit=Balance.from_rao(deposit),
            on_registration_sponsorship=callback,
        )
    callback.assert_not_called()
    assert not chain.submissions


async def test_explicitly_authored_registration_raw_call_is_not_automatic_sponsorship(setup):
    chain, executor, key, _, _ = setup
    call = calls.HashedAccounts.register(
        descriptor=registration_descriptor_value(bytes(key.hashed_descriptor))
    )
    assert (await executor.submit_call(call, dev_wallet())).success
    assert len(chain.submissions) == 1


@pytest.mark.parametrize("reserve", [-1, True, "200"])
def test_invalid_internal_quotes_fail_closed(reserve):
    with pytest.raises(ValueError, match="invalid automatic"):
        registration_sponsorship_quote(
            {"hashed_registration_max_deposit_rao": reserve, "hashed_registration": BOB}, ALICE
        )


def test_positive_quote_must_identify_recipient():
    with pytest.raises(ValueError, match="missing its recipient"):
        registration_sponsorship_quote({"hashed_registration_max_deposit_rao": 1}, ALICE)
