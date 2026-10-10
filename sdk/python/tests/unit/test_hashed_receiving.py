"""Receiving-address boundaries using real native keys and the SDK call harness.

The fake substrate records calls; FRAME tests separately prove atomic dispatch.
These tests exercise information preservation and rejection before signing.
"""

from unittest.mock import AsyncMock

import pytest

from bittensor import Balance, Client, Policy, PolicyError
from bittensor.executor import Executor, _compose_intent_call
from bittensor.hashed import descriptor_value
from bittensor.intents import Batch, Transfer, TransferAll
from bittensor.intents.coldkey import AnnounceColdkeySwap, SwapColdkeyAnnounced
from bittensor.intents.evm import FundEvmKey
from bittensor.intents.multisig import MultisigIntentAdapter, MultisigThreshold1
from bittensor.intents.registration import BurnedRegister, PowRegister, RegisterSubnet
from bittensor.intents.staking import MoveSwapStake, RemoveStake
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.sp_core import CRYPTO_HASHED, Keypair
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.receiving import legacy_receiving_address
from tests.harness.samples import ALICE, BOB, dev_wallet

GENESIS = bytes(32)
RESERVE = 200_000_000


@pytest.fixture
def setup():
    chain = FakeSubstrate()
    chain.seed_constant("HashedAccounts", "Enabled", True)
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", RESERVE)
    chain.seed("System", "Account", [ALICE], {"data": {"free": 10**12, "frozen": 0}})
    key = Keypair.create_from_seed(bytes([91]) * 32, CRYPTO_HASHED)
    return chain, dev_wallet(), key, receiving_address(key, GENESIS)


def registered(chain, key, generation=17):
    chain.seed(
        "HashedAccounts",
        "Accounts",
        [key.ss58_address],
        {
            "descriptor": descriptor_value(bytes(key.hashed_descriptor)),
            "generation": generation,
            "commitment": key.at_generation(generation).hashed_current_commitment,
        },
    )


def pinned(semantic):
    return MultisigIntentAdapter(
        dispatch=MultisigThreshold1(other_signatories=[BOB], call=semantic.to_dict()),
        semantic=semantic,
        inner_call_data="0x0000",
    )


def test_address_preserves_original_descriptor_after_rotation(setup):
    _, _, key, address = setup
    recipient = parse_recipient(address)
    assert len(address) == 104
    assert recipient.account == key.ss58_address
    assert recipient.descriptor == bytes(key.hashed_descriptor)
    assert receiving_address(key, bytes([9]) * 32) == address
    assert receiving_address(key.at_generation(123), GENESIS) == address


async def test_remote_payment_keeps_complete_input_and_registers_before_funding(setup):
    chain, wallet, key, address = setup
    intent = Transfer(address, 1)
    plan = await Executor(chain).plan(intent, wallet)
    assert not plan.violations
    assert intent.dest_ss58 == address
    assert plan.args["dest_ss58"] == address
    assert plan.call.function == "batch_all"
    guard, payment = plan.call.params["calls"]
    assert guard.params["descriptor"] == descriptor_value(bytes(key.hashed_descriptor))
    assert payment.params["dest"] == key.ss58_address
    assert payment.params["value"] == 10**9
    assert plan.spend.rao == 10**9 + RESERVE
    result = await Executor(chain).execute(intent, wallet)
    assert result.success
    assert chain.last_call == plan.call


async def test_remote_wallet_object_uses_only_its_public_file(tmp_path, setup, monkeypatch):
    chain, wallet, _, _ = setup
    remote = Wallet("remote", path=str(tmp_path / "another-machine"))
    remote.regenerate_coldkey(
        seed=bytes([92]) * 32, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
    )
    monkeypatch.setattr(
        Wallet,
        "coldkey",
        property(lambda _: pytest.fail("recipient private key must not be accessed")),
    )
    plan = await Executor(chain).plan(Transfer(remote, 1), wallet)
    assert plan.extras["hashed_registration"] == remote.coldkeypub.ss58_address
    assert plan.args["dest_ss58"].startswith("bth1_")


@pytest.mark.parametrize("case", ["checksum", "version", "whitespace", "disabled"])
async def test_bad_receiving_addresses_fail_before_composition_or_submission(setup, case):
    chain, wallet, _key, address = setup
    chain.compose = AsyncMock(wraps=chain.compose)
    if case == "checksum":
        address = address[:-2] + ("A" if address[-2] != "A" else "B") + address[-1]
    elif case == "version":
        address = "bth2_" + address[5:]
    elif case == "whitespace":
        address = " " + address
    else:
        chain.seed_constant("HashedAccounts", "Enabled", False)
    with pytest.raises(ValueError):
        await Executor(chain).execute(Transfer(address, 1), wallet)
    chain.compose.assert_not_awaited()
    assert not chain.submissions


@pytest.mark.parametrize("exists", [False, True])
async def test_policy_counts_maximum_registration_reserve_before_signing(setup, exists):
    chain, wallet, key, address = setup
    if exists:
        registered(chain, key)
    policy = Policy(max_spend_tao="1.1")
    plan = await Executor(chain).plan(Transfer(address, 1), wallet, policy=policy)
    assert any("max_spend" in item for item in plan.violations) == (not exists)
    assert plan.extras["hashed_registration_max_deposit_rao"] == (0 if exists else RESERVE)
    assert ("hashed_registration" not in plan.extras) == exists
    assert plan.call.params["calls"][0].function == ("check_registered" if exists else "register")
    if exists:
        assert (await Executor(chain).execute(Transfer(address, 1), wallet, policy=policy)).success
    else:
        with pytest.raises(PolicyError):
            await Executor(chain).execute(Transfer(address, 1), wallet, policy=policy)
        assert not chain.submissions


@pytest.mark.parametrize("keep_alive", [False, True])
async def test_first_payment_requires_fee_reserve_and_existential_deposit(setup, keep_alive):
    chain, wallet, _, address = setup
    required = 10**9 + RESERVE + chain.fee.rao + 500
    intent = Transfer(address, 1, keep_alive=keep_alive)
    for delta, blocked in [(-1, True), (0, False)]:
        chain.seed("System", "Account", [ALICE], {"data": {"free": required + delta}})
        preview = await Executor(chain).preflight(intent, wallet)
        assert preview.required_free.rao == required
        assert bool(preview.blocks) == blocked
    chain.estimate_fee = AsyncMock(side_effect=RuntimeError("offline"))
    with pytest.raises(PolicyError, match="could not quote"):
        await Executor(chain).execute(intent, wallet)
    assert not chain.submissions


async def test_send_all_stays_a_runtime_sweep_after_reserving_setup(setup):
    chain, wallet, _, address = setup
    plan = await Executor(chain).plan(TransferAll(address, keep_alive=False), wallet)
    guard, sweep = plan.call.params["calls"]
    assert guard.function == "register"
    assert sweep.function == "transfer_all"
    assert sweep.params["keep_alive"] is False
    assert "value" not in sweep.params


@pytest.mark.parametrize("form", ["typed", "explicit", "local"])
async def test_first_setup_inside_batch_is_rejected_for_every_recipient_form(
    setup, monkeypatch, form
):
    chain, wallet, key, address = setup
    if form == "typed":
        transfer = Transfer(address, 1)
    else:
        transfer = Transfer(key.ss58_address, 1)
        if form == "explicit":
            transfer.hashed_descriptor = bytes(key.hashed_descriptor).hex()
        else:
            monkeypatch.setattr(
                "bittensor.hashed._local_descriptor", lambda *_: bytes(key.hashed_descriptor)
            )
    with pytest.raises(ValueError, match="first payment"):
        await Executor(chain).execute(Batch([transfer]), wallet)
    assert not chain.submissions


async def test_registered_batch_flattens_guards_and_deduplicates_reserve(setup):
    chain, wallet, key, address = setup
    registered(chain, key)
    plan = await Executor(chain).plan(Batch([Transfer(address, 1), Transfer(address, 2)]), wallet)
    assert [call.function for call in plan.call.params["calls"]] == [
        "check_registered",
        "transfer_keep_alive",
        "check_registered",
        "transfer_keep_alive",
    ]
    assert plan.extras["hashed_registration_max_deposit_rao"] == 0
    assert plan.spend.rao == 3 * 10**9
    # A reorg makes the check fail; it cannot reserve funds or initialize an account.
    one = await Executor(chain).plan(
        Batch([Transfer(address, 1)]), wallet, policy=Policy(max_spend_tao="1.1")
    )
    assert not one.violations


@pytest.mark.parametrize("intent_type", [SwapColdkeyAnnounced, AnnounceColdkeySwap])
@pytest.mark.parametrize("form", ["typed", "local"])
async def test_coldkey_swap_uses_finalized_registration_without_a_batch(
    setup, monkeypatch, form, intent_type
):
    chain, wallet, key, address = setup
    registered(chain, key)
    if form == "local":
        address = key.ss58_address
        monkeypatch.setattr(
            "bittensor.hashed._local_descriptor", lambda *_: bytes(key.hashed_descriptor)
        )
    chain.query = AsyncMock(wraps=chain.query)
    finalized_hash = await chain.block_hash(await chain.finalized_block_number())
    intent = intent_type(address)
    plan = await Executor(chain).plan(intent, wallet)
    assert plan.call.module == "SubtensorModule"
    assert plan.call.function == intent.op
    assert plan.call == await intent_type(key.ss58_address).build(chain, wallet)
    assert plan.extras["hashed_registration_finalized_at"] == finalized_hash
    assert "hashed_registration_guards" not in plan.extras
    assert "hashed_registration_max_deposit_rao" not in plan.extras
    chain.query.assert_any_await(
        "HashedAccounts", "Accounts", [key.ss58_address], block_hash=finalized_hash
    )
    assert (await Executor(chain).execute(intent, wallet)).success
    assert chain.last_call == plan.call


@pytest.mark.parametrize("intent_type", [SwapColdkeyAnnounced, AnnounceColdkeySwap])
@pytest.mark.parametrize("finalized_state", ["missing", "mismatch", "unavailable"])
async def test_coldkey_swap_rejects_unverified_finalized_destination(
    setup, finalized_state, intent_type
):
    chain, wallet, key, address = setup
    # A matching best-head registration is insufficient: it may be reorged out.
    registered(chain, key)
    query = chain.query
    finalized_hash = await chain.block_hash(await chain.finalized_block_number())

    async def query_at(module, storage_function, params=None, block_hash=None):
        if (module, storage_function) == ("HashedAccounts", "Accounts"):
            if block_hash is None:
                return await query(module, storage_function, params, block_hash)
            assert block_hash == finalized_hash
            if finalized_state == "unavailable":
                raise RuntimeError("finalized state unavailable")
            if finalized_state == "missing":
                return None
            other = Keypair.create_from_seed(bytes([92]) * 32, CRYPTO_HASHED)
            return {"descriptor": descriptor_value(bytes(other.hashed_descriptor))}
        return await query(module, storage_function, params, block_hash)

    chain.query = AsyncMock(side_effect=query_at)
    error, message = {
        "missing": (ValueError, "registration is not finalized"),
        "mismatch": (ValueError, "descriptor does not match"),
        "unavailable": (RuntimeError, "finalized state unavailable"),
    }[finalized_state]
    with pytest.raises(error, match=message):
        await Executor(chain).execute(intent_type(address), wallet)
    assert not chain.submissions


@pytest.mark.parametrize("intent_type", [SwapColdkeyAnnounced, AnnounceColdkeySwap])
async def test_coldkey_swap_missing_finalized_hash_cannot_fall_back_to_head(setup, intent_type):
    chain, wallet, key, address = setup
    registered(chain, key)
    block_hash = chain.block_hash

    async def missing_finalized_hash(block=None):
        return await block_hash(block) if block == 0 else None

    chain.block_hash = AsyncMock(side_effect=missing_finalized_hash)
    with pytest.raises(ValueError, match="could not verify finalized"):
        await Executor(chain).execute(intent_type(address), wallet)
    assert not chain.submissions


@pytest.mark.parametrize("intent_type", [SwapColdkeyAnnounced, AnnounceColdkeySwap])
async def test_hashed_coldkey_swap_cannot_be_rewrapped_in_a_batch(setup, intent_type):
    chain, wallet, key, address = setup
    registered(chain, key)
    with pytest.raises(ValueError, match="submit the coldkey swap directly"):
        await Executor(chain).execute(Batch([intent_type(address)]), wallet)
    assert not chain.submissions


async def test_first_coldkey_announcement_keeps_atomic_registration_setup(setup):
    chain, wallet, _, address = setup
    plan = await Executor(chain).plan(AnnounceColdkeySwap(address), wallet)
    guard, announcement = plan.call.params["calls"]
    assert guard.function == "register"
    assert announcement.function == "announce_coldkey_swap"


async def test_classical_coldkey_swap_does_not_require_hashed_registration(setup):
    chain, wallet, _, _ = setup
    chain.finalized_block_number = AsyncMock(side_effect=AssertionError("unexpected lookup"))
    plan = await Executor(chain).plan(SwapColdkeyAnnounced(BOB), wallet)
    assert plan.call.function == "swap_coldkey_announced"
    assert plan.call.params == {"new_coldkey": BOB}


@pytest.mark.parametrize("operation", ["move_swap", "claim_unstake"])
@pytest.mark.parametrize("batched", [False, True])
async def test_registered_composite_operation_has_one_atomic_batch(setup, operation, batched):
    chain, wallet, key, address = setup
    registered(chain, key)
    if operation == "move_swap":
        intent = MoveSwapStake(BOB, 1, address, 2, 1, slippage_protection=False)
        expected = ["check_registered", "move_stake", "swap_stake"]
    else:
        intent = RemoveStake(address, 0, 1, slippage_protection=False, claim=True)
        expected = ["check_registered", "claim_root_with_hotkey", "remove_stake"]
    if batched:
        intent = Batch([intent])
    call, _ = await _compose_intent_call(chain, intent, wallet)
    assert call.function == "batch_all"
    assert [child.function for child in call.params["calls"]] == expected


async def test_atomic_flattening_round_trips_real_scale_calls():
    from bittensor._generated import calls
    from bittensor.hashed import atomic_calls
    from tests.conftest import golden_codec

    codec = golden_codec()
    chain = FakeSubstrate()
    chain.decode_scale = AsyncMock(side_effect=codec.decode)
    chain.compose = AsyncMock(side_effect=lambda call: codec.compose_call(*call))
    leaf = codec.compose_call(*calls.Balances.transfer_keep_alive(dest=BOB, value=123))
    nested = codec.compose_call(*calls.Utility.batch_all(calls=[leaf, leaf]))
    outer = codec.compose_call(*calls.Utility.batch_all(calls=[nested, leaf]))
    flat = await atomic_calls(chain, outer)
    recomposed = codec.compose_call(*calls.Utility.batch_all(calls=flat))
    assert recomposed.data == codec.compose_call(*calls.Utility.batch_all(calls=[leaf] * 3)).data
    # Flattening must retain a child's proxy origin and its encoded nested call.
    proxied = codec.compose_call(*calls.Proxy.proxy(real=ALICE, force_proxy_type=None, call=leaf))
    wrapped = codec.compose_call(*calls.Utility.batch_all(calls=[proxied]))
    flattened = await atomic_calls(chain, wrapped)
    assert codec.compose_call(*calls.Utility.batch_all(calls=flattened)).data == wrapped.data
    for wrapper in ["batch", "force_batch"]:
        non_atomic = codec.compose_call("Utility", wrapper, {"calls": [leaf]})
        assert await atomic_calls(chain, non_atomic) == [non_atomic]


@pytest.mark.parametrize("form", ["typed", "explicit", "default_hotkey", "evm", "evm_disabled"])
@pytest.mark.parametrize("in_batch", [False, True])
async def test_imported_multisig_bytes_cannot_bypass_recipient_information(setup, form, in_batch):
    chain, wallet, key, address = setup
    if form == "typed":
        semantic = Transfer(address, 1)
    elif form == "explicit":
        semantic = Transfer(
            key.ss58_address, 1, hashed_descriptor=bytes(key.hashed_descriptor).hex()
        )
    elif form == "default_hotkey":
        wallet.hotkey = key
        semantic = BurnedRegister(netuid=1)
    else:
        semantic = FundEvmKey("0x" + "12" * 20, 1)
        if form == "evm_disabled":
            chain.seed_constant("HashedAccounts", "Enabled", False)
            registered(chain, key)
            alias = "0x" + bytes(key.public_key)[:20].hex()
            chain.seed("HashedAccounts", "EvmAliases", [alias], key.ss58_address)
            semantic = FundEvmKey(alias, 1)
    if in_batch:
        semantic = Batch([semantic])
    adapter = pinned(semantic)
    adapter.wrap_call = AsyncMock()
    with pytest.raises(ValueError, match="imported multisig"):
        await _compose_intent_call(chain, adapter, wallet)
    adapter.wrap_call.assert_not_awaited()
    assert not chain.submissions


async def test_default_in_memory_hashed_hotkey_retains_setup_descriptor(setup):
    chain, wallet, key, _ = setup
    wallet.hotkey = key
    call, extras = await _compose_intent_call(chain, BurnedRegister(netuid=1), wallet)
    assert extras["hashed_registration"] == key.ss58_address
    guard, registration = call.params["calls"]
    assert guard.params["descriptor"] == descriptor_value(bytes(key.hashed_descriptor))
    assert registration.params["hotkey"] == key.ss58_address


async def test_pow_keeps_direct_fee_free_call_after_finalized_registration(setup):
    chain, wallet, key, _ = setup
    wallet.hotkey = key
    registered(chain, key)
    chain.query = AsyncMock(wraps=chain.query)
    intent = PowRegister(netuid=1, work_block=1, nonce=0, work_hex="00" * 32)
    call, extras = await _compose_intent_call(chain, intent, wallet)
    assert (call.module, call.function) == ("SubtensorModule", "pow_register")
    assert extras["hashed_registration_finalized_at"]
    registry_reads = [
        c for c in chain.query.call_args_list if c.args[:2] == ("HashedAccounts", "Accounts")
    ]
    assert registry_reads
    assert all(
        c.kwargs["block_hash"] == extras["hashed_registration_finalized_at"] for c in registry_reads
    )


async def test_pow_requires_sponsored_registration_before_mining_submission(setup):
    chain, wallet, key, _ = setup
    wallet.hotkey = key
    intent = PowRegister(netuid=1, work_block=1, nonce=0, work_hex="00" * 32)
    with pytest.raises(ValueError, match=r"register.*finaliz"):
        await _compose_intent_call(chain, intent, wallet)


async def test_registry_descriptor_mismatch_is_rejected(setup):
    chain, wallet, key, address = setup
    other = Keypair.create_from_seed(bytes([93]) * 32, CRYPTO_HASHED)
    chain.seed(
        "HashedAccounts",
        "Accounts",
        [key.ss58_address],
        {"descriptor": descriptor_value(bytes(other.hashed_descriptor))},
    )
    with pytest.raises(ValueError, match="registered hashed descriptor"):
        await Executor(chain).execute(Transfer(address, 1), wallet)
    assert not chain.submissions


async def test_reads_resolve_current_and_legacy_addresses_on_client_and_snapshot(setup):
    chain, _, key, address = setup
    client = Client("local", substrate=chain)
    chain.seed("System", "Account", [key.ss58_address], {"data": {"free": 123456}})
    for view in (client, await client.at(50)):
        assert await view.read("balance", coldkey_ss58=address) == Balance.from_rao(123456)
        assert await view.read("balance", coldkey_ss58=legacy_receiving_address(key)) == (
            Balance.from_rao(123456)
        )


async def test_proxy_identity_is_normalized_without_losing_payment_guard(setup):
    chain, wallet, key, address = setup
    registered(chain, key)
    plan = await Executor(chain).plan(Transfer(address, 1), wallet, proxy_for=address)
    assert plan.call.function == "proxy"
    assert plan.call.params["real"] == key.ss58_address
    assert plan.call.params["call"].function == "batch_all"


async def test_subnet_completion_uses_resolved_proxy_owner_and_hotkey(setup, monkeypatch):
    chain, wallet, key, address = setup
    registered(chain, key)
    chain.seed("System", "Account", [key.ss58_address], {"data": {"free": 10**12}})
    completion = AsyncMock()
    monkeypatch.setattr("bittensor.executor._complete_subnet_registration", completion)
    await Executor(chain).execute(RegisterSubnet(hotkey_ss58=address), wallet, proxy_for=address)
    assert completion.await_count == 1
    assert completion.await_args.kwargs["owner"] == key.ss58_address
    assert completion.await_args.kwargs["hotkey"] == key.ss58_address


@pytest.mark.parametrize("wrapper", ["batch", "multisig"])
async def test_wrapped_preflight_queries_the_internal_account(setup, monkeypatch, wrapper):
    chain, _, key, address = setup
    registered(chain, key)
    semantic = BurnedRegister(netuid=1, hotkey_ss58=address)
    seen = []
    original = BurnedRegister.preflight

    async def checked(self, *args, **kwargs):
        seen.append(self.hotkey_ss58)
        return await original(self, *args, **kwargs)

    monkeypatch.setattr(BurnedRegister, "preflight", checked)
    wrapped = Batch([semantic]) if wrapper == "batch" else pinned(semantic)
    await wrapped.preflight(chain, ALICE, ALICE)
    assert seen == [key.ss58_address]


@pytest.mark.parametrize("code", [0, 1, 4, 5])
def test_all_wallet_addresses_are_network_independent(code):
    key = Keypair.create_from_seed(bytes([39]) * 32, code)
    assert receiving_address(key) == receiving_address(key, bytes(32))
    assert receiving_address(key) == receiving_address(key, bytes([17]) * 32)
    assert parse_recipient(receiving_address(key)).account == key.ss58_address


@pytest.mark.parametrize("code", [4, 5])
@pytest.mark.parametrize("registered_here", [False, True])
async def test_legacy_address_transfers_on_another_chain(code, registered_here):
    chain = FakeSubstrate()
    chain.seed_constant("HashedAccounts", "Enabled", True)
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", RESERVE)
    key = Keypair.create_from_seed(bytes([39]) * 32, code)
    if registered_here:
        registered(chain, key)
    address = legacy_receiving_address(key)
    assert address != receiving_address(key)
    assert parse_recipient(address).account == key.ss58_address
    plan = await Executor(chain).plan(Transfer(address, 1), dev_wallet())
    guard, transfer = plan.call.params["calls"]
    assert guard.function == ("check_registered" if registered_here else "register")
    assert guard.params["descriptor"] == descriptor_value(bytes(key.hashed_descriptor))
    assert transfer.params["dest"] == key.ss58_address
