"""Explicit wallet registration requires consent, Shield, and finalized records."""

import asyncio
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest

from bittensor import Balance, Client, SyncClient
from bittensor.hashed import descriptor_bytes, descriptor_storage, descriptor_value
from bittensor.sp_core import (
    CRYPTO_ED25519,
    CRYPTO_HASHED,
    CRYPTO_MLDSA65,
    CRYPTO_SR25519,
    FIXED_PQ_CRYPTO_TYPES,
    HASHED_CRYPTO_TYPES,
    Keypair,
    requires_registration,
)
from bittensor.wallet import Wallet
from bittensor.wallet_registration import WalletRegistrationError
from tests.harness.fake_substrate import FakeSubstrate, success_result
from tests.harness.samples import ALICE, dev_wallet

DEPOSIT = 200_000_000


def make_wallet(cold=CRYPTO_HASHED, hot=None, *, same_key=False):
    coldkey = Keypair.create_from_seed(bytes([41]) * 32, cold)
    hotkey = (
        coldkey
        if same_key
        else Keypair.create_from_seed(bytes([42]) * 32, cold if hot is None else hot)
    )
    return SimpleNamespace(coldkey=coldkey, coldkeypub=coldkey, hotkey=hotkey, hotkeypub=hotkey)


class RegistrationChain(FakeSubstrate):
    """Registration state is visible only at the block containing its snapshot."""

    def __init__(self, deposit=DEPOSIT):
        super().__init__()
        self.seed("AdminUtils", "HashedAccountsEnabled", [], True)
        self.seed("HashedAccounts", "HashedAccountRegistrationDeposit", [], deposit)
        self.seed_constant("HashedAccounts", "RegistrationDeposit", 1)
        self.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 2)
        self.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
        self.seed_constant("HashedAccounts", "FalconAccountProfiles", "0x06070809")
        self.seed("System", "Account", [ALICE], {"data": {"free": 10**12, "frozen": 0}})
        self.mev_key = bytes(1184)
        self.records = {self.finalized: {}}
        self.reads = []
        self.registration_submissions = []
        self.submission_error = None
        self.submission_result = success_result(self.finalized + 1)
        self.publish_records = True
        self.post_descriptor = None

    def register(self, key, *, descriptor=None):
        original = bytes(key.hashed_descriptor)
        self.records[self.finalized][(descriptor_storage(original), key.ss58_address)] = {
            "descriptor": descriptor_value(original if descriptor is None else descriptor)
        }

    async def query(self, module, item, params=None, block_hash=None):
        self.reads.append((module, item, params, block_hash))
        if module == "HashedAccounts" and item in ("Accounts", "MlDsaAccounts"):
            block = self.block if block_hash is None else int(block_hash, 16)
            return self.records.get(block, {}).get((item, params[0]))
        return await super().query(module, item, params, block_hash=block_hash)

    async def submit_registration(self, call, payer, **kwargs):
        self.registration_submissions.append((call, payer, kwargs))
        if self.submission_error is not None:
            raise self.submission_error
        if self.submission_result.success:
            records = dict(self.records[self.finalized])
            leaves = call.params["calls"] if call.module == "Utility" else [call]
            for leaf in leaves:
                if (leaf.module, leaf.function) == ("HashedAccounts", "check_registration_deposit"):
                    continue
                assert (leaf.module, leaf.function) == ("HashedAccounts", "register")
                value = next(iter(leaf.params["descriptor"].values()))
                descriptor = descriptor_bytes(value)
                account = Keypair.from_hashed_descriptor(descriptor).ss58_address
                if self.publish_records:
                    records[(descriptor_storage(descriptor), account)] = {
                        "descriptor": descriptor_value(self.post_descriptor or descriptor)
                    }
            self.finalized += 1
            self.block = self.finalized
            self.records[self.finalized] = records
        return self.submission_result


def setup_client(*, deposit=DEPOSIT):
    chain = RegistrationChain(deposit)
    client = Client(substrate=chain)
    client.submit_call = AsyncMock(side_effect=chain.submit_registration)
    return chain, client


def registration_leaves(client):
    call = client.submit_call.await_args.args[0]
    leaves = call.params["calls"] if call.module == "Utility" else [call]
    return [leaf for leaf in leaves if leaf.function == "register"]


@pytest.mark.parametrize("crypto", HASHED_CRYPTO_TYPES)
async def test_registers_both_rotating_keys_in_one_shielded_finalized_batch(crypto):
    chain, client = setup_client()
    wallet = make_wallet(crypto)
    payer = dev_wallet()
    outcome = await client.register_wallet(
        wallet, payer=payer, max_registration_deposit=Balance.from_rao(2 * DEPOSIT)
    )

    client.submit_call.assert_awaited_once()
    call, submitted_payer = client.submit_call.await_args.args
    assert submitted_payer is payer
    assert (call.module, call.function) == ("Utility", "batch_all")
    guard = call.params["calls"][0]
    assert (guard.module, guard.function) == ("HashedAccounts", "check_registration_deposit")
    assert guard.params == {"max_deposit": DEPOSIT}
    assert len(registration_leaves(client)) == 2
    options = client.submit_call.await_args.kwargs
    assert options["shielded"] is True
    assert options["wait_for_inclusion"] is True
    assert options["wait_for_finalization"] is True
    assert outcome.roles == {
        "coldkey": wallet.coldkeypub.ss58_address,
        "hotkey": wallet.hotkeypub.ss58_address,
    }
    assert set(outcome.accounts) == set(outcome.roles.values())
    assert outcome.deposit == Balance.from_rao(2 * DEPOSIT)
    assert outcome.result is chain.submission_result
    assert outcome.finalized_block_hash == await chain.block_hash(chain.finalized)
    registry_reads = [entry for entry in chain.reads if entry[0] == "HashedAccounts"]
    assert all(entry[3] is not None for entry in registry_reads)
    for account in outcome.accounts:
        assert any(
            params == [account] and block == outcome.finalized_block_hash
            for _, _, params, block in registry_reads
        )


@pytest.mark.parametrize("crypto", [CRYPTO_SR25519, CRYPTO_ED25519, *FIXED_PQ_CRYPTO_TYPES])
async def test_standard_wallets_need_no_registration_and_no_network(crypto):
    chain, client = setup_client()
    methods = ("query", "constant", "block_hash", "finalized_block_number", "mev_next_key")
    for method in methods:
        setattr(chain, method, AsyncMock(side_effect=AssertionError("unexpected network read")))
    outcome = await client.register_wallet(make_wallet(crypto), payer=dev_wallet())
    assert not outcome.accounts
    assert not outcome.roles
    assert outcome.deposit == Balance.from_rao(0)
    assert outcome.result is None
    assert outcome.finalized_block_hash is None
    client.submit_call.assert_not_awaited()
    for method in methods:
        getattr(chain, method).assert_not_awaited()


async def test_only_missing_rotating_descriptors_are_registered():
    chain, client = setup_client()
    wallet = make_wallet(CRYPTO_HASHED, CRYPTO_MLDSA65)
    chain.register(wallet.coldkey)
    approve = Mock(return_value=True)
    outcome = await client.register_wallet(
        wallet, payer=dev_wallet(), on_registration_sponsorship=approve
    )
    assert len(registration_leaves(client)) == 1
    quote = approve.call_args.args[0]
    assert quote.accounts == (wallet.hotkey.ss58_address,)
    assert quote.roles == {"hotkey": wallet.hotkey.ss58_address}
    assert quote.deposit == Balance.from_rao(DEPOSIT)
    assert quote.payer_account == ALICE
    assert quote.fee.rao > 0
    assert set(outcome.accounts) == {wallet.coldkey.ss58_address, wallet.hotkey.ss58_address}


async def test_registration_is_idempotent_after_finalized_verification():
    chain, client = setup_client()
    wallet = make_wallet()
    for key in (wallet.coldkey, wallet.hotkey):
        chain.register(key)
    approve = Mock(side_effect=AssertionError("already registered"))
    outcome = await client.register_wallet(
        wallet, payer=dev_wallet(), on_registration_sponsorship=approve
    )
    assert outcome.result is None
    assert outcome.deposit == Balance.from_rao(0)
    assert outcome.finalized_block_hash == await chain.block_hash(chain.finalized)
    client.submit_call.assert_not_awaited()
    approve.assert_not_called()


async def test_shared_coldkey_and_hotkey_descriptor_reserves_only_once():
    _, client = setup_client()
    wallet = make_wallet(same_key=True)
    outcome = await client.register_wallet(
        wallet, payer=dev_wallet(), max_registration_deposit=Balance.from_rao(DEPOSIT)
    )
    assert len(registration_leaves(client)) == 1
    assert outcome.accounts == (wallet.coldkey.ss58_address,)
    assert outcome.roles["coldkey"] == outcome.roles["hotkey"]
    assert outcome.deposit == Balance.from_rao(DEPOSIT)


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
async def test_role_selection_registers_only_the_requested_key(role):
    _, client = setup_client()
    wallet = make_wallet()
    outcome = await client.register_wallet(
        wallet,
        payer=dev_wallet(),
        roles=(role,),
        max_registration_deposit=Balance.from_rao(DEPOSIT),
    )
    assert outcome.roles == {role: getattr(wallet, role).ss58_address}
    assert len(registration_leaves(client)) == 1


async def test_mixed_wallet_registers_only_its_rotating_key():
    _, client = setup_client()
    wallet = make_wallet(CRYPTO_SR25519, CRYPTO_MLDSA65)
    outcome = await client.register_wallet(
        wallet, payer=dev_wallet(), max_registration_deposit=Balance.from_rao(DEPOSIT)
    )
    assert outcome.roles == {"hotkey": wallet.hotkey.ss58_address}
    assert len(registration_leaves(client)) == 1


async def test_registering_target_wallet_never_opens_its_private_keys():
    _, client = setup_client()
    keys = make_wallet()

    class PublicOnlyWallet:
        coldkeypub = keys.coldkeypub
        hotkeypub = keys.hotkeypub

        @property
        def coldkey(self):
            raise AssertionError("opened target coldkey")

        @property
        def hotkey(self):
            raise AssertionError("opened target hotkey")

    outcome = await client.register_wallet(
        PublicOnlyWallet(),
        payer=dev_wallet(),
        max_registration_deposit=Balance.from_rao(2 * DEPOSIT),
    )
    assert set(outcome.accounts) == {keys.coldkey.ss58_address, keys.hotkey.ss58_address}


@pytest.mark.parametrize("deposit", [0, DEPOSIT, 350_000_000])
async def test_quotes_live_shared_deposit_instead_of_old_per_scheme_constants(deposit):
    _, client = setup_client(deposit=deposit)
    outcome = await client.register_wallet(
        make_wallet(CRYPTO_HASHED, CRYPTO_MLDSA65),
        payer=dev_wallet(),
        max_registration_deposit=Balance.from_rao(2 * deposit),
    )
    assert outcome.deposit == Balance.from_rao(2 * deposit)
    call = client.submit_call.await_args.args[0]
    assert (call.module, call.function) == ("Utility", "batch_all")
    assert call.params["calls"][0].params == {"max_deposit": deposit}


async def test_absent_consent_stops_before_submission():
    _, client = setup_client()
    with pytest.raises(WalletRegistrationError, match=r"consent|authoriz|approval"):
        await client.register_wallet(make_wallet(), payer=dev_wallet())
    client.submit_call.assert_not_awaited()


async def test_insufficient_cap_cannot_be_overridden_by_callback():
    _, client = setup_client()
    approve = Mock(return_value=True)
    with pytest.raises(WalletRegistrationError, match=r"maximum|cap|exceed"):
        await client.register_wallet(
            make_wallet(),
            payer=dev_wallet(),
            max_registration_deposit=Balance.from_rao(2 * DEPOSIT - 1),
            on_registration_sponsorship=approve,
        )
    approve.assert_not_called()
    client.submit_call.assert_not_awaited()


@pytest.mark.parametrize("answer", [False, None, 1, "yes"])
async def test_callback_requires_literal_true(answer):
    _, client = setup_client()
    approve = AsyncMock(return_value=answer)
    with pytest.raises(WalletRegistrationError, match=r"declin|consent|authoriz"):
        await client.register_wallet(
            make_wallet(), payer=dev_wallet(), on_registration_sponsorship=approve
        )
    approve.assert_awaited_once()
    client.submit_call.assert_not_awaited()


async def test_async_callback_can_authorize_the_current_quote():
    _, client = setup_client()
    approve = AsyncMock(return_value=True)
    outcome = await client.register_wallet(
        make_wallet(), payer=dev_wallet(), on_registration_sponsorship=approve
    )
    approve.assert_awaited_once()
    assert approve.await_args.args[0].deposit == outcome.deposit
    assert outcome.result.success


@pytest.mark.parametrize("cap", [0.2, "0.2", True, Balance.from_rao(-1), Balance.from_alpha(1, 2)])
async def test_ambiguous_or_invalid_reserve_caps_fail_before_network(cap):
    chain, client = setup_client()
    chain.finalized_block_number = AsyncMock(side_effect=AssertionError("unexpected planning"))
    with pytest.raises(
        (TypeError, ValueError, WalletRegistrationError), match=r"native TAO Balance"
    ):
        await client.register_wallet(
            make_wallet(), payer=dev_wallet(), max_registration_deposit=cap
        )
    chain.finalized_block_number.assert_not_awaited()
    client.submit_call.assert_not_awaited()


async def test_mismatched_finalized_registration_is_not_overwritten():
    chain, client = setup_client()
    wallet = make_wallet()
    chain.register(wallet.coldkey, descriptor=bytes(wallet.hotkey.hashed_descriptor))
    with pytest.raises(WalletRegistrationError, match=r"descriptor|match"):
        await client.register_wallet(
            wallet, payer=dev_wallet(), max_registration_deposit=Balance.from_rao(2 * DEPOSIT)
        )
    client.submit_call.assert_not_awaited()


async def test_paused_registration_fails_before_submission():
    chain, client = setup_client()
    chain.seed("AdminUtils", "HashedAccountsEnabled", [], False)
    with pytest.raises(WalletRegistrationError, match=r"enabled|paused|disabled"):
        await client.register_wallet(
            make_wallet(),
            payer=dev_wallet(),
            max_registration_deposit=Balance.from_rao(2 * DEPOSIT),
        )
    client.submit_call.assert_not_awaited()


async def test_shield_unavailability_never_falls_back_to_plain_submission():
    chain, client = setup_client()
    chain.mev_key = None
    with pytest.raises(WalletRegistrationError, match=r"Shield|shield"):
        await client.register_wallet(
            make_wallet(),
            payer=dev_wallet(),
            max_registration_deposit=Balance.from_rao(2 * DEPOSIT),
        )
    client.submit_call.assert_not_awaited()
    assert not chain.submissions


@pytest.mark.parametrize("mismatch", [False, True])
async def test_success_receipt_is_insufficient_without_matching_finalized_records(mismatch):
    chain, client = setup_client()
    wallet = make_wallet()
    if mismatch:
        chain.post_descriptor = bytes(make_wallet(same_key=True).hotkey.hashed_descriptor)
    else:
        chain.publish_records = False
    with pytest.raises(WalletRegistrationError, match=r"finaliz|descriptor|match") as caught:
        await client.register_wallet(
            wallet, payer=dev_wallet(), max_registration_deposit=Balance.from_rao(2 * DEPOSIT)
        )
    assert caught.value.wallet is wallet
    assert caught.value.result is chain.submission_result


async def test_unregistered_rotating_payer_is_rejected_before_submission():
    _, client = setup_client()
    with pytest.raises(WalletRegistrationError, match=r"payer|registered"):
        await client.register_wallet(
            make_wallet(),
            payer=make_wallet(CRYPTO_MLDSA65),
            max_registration_deposit=Balance.from_rao(2 * DEPOSIT),
        )
    client.submit_call.assert_not_awaited()


async def test_reserve_and_fee_must_both_fit_the_payer_balance():
    chain, client = setup_client()
    chain.seed("System", "Account", [ALICE], {"data": {"free": 2 * DEPOSIT, "frozen": 0}})
    with pytest.raises(WalletRegistrationError, match=r"balance|fund|afford"):
        await client.register_wallet(
            make_wallet(),
            payer=dev_wallet(),
            max_registration_deposit=Balance.from_rao(2 * DEPOSIT),
        )
    client.submit_call.assert_not_awaited()


@pytest.mark.parametrize("failure", ["transport", "dispatch"])
async def test_submission_failure_preserves_generated_keyfiles_and_recovery_context(
    tmp_path, failure
):
    chain, client = setup_client()
    wallet = Wallet("registration", path=str(tmp_path))
    for index, role in enumerate(("coldkey", "hotkey"), start=51):
        getattr(wallet, f"regenerate_{role}")(
            seed=bytes([index]) * 32,
            crypto_type=CRYPTO_HASHED,
            use_password=False,
            overwrite=True,
        )
    files = {
        Path(keyfile.path): Path(keyfile.path).read_bytes()
        for keyfile in (
            wallet.coldkey_file,
            wallet.coldkeypub_file,
            wallet.hotkey_file,
            wallet.hotkeypub_file,
        )
    }
    if failure == "transport":
        chain.submission_error = ConnectionError("connection lost during registration")
    else:
        chain.submission_result = replace(
            chain.submission_result, success=False, message="Rejected"
        )
    with pytest.raises(WalletRegistrationError) as caught:
        await client.register_wallet(
            wallet, payer=dev_wallet(), max_registration_deposit=Balance.from_rao(2 * DEPOSIT)
        )
    assert caught.value.wallet is wallet
    assert caught.value.quote.deposit == Balance.from_rao(2 * DEPOSIT)
    if failure == "dispatch":
        assert caught.value.result is chain.submission_result
    assert {path: path.read_bytes() for path in files} == files
    assert requires_registration(wallet.coldkeypub)
    assert requires_registration(wallet.hotkeypub)


def test_sync_client_forwards_registration_arguments_and_returns_the_result(monkeypatch):
    chain = RegistrationChain()
    client = SyncClient(substrate=chain)
    outcome = object()
    client._client.register_wallet = AsyncMock(return_value=outcome)
    monkeypatch.setattr(client, "_call", asyncio.run)
    wallet, payer = make_wallet(), dev_wallet()
    approve = Mock(return_value=True)
    cap = Balance.from_rao(DEPOSIT)
    try:
        assert (
            client.register_wallet(
                wallet,
                payer=payer,
                roles=("hotkey",),
                max_registration_deposit=cap,
                on_registration_sponsorship=approve,
            )
            is outcome
        )
        client._client.register_wallet.assert_awaited_once_with(
            wallet,
            payer=payer,
            roles=("hotkey",),
            max_registration_deposit=cap,
            on_registration_sponsorship=approve,
        )
    finally:
        client.close()


def test_sync_standard_wallet_does_not_connect(monkeypatch):
    client = SyncClient(substrate=RegistrationChain())
    run = Mock(side_effect=AssertionError("standard wallet must not connect"))
    monkeypatch.setattr(client, "_call", run)
    try:
        outcome = client.register_wallet(make_wallet(CRYPTO_SR25519), payer=dev_wallet())
        assert not outcome.accounts
        assert outcome.result is None
        run.assert_not_called()
    finally:
        client.close()
