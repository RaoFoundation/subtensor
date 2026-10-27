"""Policy and previews account for reserves and the complete submission route."""

import json
from unittest.mock import AsyncMock, Mock

import pytest
from typer.testing import CliRunner

from bittensor import Balance, Client, calls
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.executor import Executor
from bittensor.hashed import descriptor_value
from bittensor.intents import Batch, CreatePureProxy, Policy, Transfer
from bittensor.receiving import receiving_address
from bittensor.result import PolicyError
from bittensor.sp_core import Keypair, ss58_decode, ss58_encode
from tests.harness.fake_substrate import FakeSubstrate, success_result
from tests.harness.samples import ALICE, BOB, dev_wallet
from tests.unit import test_cli

fake = test_cli.fake
wallet_dir = test_cli.wallet_dir

DEPOSIT = 200_000_000
INNER = 100
CARRIER = 70


def chain():
    substrate = FakeSubstrate()
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    substrate.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    for name in ("RegistrationDeposit", "MlDsaRegistrationDeposit", "DerivedRegistrationDeposit"):
        substrate.seed_constant("HashedAccounts", name, DEPOSIT)
    substrate.seed_constant("Balances", "ExistentialDeposit", 0)
    substrate.seed_constant("Proxy", "ProxyDepositBase", 30)
    substrate.seed_constant("Proxy", "ProxyDepositFactor", 20)
    substrate.seed_constant("Multisig", "DepositBase", 0)
    substrate.seed_constant("Multisig", "DepositFactor", 0)
    substrate.seed_default("System", "Account", {"data": {"free": 10**13, "frozen": 0}})
    substrate.fee = Balance.from_rao(INNER)
    substrate.estimate_shielded_carrier_fee = AsyncMock(return_value=Balance.from_rao(CARRIER))
    substrate.mev_key = bytes(1184)
    return substrate


def protected_wallet(substrate, crypto):
    key = Keypair.create_from_seed(bytes([82]) * 32, crypto)
    substrate.seed(
        "HashedAccounts",
        "MlDsaAccounts",
        [key.ss58_address],
        {"descriptor": descriptor_value(bytes(key.hashed_descriptor))},
    )
    return key


@pytest.mark.parametrize("crypto", [7])
@pytest.mark.parametrize("count", [1, 2])
@pytest.mark.parametrize("shielded", [False, True])
async def test_pure_creation_reserve_is_quoted_aggregated_and_enforced(
    crypto, count, shielded, monkeypatch
):
    substrate = chain()
    wallet = protected_wallet(substrate, crypto)
    intent = (
        CreatePureProxy(proxy_type="Any")
        if count == 1
        else Batch([CreatePureProxy(proxy_type="Any", index=i) for i in range(count)])
    )
    executor = Executor(substrate)
    plan = await executor.plan(intent, wallet, shielded=shielded, policy=Policy(max_spend_tao=0))
    assert not plan.ok
    assert plan.spend == Balance.from_rao(count * DEPOSIT)
    assert any("max_spend_tao" in v for v in plan.violations)
    assert any("killing the proxy does not refund" in e for e in plan.effects)
    assert plan.extras["pure_proxy_deposit_rao"] == count * 50
    preview = await executor.preflight(intent, wallet, shielded=shielded)
    assert any("permanent" in label for label, _ in preview.facts)
    assert preview.registration_sponsorship is None
    assert preview.required_free == Balance.from_rao(
        count * (DEPOSIT + 50) + INNER + (CARRIER if shielded else 0)
    )
    unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    with pytest.raises(PolicyError, match="max_spend_tao"):
        await executor.execute(intent, wallet, shielded=shielded, policy=Policy(max_spend_tao=0))
    unlock.assert_not_called()
    assert not substrate.submissions


async def test_pure_reserve_uses_runtime_value_and_aggregates_with_payment():
    substrate = chain()
    wallet = protected_wallet(substrate, 7)
    substrate.seed_constant("HashedAccounts", "DerivedRegistrationDeposit", DEPOSIT + 1)
    intent = Batch([CreatePureProxy(proxy_type="Any"), Transfer(BOB, 1)])
    executor = Executor(substrate)
    for headroom in (-1, 0):
        cap = Balance.from_rao(DEPOSIT + 1 + 10**9 + headroom)
        plan = await executor.plan(intent, wallet, shielded=False, policy=Policy(max_spend_tao=cap))
        assert plan.ok is (headroom == 0)
        assert plan.spend == Balance.from_rao(DEPOSIT + 1 + 10**9)


async def test_classical_pure_creation_keeps_ordinary_route_and_no_registration_cost():
    plan = await Executor(chain()).plan(
        CreatePureProxy(), dev_wallet(), policy=Policy(max_spend_tao=0)
    )
    assert plan.ok
    assert plan.call.function == "create_pure"
    assert plan.spend is None
    assert not plan.shielded


@pytest.mark.parametrize("shielded", [False, True])
async def test_pure_insufficient_funds_includes_both_deposits_and_selected_fees(shielded):
    substrate = chain()
    wallet = protected_wallet(substrate, 7)
    substrate.seed(
        "System",
        "Account",
        [wallet.ss58_address],
        {"data": {"free": DEPOSIT + 50 + INNER + (CARRIER if shielded else 0) - 1}},
    )
    plan = await Executor(substrate).plan(
        CreatePureProxy(proxy_type="Any"), wallet, shielded=shielded
    )
    assert not plan.ok
    assert any("reserve" in v or "deposit" in v for v in plan.violations)


@pytest.mark.parametrize("automatic", [False, True])
@pytest.mark.parametrize("headroom", [-1, 0, 1])
async def test_shield_fee_cap_uses_total_before_unlock(automatic, headroom, monkeypatch):
    substrate = chain()
    key = Keypair.create_from_seed(bytes([83]) * 32, 7)
    intent = Transfer(receiving_address(key), 1) if automatic else Transfer(BOB, 1)
    kwargs = {} if automatic else {"shielded": True}
    executor = Executor(substrate)
    executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    cap = Policy(max_fee_tao=Balance.from_rao(INNER + CARRIER + headroom))
    plan = await executor.plan(intent, dev_wallet(), policy=cap, period=16, **kwargs)
    assert plan.shielded
    assert plan.fee.rao == INNER + CARRIER
    assert plan.ok is (headroom >= 0)
    assert substrate.estimate_shielded_carrier_fee.await_args.kwargs["period"] == 16
    if headroom < 0:
        unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
        monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
        with pytest.raises(PolicyError, match="max_fee_tao"):
            await executor.execute(intent, dev_wallet(), policy=cap, **kwargs)
        unlock.assert_not_called()
        executor._submit_encrypted_call.assert_not_awaited()
    else:
        result = await executor.execute(
            intent,
            dev_wallet(),
            policy=cap,
            max_registration_deposit=Balance.from_rao(DEPOSIT),
            **kwargs,
        )
        assert result.success
        executor._submit_encrypted_call.assert_awaited_once()


@pytest.mark.parametrize("failed_component", ["inner", "carrier"])
async def test_unavailable_total_never_passes_fee_cap(failed_component, monkeypatch):
    substrate = chain()
    target = "estimate_fee" if failed_component == "inner" else "estimate_shielded_carrier_fee"
    setattr(substrate, target, AsyncMock(side_effect=RuntimeError("quote unavailable")))
    executor = Executor(substrate)
    policy = Policy(max_fee_tao=1)
    plan = await executor.plan(Transfer(BOB, 1), dev_wallet(), shielded=True, policy=policy)
    assert plan.fee is None
    assert not plan.ok
    unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    with pytest.raises(PolicyError, match="could not be estimated"):
        await executor.submit_shielded(Transfer(BOB, 1), dev_wallet(), policy=policy)
    unlock.assert_not_called()


async def test_opt_out_prices_only_inner_and_public_client_previews_total():
    substrate = chain()
    key = Keypair.create_from_seed(bytes([84]) * 32, 7)
    intent = Transfer(receiving_address(key), 1)
    client = Client(substrate=substrate)
    plan = await client.plan(
        intent, dev_wallet(), shielded=False, policy=Policy(max_fee_tao=Balance.from_rao(INNER))
    )
    assert plan.ok and not plan.shielded
    assert plan.fee.rao == INNER
    substrate.estimate_shielded_carrier_fee.assert_not_awaited()
    preview = await client.preflight(intent, dev_wallet())
    assert preview.estimated_fee.rao == INNER + CARRIER
    assert any("Total transaction fee" in label for label, _ in preview.facts)
    substrate.mev_key = None
    with pytest.warns(UserWarning, match="submitting account registration unshielded"):
        fallback = await client.plan(intent, dev_wallet())
    assert not fallback.shielded
    assert fallback.fee.rao == INNER


async def test_proxy_origin_pays_registration_while_delegate_only_pays_fees():
    substrate = chain()
    owner = protected_wallet(substrate, 7)
    substrate.seed("System", "Account", [BOB], {"data": {"free": INNER + CARRIER}})
    from bittensor.sp_core import Keypair

    delegate = Keypair.create_from_uri("//Bob")
    executor = Executor(substrate)
    plan = await executor.plan(
        CreatePureProxy(proxy_type="Any"), delegate, proxy_for=owner.ss58_address
    )
    assert plan.ok
    assert plan.spend.rao == DEPOSIT
    assert plan.extras["pure_proxy_reserves"] == {owner.ss58_address: DEPOSIT + 50}
    rejected = await executor.plan(
        CreatePureProxy(proxy_type="Any"),
        delegate,
        proxy_for=owner.ss58_address,
        policy=Policy(max_spend_tao=0),
    )
    assert not rejected.ok


@pytest.mark.parametrize("shielded", [False, True])
@pytest.mark.parametrize("headroom", [-1, 0, 1])
async def test_distinct_proxy_fee_payer_affordability_is_checked_before_unlock(
    shielded, headroom, monkeypatch
):
    substrate = chain()
    owner = protected_wallet(substrate, 7)
    delegate = Keypair.create_from_uri("//Bob")
    substrate.seed_constant("Balances", "ExistentialDeposit", 10)
    frozen = 25
    required = frozen + INNER + (CARRIER if shielded else 0)
    substrate.seed(
        "System", "Account", [BOB], {"data": {"free": required + headroom, "frozen": frozen}}
    )
    # The owner has exactly its own reserves and ED, independently of fees.
    substrate.seed("System", "Account", [owner.ss58_address], {"data": {"free": DEPOSIT + 50 + 10}})
    executor = Executor(substrate)
    intent = CreatePureProxy(proxy_type="Any")
    kwargs = {"proxy_for": owner.ss58_address, "shielded": shielded}
    preview = await executor.preflight(intent, delegate, **kwargs)
    assert preview.required_free == Balance.from_rao(required)
    assert preview.available_free == Balance.from_rao(required + headroom)
    plan = await executor.plan(intent, delegate, **kwargs)
    assert plan.ok is (headroom >= 0)
    if headroom < 0:
        unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
        monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
        with pytest.raises(PolicyError, match=r"carrier fee|transaction fee"):
            await executor.execute(intent, delegate, **kwargs)
        unlock.assert_not_called()
        assert not substrate.submissions


@pytest.mark.parametrize("route", ["announced", "multisig"])
async def test_nested_pure_reserve_checks_distinct_fee_payer(route, monkeypatch):
    from bittensor.intents.multisig import MultisigExecute, MultisigIntentAdapter
    from bittensor.intents.proxy import ExecuteProxyAnnounced
    from tests.harness.samples import ALICE

    substrate = chain()
    if route == "announced":
        owner = protected_wallet(substrate, 7)
        intent = Batch(
            [
                ExecuteProxyAnnounced(
                    delegate_ss58=BOB,
                    real_ss58=owner.ss58_address,
                    inner_op="create_pure_proxy",
                    inner_args={"proxy_type": "Any"},
                )
            ]
        )
    else:
        owner = substrate.multisig_account([ALICE, BOB], 2)
        substrate.seed(
            "HashedAccounts",
            "DerivedAuthorities",
            [owner.ss58_address],
            {"descriptor": {"Multisig": {}}},
        )
        semantic = CreatePureProxy(proxy_type="Any")
        intent = MultisigIntentAdapter(
            dispatch=MultisigExecute(
                threshold=2,
                other_signatories=[BOB],
                call=semantic.to_dict(),
                timepoint={"height": 1, "index": 1},
            ),
            semantic=semantic,
        )
    substrate.seed("System", "Account", [ALICE], {"data": {"free": INNER + CARRIER - 1}})
    executor = Executor(substrate)
    plan = await executor.plan(intent, dev_wallet(), shielded=True)
    assert plan.extras["pure_proxy_reserves"] == {owner.ss58_address: DEPOSIT + 50}
    assert not plan.ok
    unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    with pytest.raises(PolicyError, match="carrier fee"):
        await executor.submit_shielded(intent, dev_wallet())
    unlock.assert_not_called()
    assert not substrate.submissions


async def test_distinct_fee_payer_rejects_unknown_carrier_quote_before_unlock(monkeypatch):
    substrate = chain()
    owner = protected_wallet(substrate, 7)
    delegate = Keypair.create_from_uri("//Bob")
    substrate.estimate_shielded_carrier_fee.side_effect = RuntimeError("quote unavailable")
    executor = Executor(substrate)
    unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    with pytest.raises(PolicyError, match="could not verify free TAO"):
        await executor.submit_shielded(
            CreatePureProxy(proxy_type="Any"), delegate, proxy_for=owner.ss58_address
        )
    unlock.assert_not_called()
    assert not substrate.submissions


async def test_pure_reserves_and_fees_aggregate_across_ss58_prefixes():
    from bittensor.intents.proxy import ExecuteProxyAnnounced

    substrate = chain()
    owner = protected_wallet(substrate, 7)
    alias = ss58_encode(bytes(ss58_decode(owner.ss58_address)), 0)
    substrate.seed(
        "HashedAccounts",
        "MlDsaAccounts",
        [alias],
        {"descriptor": descriptor_value(bytes(owner.hashed_descriptor))},
    )
    intent = Batch(
        [
            ExecuteProxyAnnounced(
                delegate_ss58=BOB,
                real_ss58=address,
                inner_op="create_pure_proxy",
                inner_args={"proxy_type": "Any", "index": index},
            )
            for index, address in enumerate([owner.ss58_address, alias])
        ]
    )
    required = 2 * (DEPOSIT + 50) + INNER + CARRIER
    executor = Executor(substrate)
    for headroom in (-1, 0):
        substrate.seed(
            "System", "Account", [owner.ss58_address], {"data": {"free": required + headroom}}
        )
        preview = await executor.preflight(intent, owner, shielded=True)
        assert preview.required_free == Balance.from_rao(required)
        assert bool(preview.blocks) is (headroom < 0)


@pytest.mark.parametrize("route", ["execute", "approve", "adapter", "group"])
@pytest.mark.parametrize("opening", [False, True])
@pytest.mark.parametrize("headroom", [-1, 0, 1])
async def test_multisig_approval_total_reserve_boundary_before_unlock(
    route, opening, headroom, monkeypatch
):
    from bittensor.intents.multisig import MultisigApprove, MultisigExecute, MultisigIntentAdapter
    from bittensor.multisig import MultisigFundingError

    substrate = chain()
    substrate.seed_constant("Multisig", "DepositBase", 500)
    substrate.seed_constant("Multisig", "DepositFactor", 50)
    substrate.seed_constant("Balances", "ExistentialDeposit", 10)
    deposit = 600 if opening else 0
    retained = 25
    required = deposit + INNER + CARRIER + retained
    substrate.seed(
        "System", "Account", [ALICE], {"data": {"free": required + headroom, "frozen": retained}}
    )
    wallet = dev_wallet()
    semantic = Transfer(BOB, Balance.from_rao(1))
    timepoint = None if opening else {"height": 1, "index": 2}
    client = Client(substrate=substrate)
    client._executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    if route == "group":
        group = await client.multisig([ALICE, BOB], 2)
        call = await client.compose(calls.Balances.transfer_keep_alive(dest=BOB, value=1))
        if not opening:
            substrate.seed(
                "Multisig",
                "Multisigs",
                [group.address, "0x" + bytes(call.call_hash).hex()],
                {"when": timepoint, "approvals": [BOB]},
            )

        async def action():
            return await group.approve(call, wallet, shielded=True)

        expected_error = MultisigFundingError
    else:
        dispatch_type = MultisigApprove if route == "approve" else MultisigExecute
        intent = dispatch_type(
            threshold=2, other_signatories=[BOB], call=semantic.to_dict(), timepoint=timepoint
        )
        if route == "adapter":
            intent = MultisigIntentAdapter(dispatch=intent, semantic=semantic)
        preview = await client.preflight(intent, wallet, shielded=True)
        assert preview.required_free == Balance.from_rao(required)
        assert any("refundable" in label for label, _ in preview.facts) is opening
        # Refundable approval deposits are affordability, not permanent spend.
        plan = await client.plan(
            intent, wallet, shielded=True, policy=Policy(max_spend_tao=Balance.from_rao(1))
        )
        assert plan.ok is (headroom >= 0)

        async def action():
            return await client.execute(intent, wallet, shielded=True)

        expected_error = PolicyError
    if headroom < 0:
        unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
        monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
        monkeypatch.setattr("bittensor.multisig.resolve_signer", unlock)
        with pytest.raises(expected_error):
            await action()
        unlock.assert_not_called()
        client._executor._submit_encrypted_call.assert_not_awaited()
    else:
        assert (await action()).success
        client._executor._submit_encrypted_call.assert_awaited_once()
        if route == "group":
            submitted = client._executor._submit_encrypted_call.await_args.args[0]
            assert submitted.params["maybe_timepoint"] == timepoint


@pytest.mark.parametrize("proxy", [False, True])
async def test_batched_multisig_deposits_aggregate_once_by_approval_origin(proxy):
    from bittensor.intents.multisig import MultisigExecute

    substrate = chain()
    substrate.seed_constant("Multisig", "DepositBase", 500)
    substrate.seed_constant("Multisig", "DepositFactor", 50)
    substrate.seed_constant("Balances", "ExistentialDeposit", 10)
    intent = Batch(
        [
            MultisigExecute(
                threshold=2,
                other_signatories=[BOB],
                call=Transfer(BOB, Balance.from_rao(value)).to_dict(),
            )
            for value in (1, 2)
        ]
    )
    wallet = dev_wallet("//Charlie") if proxy else dev_wallet()
    kwargs = {"proxy_for": ss58_encode(bytes(ss58_decode(ALICE)), 0)} if proxy else {}
    owner = kwargs.get("proxy_for", ALICE)
    substrate.seed(
        "System",
        "Account",
        [owner],
        {"data": {"free": 1200 + 10 + (0 if proxy else INNER + CARRIER)}},
    )
    substrate.seed(
        "System",
        "Account",
        [wallet.coldkey.ss58_address],
        {"data": {"free": (0 if proxy else 1200) + 10 + INNER + CARRIER}},
    )
    preview = await Executor(substrate).preflight(intent, wallet, shielded=True, **kwargs)
    assert not preview.blocks
    assert preview.required_free.rao == (0 if proxy else 1200) + 10 + INNER + CARRIER
    assert sum(label.startswith("Multisig approval reserve") for label, _ in preview.facts) == 1
    substrate.seed(
        "System",
        "Account",
        [owner],
        {"data": {"free": 1200 + 10 + (0 if proxy else INNER + CARRIER) - 1}},
    )
    assert not (await Executor(substrate).plan(intent, wallet, shielded=True, **kwargs)).ok


@pytest.mark.parametrize("opening", [False, True])
async def test_nested_multisig_does_not_reserve_unexecuted_approval_branch(opening):
    from bittensor.intents.multisig import MultisigExecute

    substrate = chain()
    substrate.seed_constant("Multisig", "DepositBase", 500)
    substrate.seed_constant("Multisig", "DepositFactor", 50)
    inner = MultisigExecute(threshold=2, other_signatories=[BOB], call=Transfer(BOB, 1).to_dict())
    outer = MultisigExecute(
        threshold=2,
        other_signatories=[BOB],
        call=inner.to_dict(),
        timepoint=None if opening else {"height": 1, "index": 1},
    )
    plan = await Executor(substrate).plan(outer, dev_wallet(), shielded=True)
    assert plan.ok
    records = plan.extras["multisig_approvals"]
    assert len(records) == (1 if opening else 2)
    assert sum(record["opening"] for record in records) == 1
    preview = await Executor(substrate).preflight(outer, dev_wallet(), shielded=True)
    assert preview.required_free.rao == INNER + CARRIER + (600 if opening else 0)


async def test_opening_defers_pure_affordability_but_preserves_permanent_spend_policy():
    from bittensor.intents.multisig import MultisigExecute, MultisigIntentAdapter

    substrate = chain()
    substrate.seed_constant("Multisig", "DepositBase", 500)
    substrate.seed_constant("Multisig", "DepositFactor", 50)
    group = substrate.multisig_account([ALICE, BOB], 2)
    substrate.seed(
        "HashedAccounts",
        "DerivedAuthorities",
        [group.ss58_address],
        {"descriptor": {"Multisig": {}}},
    )
    substrate.seed("System", "Account", [group.ss58_address], {"data": {"free": 0}})
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 600 + INNER + CARRIER}})
    semantic = CreatePureProxy(proxy_type="Any")
    intent = MultisigIntentAdapter(
        dispatch=MultisigExecute(threshold=2, other_signatories=[BOB], call=semantic.to_dict()),
        semantic=semantic,
    )
    executor = Executor(substrate)
    plan = await executor.plan(intent, dev_wallet(), shielded=True)
    assert plan.ok
    assert plan.spend.rao == DEPOSIT
    assert plan.extras["pure_proxy_reserves"] == {}
    preview = await executor.preflight(intent, dev_wallet(), shielded=True)
    assert preview.required_free.rao == 600 + INNER + CARRIER
    assert not (
        await executor.plan(intent, dev_wallet(), shielded=True, policy=Policy(max_spend_tao=0))
    ).ok
    # The final approval must require the inner pure reserves before signing.
    intent.dispatch.timepoint = {"height": 1, "index": 1}
    assert not (await executor.plan(intent, dev_wallet(), shielded=True)).ok


@pytest.mark.parametrize("component", ["deposit", "carrier"])
async def test_group_shield_unknown_quote_fails_before_unlock(component, monkeypatch):
    from bittensor.result import ChainError

    substrate = chain()
    if component == "deposit":
        substrate.seed_constant("Multisig", "DepositBase", None)
    else:
        substrate.estimate_shielded_carrier_fee.side_effect = RuntimeError("quote unavailable")
    client = Client(substrate=substrate)
    group = await client.multisig([ALICE, BOB], 2)
    unlock = Mock(side_effect=AssertionError("must enforce before unlock"))
    monkeypatch.setattr("bittensor.multisig.resolve_signer", unlock)
    with pytest.raises(ChainError, match="could not quote complete shielded multisig approval"):
        await group.approve(
            calls.Balances.transfer_keep_alive(dest=BOB, value=1), dev_wallet(), shielded=True
        )
    unlock.assert_not_called()


@pytest.mark.parametrize("alternate_prefix", [False, True])
async def test_group_threshold_one_shield_quotes_fee_only_and_excludes_signer_identity(
    alternate_prefix,
):
    substrate = chain()
    substrate.seed_constant("Multisig", "DepositBase", None)
    substrate.seed("System", "Account", [ALICE], {"data": {"free": INNER + CARRIER}})
    member = ss58_encode(bytes(ss58_decode(ALICE)), 0) if alternate_prefix else ALICE
    client = Client(substrate=substrate)
    group = await client.multisig([member, BOB], 1)
    client._executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    result = await group.approve(
        calls.Balances.transfer_keep_alive(dest=BOB, value=1), dev_wallet(), shielded=True
    )
    assert result.success
    call = client._executor._submit_encrypted_call.await_args.args[0]
    assert call.function == "as_multi_threshold_1"
    assert call.params["other_signatories"] == [BOB]


@pytest.mark.parametrize("batched", [False, True])
async def test_pinned_multisig_approval_keeps_registration_cost(batched):
    from bittensor.intents.multisig import MultisigExecute, MultisigIntentAdapter
    from tests.harness.samples import ALICE

    substrate = chain()
    group = substrate.multisig_account([ALICE, BOB], 2)
    substrate.seed(
        "HashedAccounts",
        "DerivedAuthorities",
        [group.ss58_address],
        {"descriptor": {"Multisig": {}}},
    )
    semantic = CreatePureProxy(proxy_type="Any")
    if batched:
        semantic = Batch([semantic, CreatePureProxy(proxy_type="Any", index=1)])
    dispatch = MultisigExecute(threshold=2, other_signatories=[BOB], call=semantic.to_dict())
    adapter = MultisigIntentAdapter(dispatch=dispatch, semantic=semantic)
    executor = Executor(substrate)
    original = await executor.plan(adapter, dev_wallet(), shielded=False)
    adapter.inner_call_data = original.extras["multisig_call_data"]
    from bittensor.fee_filters import _call_dict

    substrate.decode_scale = AsyncMock(return_value=_call_dict(original.call.params["call"]))
    plan = await executor.plan(
        adapter, dev_wallet(), shielded=False, policy=Policy(max_spend_tao=0)
    )
    assert not plan.ok
    assert plan.spend.rao == DEPOSIT * (2 if batched else 1)
    assert any("max_spend_tao" in v for v in plan.violations)
    adapter.inner_call_data = "0x00"
    with pytest.raises(ValueError, match="imported multisig call bytes do not match"):
        await executor.plan(adapter, dev_wallet(), shielded=False)


@pytest.mark.parametrize(
    "constant", ["DerivedRegistrationDeposit", "ProxyDepositBase", "ProxyDepositFactor"]
)
async def test_unknown_pure_creation_cost_fails_closed(constant):
    substrate = chain()
    wallet = protected_wallet(substrate, 7)
    substrate.seed_constant(
        "HashedAccounts" if constant.startswith("Derived") else "Proxy", constant, None
    )
    with pytest.raises(ValueError, match="could not quote"):
        await Executor(substrate).plan(CreatePureProxy(proxy_type="Any"), wallet)


@pytest.mark.parametrize("dry_run", [False, True])
@pytest.mark.parametrize("automatic", [False, True])
@pytest.mark.parametrize("opt_out", [False, True])
def test_cli_review_quotes_selected_route_total(fake, monkeypatch, dry_run, automatic, opt_out):
    fake.mev_key = bytes(1184)
    fake.seed("MevShield", "NextKey", [], fake.mev_key)
    fake.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    fake.seed_constant("HashedAccounts", "MldsaAccountProfiles", "0x0405")
    fake.seed_constant("HashedAccounts", "RegistrationDeposit", DEPOSIT)
    fake.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", DEPOSIT)
    fake.seed_default("System", "Account", {"data": {"free": 10**13, "frozen": 0}})
    fake.estimate_shielded_carrier_fee = AsyncMock(return_value=Balance.from_rao(CARRIER))
    encrypt = AsyncMock(return_value=success_result())
    monkeypatch.setattr(Executor, "_submit_encrypted_call", encrypt)
    cards = []
    monkeypatch.setattr(
        AppContext, "_transaction_card", lambda *args, **kwargs: cards.append(kwargs)
    )
    recipient = (
        receiving_address(Keypair.create_from_seed(bytes([85]) * 32, 7)) if automatic else BOB
    )
    args = [
        "--yes",
        "--json",
        "--network",
        "local",
        "wallet",
        "transfer",
        "--dest",
        recipient,
        "--amount",
        "1",
        "--max-registration-deposit",
        "0.2",
    ]
    if opt_out:
        args.append("--no-mev-shield")
    elif not automatic:
        args.append("--mev-shield")
    if dry_run:
        args.append("--dry-run")
    result = CliRunner().invoke(app, args)
    assert result.exit_code == 0, result.output
    total = fake.fee.rao + (0 if opt_out else CARRIER)
    if dry_run:
        plan = json.loads(result.output)
        assert Balance.from_tao(plan["fee_tao"]).rao == total
        assert plan["shielded"] is (not opt_out)
    else:
        assert cards[-1]["estimated_fee"].rao == total
        assert cards[-1]["shield"] is (not opt_out)
        assert bool(encrypt.await_count) is (not opt_out)


@pytest.mark.parametrize("batched", [False, True])
async def test_announced_proxy_preserves_reserve_cost_and_payer(batched):
    from bittensor.intents.proxy import ExecuteProxyAnnounced

    substrate = chain()
    owner = protected_wallet(substrate, 7)
    intent = ExecuteProxyAnnounced(
        delegate_ss58=BOB,
        real_ss58=owner.ss58_address,
        inner_op="create_pure_proxy",
        inner_args={"proxy_type": "Any"},
    )
    if batched:
        intent = Batch([intent])
    assert intent.requires_build_for_policy()
    plan = await Executor(substrate).plan(intent, dev_wallet(), policy=Policy(max_spend_tao=0))
    assert not plan.ok
    assert plan.spend.rao == DEPOSIT
    assert plan.extras["pure_proxy_reserves"] == {owner.ss58_address: DEPOSIT + 50}
    assert any("max_spend_tao" in v for v in plan.violations)


@pytest.mark.parametrize("period", [8, 16, 64])
async def test_explicit_shield_quote_uses_actual_submission_period(period):
    substrate = chain()
    executor = Executor(substrate)
    executor._submit_encrypted_call = AsyncMock(return_value=success_result())
    await executor.submit_shielded(Transfer(BOB, 1), dev_wallet(), period=period)
    assert substrate.estimate_shielded_carrier_fee.await_args.kwargs["period"] == period
    assert executor._submit_encrypted_call.await_args.kwargs["period"] == period
