"""Dry-run affordability across direct and delegated execution."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.intents.batch import Batch
from bittensor.intents.plan import Policy
from bittensor.intents.registration import RootRegister
from bittensor.intents.staking import AddStake
from bittensor.intents.transfer import Transfer
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, BOB_HOT, dev_wallet


def _account(free: int, *, reserved: int = 0, frozen: int = 0) -> dict:
    return {"data": {"free": free, "reserved": reserved, "frozen": frozen}}


@pytest.fixture()
def substrate() -> FakeSubstrate:
    return FakeSubstrate()


@pytest.fixture()
def client(substrate: FakeSubstrate) -> Client:
    return Client("local", substrate=substrate)


def _stake() -> AddStake:
    return AddStake(
        hotkey_ss58=BOB_HOT,
        netuid=0,
        amount_tao=1,
        slippage_protection=False,
    )


@pytest.mark.asyncio
async def test_static_spend_blocks_zero_balance(client: Client):
    plan = await client.plan(_stake(), dev_wallet())

    assert not plan.ok
    assert any("free TAO" in reason and "spend τ1" in reason for reason in plan.violations)


@pytest.mark.asyncio
async def test_static_spend_accepts_exact_spend_plus_fee(client: Client, substrate: FakeSubstrate):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    required = Balance.from_tao(1).rao + substrate.fee.rao + deposit
    substrate.seed("System", "Account", [ALICE], _account(required))

    plan = await client.plan(_stake(), dev_wallet())

    assert plan.ok, plan.violations


@pytest.mark.asyncio
async def test_static_spend_accounts_for_frozen_balance(client: Client, substrate: FakeSubstrate):
    frozen = 1_000
    required = Balance.from_tao(1).rao + substrate.fee.rao + frozen
    substrate.seed("System", "Account", [ALICE], _account(required - 1, frozen=frozen))

    plan = await client.plan(_stake(), dev_wallet())

    assert not plan.ok
    assert any("frozen balance" in reason for reason in plan.violations)


@pytest.mark.asyncio
async def test_reserved_balance_offsets_the_total_frozen_floor(
    client: Client, substrate: FakeSubstrate
):
    required = Balance.from_tao(1).rao + substrate.fee.rao
    frozen = 1_000
    reserved = 400
    substrate.seed(
        "System",
        "Account",
        [ALICE],
        _account(required + frozen - reserved, reserved=reserved, frozen=frozen),
    )

    exact = await client.plan(_stake(), dev_wallet())
    assert exact.ok, exact.violations

    substrate.seed(
        "System",
        "Account",
        [ALICE],
        _account(required + frozen - reserved - 1, reserved=reserved, frozen=frozen),
    )
    short = await client.plan(_stake(), dev_wallet())
    assert not short.ok
    assert any("frozen balance" in reason for reason in short.violations)


@pytest.mark.asyncio
async def test_fee_withdrawal_preserves_existential_deposit(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    transfer = Transfer(dest_ss58=BOB, amount_tao=Balance.from_rao(1), keep_alive=False)
    substrate.seed(
        "System",
        "Account",
        [ALICE],
        _account(substrate.fee.rao + deposit - 1),
    )

    short = await client.plan(transfer, dev_wallet())

    assert not short.ok
    assert any("existential deposit" in reason for reason in short.violations)

    substrate.seed("System", "Account", [ALICE], _account(substrate.fee.rao + deposit))
    exact = await client.plan(transfer, dev_wallet())
    assert exact.ok, exact.violations


@pytest.mark.asyncio
async def test_keep_alive_transfer_preserves_deposit_after_spend(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    amount = Balance.from_tao(1)
    transfer = Transfer(dest_ss58=BOB, amount_tao=amount, keep_alive=True)
    required = substrate.fee.rao + amount.rao + deposit
    substrate.seed("System", "Account", [ALICE], _account(required - 1))

    short = await client.plan(transfer, dev_wallet())

    assert not short.ok
    assert any("existential deposit" in reason for reason in short.violations)

    substrate.seed("System", "Account", [ALICE], _account(required))
    exact = await client.plan(transfer, dev_wallet())
    assert exact.ok, exact.violations


@pytest.mark.asyncio
async def test_allow_death_transfer_can_spend_the_post_fee_deposit(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    amount = Balance.from_rao(deposit)
    transfer = Transfer(dest_ss58=BOB, amount_tao=amount, keep_alive=False)
    substrate.seed("System", "Account", [ALICE], _account(substrate.fee.rao + amount.rao))

    plan = await client.plan(transfer, dev_wallet())

    assert plan.ok, plan.violations


@pytest.mark.asyncio
async def test_root_registration_exposes_and_checks_dynamic_burn(
    client: Client, substrate: FakeSubstrate
):
    burn = 2_000_000_000
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    substrate.seed("SubtensorModule", "Burn", [0], burn)
    substrate.seed(
        "System",
        "Account",
        [ALICE],
        _account(burn + substrate.fee.rao + deposit),
    )

    plan = await client.plan(RootRegister(), dev_wallet())

    assert plan.ok, plan.violations
    assert plan.to_dict()["spend_tao"] == pytest.approx(2.0)
    capped = await client.plan(RootRegister(), dev_wallet(), policy=Policy(max_spend_tao=1))
    assert any("spend τ2" in reason for reason in capped.violations)

    substrate.seed(
        "System",
        "Account",
        [ALICE],
        _account(burn + substrate.fee.rao + deposit - 1),
    )
    short = await client.plan(RootRegister(), dev_wallet())
    assert not short.ok
    assert any("existential deposit" in reason for reason in short.violations)


@pytest.mark.asyncio
async def test_root_registration_uses_one_burn_snapshot(
    client: Client, substrate: FakeSubstrate, monkeypatch
):
    original_query = substrate.query
    burn_reads = 0

    async def counted_query(module, item, params=None):
        nonlocal burn_reads
        if (module, item, params) == ("SubtensorModule", "Burn", [0]):
            burn_reads += 1
        return await original_query(module, item, params)

    monkeypatch.setattr(substrate, "query", counted_query)
    substrate.seed_default("System", "Account", _account(Balance.from_tao(10).rao))

    plan = await client.plan(RootRegister(), dev_wallet())

    assert plan.ok, plan.violations
    assert burn_reads == 1


@pytest.mark.asyncio
async def test_batch_only_preserves_the_prefix_that_needs_keep_alive(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    first = Balance.from_rao(100)
    last = Balance.from_rao(1_000)
    batch = Batch(
        intents=[
            Transfer(dest_ss58=BOB, amount_tao=first, keep_alive=True),
            Transfer(dest_ss58=BOB, amount_tao=last, keep_alive=False),
        ]
    )
    required = first.rao + last.rao + substrate.fee.rao
    assert first.rao + last.rao >= first.rao + deposit
    substrate.seed("System", "Account", [ALICE], _account(required))

    plan = await client.plan(batch, dev_wallet())

    assert plan.ok, plan.violations


@pytest.mark.asyncio
async def test_batch_preserves_deposit_when_keep_alive_spend_is_last(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    first = Balance.from_rao(1_000)
    last = Balance.from_rao(100)
    batch = Batch(
        intents=[
            Transfer(dest_ss58=BOB, amount_tao=first, keep_alive=False),
            Transfer(dest_ss58=BOB, amount_tao=last, keep_alive=True),
        ]
    )
    required = first.rao + last.rao + substrate.fee.rao + deposit
    substrate.seed("System", "Account", [ALICE], _account(required - 1))

    short = await client.plan(batch, dev_wallet())
    assert not short.ok
    assert any("existential deposit" in reason for reason in short.violations)

    substrate.seed("System", "Account", [ALICE], _account(required))
    exact = await client.plan(batch, dev_wallet())
    assert exact.ok, exact.violations


@pytest.mark.asyncio
async def test_proxy_checks_origin_spend_and_delegate_fee_separately(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    substrate.seed(
        "System",
        "Account",
        [BOB],
        _account(Balance.from_tao(1).rao + deposit),
    )
    substrate.seed("System", "Account", [ALICE], _account(substrate.fee.rao + deposit))

    plan = await client.plan(_stake(), dev_wallet(), proxy_for=BOB)
    assert plan.ok, plan.violations

    substrate.seed("System", "Account", [ALICE], _account(substrate.fee.rao + deposit - 1))
    short = await client.plan(_stake(), dev_wallet(), proxy_for=BOB)
    assert not short.ok
    assert any(ALICE in reason and "fee" in reason for reason in short.violations)

    substrate.seed("System", "Account", [ALICE], _account(substrate.fee.rao + deposit))
    substrate.seed(
        "System",
        "Account",
        [BOB],
        _account(Balance.from_tao(1).rao + deposit - 1),
    )
    short = await client.plan(_stake(), dev_wallet(), proxy_for=BOB)
    assert not short.ok
    assert any(BOB in reason and "spend" in reason for reason in short.violations)


@pytest.mark.asyncio
async def test_proxy_real_pays_fee_is_resolved_from_chain_state(
    client: Client, substrate: FakeSubstrate
):
    deposit = int(await substrate.constant("Balances", "ExistentialDeposit"))
    required = Balance.from_tao(1).rao + substrate.fee.rao + deposit
    substrate.seed_map("Proxy", "RealPaysFee", [(ALICE, ())])
    substrate.seed("System", "Account", [ALICE], _account(0))
    substrate.seed("System", "Account", [BOB], _account(required))

    plan = await client.plan(_stake(), dev_wallet(), proxy_for=BOB)

    assert plan.ok, plan.violations
    assert plan.signer_address == ALICE
    assert any(f"signed by {ALICE}" in effect for effect in plan.effects)

    substrate.seed("System", "Account", [BOB], _account(required - 1))
    short = await client.plan(_stake(), dev_wallet(), proxy_for=BOB)
    assert not short.ok
    assert any(BOB in reason and "fee" in reason for reason in short.violations)


@pytest.mark.asyncio
async def test_bounded_spend_fails_closed_when_fee_is_unavailable(
    client: Client, substrate: FakeSubstrate, monkeypatch
):
    substrate.seed("System", "Account", [ALICE], _account(Balance.from_tao(10).rao))

    async def unavailable_fee(*_args, **_kwargs):
        raise RuntimeError("payment info unavailable")

    monkeypatch.setattr(substrate, "estimate_fee", unavailable_fee)

    plan = await client.plan(_stake(), dev_wallet())

    assert not plan.ok
    assert any("transaction fee is unavailable" in reason for reason in plan.violations)


@pytest.mark.asyncio
async def test_bounded_spend_fails_closed_when_account_read_fails(
    client: Client, substrate: FakeSubstrate, monkeypatch
):
    original_query = substrate.query

    async def unavailable_account(module, item, params=None):
        if (module, item) == ("System", "Account"):
            raise RuntimeError("account storage unavailable")
        return await original_query(module, item, params)

    monkeypatch.setattr(substrate, "query", unavailable_account)

    plan = await client.plan(_stake(), dev_wallet())

    assert not plan.ok
    assert any("could not verify free TAO" in reason for reason in plan.violations)
