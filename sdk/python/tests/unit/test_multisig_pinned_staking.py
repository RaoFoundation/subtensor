"""Later approvals validate recipient guards against the original encoded call."""

import json
from dataclasses import replace
from unittest.mock import AsyncMock

import pytest

from bittensor._generated.runtime_apis import StakeInfoRuntimeApi, SwapRuntimeApi
from bittensor._transport.contract import CallBytes
from bittensor.executor import _compose_intent_call
from bittensor.hashed import _call_variants, descriptor_value
from bittensor.intents import Batch
from bittensor.intents.multisig import MultisigExecute, MultisigIntentAdapter
from bittensor.intents.proxy import ExecuteProxyAnnounced
from bittensor.intents.staking import (
    AddStake,
    AddStakeLimit,
    MoveStake,
    MoveSwapStake,
    RemoveStake,
    StakeIntoBasket,
    SwapStake,
)
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, dev_wallet


class CallCodec:
    """Opaque call bytes with the production decoder's nested-call shape."""

    def compose_call(self, module, function, params):
        def normalize(value):
            if isinstance(value, CallBytes):
                return json.loads(value.data)
            if isinstance(value, dict):
                if len(value) == 1:
                    name, inner = next(iter(value.items()))
                    if name in (
                        "Utility",
                        "Proxy",
                        "HashedAccounts",
                        "SubtensorModule",
                        "Crowdloan",
                        "EVM",
                        "Balances",
                    ):
                        method, args = next(iter(inner.items()))
                        return json.loads(self.compose_call(name, method, args).data)
                return {key: normalize(child) for key, child in value.items()}
            if isinstance(value, (list, tuple)):
                return [normalize(child) for child in value]
            return value

        decoded = {
            "call_module": module,
            "call_function": function,
            "call_args": [
                {"name": key, "value": normalize(value)} for key, value in sorted(params.items())
            ],
        }
        return CallBytes(json.dumps(decoded, sort_keys=True).encode(), 478)

    def decode(self, type_name, data):
        assert type_name == "Call"
        return json.loads(data)


@pytest.fixture
def setup():
    chain = FakeSubstrate()
    codec = CallCodec()
    chain.compose = AsyncMock(side_effect=lambda call: codec.compose_call(*call))
    chain.decode_scale = AsyncMock(side_effect=codec.decode)
    chain.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    chain.seed_runtime(*SwapRuntimeApi.current_alpha_price, 1_000_000_000)
    chain.seed_runtime(
        *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid, {"stake": 2_000_000_000}
    )
    for address in (ALICE, BOB):
        chain.seed("System", "Account", [address], {"data": {"free": 10**12, "frozen": 0}})
    return chain, codec


def register(chain, scheme, seed):
    key = Keypair.create_from_seed(bytes([seed]) * 32, scheme)
    chain.seed(
        "HashedAccounts",
        "Accounts",
        [key.ss58_address],
        {"descriptor": descriptor_value(bytes(key.hashed_descriptor))},
    )
    return receiving_address(key)


def staking(op, first, second, amount):
    return {
        "add": lambda: AddStake(first, 1, amount),
        "remove": lambda: RemoveStake(first, 1, amount),
        "move": lambda: MoveStake(first, 1, second, 2, amount),
        "swap": lambda: SwapStake(first, 1, 2, amount),
        "move_swap": lambda: MoveSwapStake(first, 1, second, 2, amount),
        "basket": lambda: StakeIntoBasket(first, amount),
    }[op]()


async def opening(chain, semantic, **kwargs):
    adapter = MultisigIntentAdapter(
        dispatch=MultisigExecute(2, [BOB], semantic.to_dict()), semantic=semantic
    )
    origin = adapter.origin_view(chain, dev_wallet())
    chain.seed("System", "Account", [origin.ss58_address], {"data": {"free": 10**12}})
    _, extras = await _compose_intent_call(chain, adapter, dev_wallet(), **kwargs)
    adapter.inner_call_data = extras["multisig_call_data"]
    adapter.dispatch = replace(
        adapter.dispatch, other_signatories=[ALICE], timepoint={"height": 100, "index": 1}
    )
    return adapter, extras


@pytest.mark.parametrize("scheme", [4, 6])
@pytest.mark.parametrize("op", ["add", "remove", "move", "swap", "move_swap", "basket"])
@pytest.mark.parametrize("amount", [1, "all"])
async def test_subsequent_approval_preserves_price_amount_and_guards(setup, scheme, op, amount):
    chain, codec = setup
    semantic = staking(op, register(chain, scheme, 92), register(chain, scheme, 93), amount)
    adapter, original = await opening(chain, semantic)
    chain.seed_runtime(
        *SwapRuntimeApi.current_alpha_price,
        lambda params: 1_010_000_000 if params[0] == 1 else 1_000_000_000,
    )
    chain.seed_runtime(
        *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid, {"stake": 3_000_000_000}
    )
    origin = adapter.origin_view(chain, dev_wallet("//Bob"))
    chain.seed("System", "Account", [origin.ss58_address], {"data": {"free": 20**12}})
    _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert later["multisig_call_data"] == original["multisig_call_data"]
    assert later["multisig_call_hash"] == original["multisig_call_hash"]
    decoded = codec.decode("Call", bytes.fromhex(later["multisig_call_data"][2:]))
    assert decoded["call_module"] == "Utility"
    assert decoded["call_function"] == "batch_all"

    # Replaying approved economic inputs must not depend on a new price quote.
    chain.runtime_call = AsyncMock(side_effect=AssertionError("unexpected quote"))
    _, again = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert again["multisig_call_data"] == original["multisig_call_data"]


@pytest.mark.parametrize("wrapper", ["batch", "proxy", "announced", "crowdloan"])
async def test_nested_staking_reuses_each_approved_limit(setup, wrapper):
    from bittensor.intents.crowdloan import CreateCrowdloan

    chain, _ = setup
    address = register(chain, 4, 92)
    semantic = AddStake(address, 1, 1)
    kwargs = {}
    if wrapper == "batch":
        semantic = Batch(
            [
                AddStakeLimit(address, 1, 1, 1_100_000_000),
                semantic,
                AddStake(address, 1, 2, rate_tolerance=0.1),
            ]
        )
    elif wrapper == "proxy":
        kwargs = {"proxy_for": ALICE, "proxy_type": "Any"}
    elif wrapper == "announced":
        semantic = ExecuteProxyAnnounced(BOB, ALICE, semantic.op, semantic.to_dict())
    else:
        semantic = CreateCrowdloan(1, 1, 10, 1000, call=semantic.to_dict())
    adapter, original = await opening(chain, semantic, **kwargs)
    chain.runtime_call = AsyncMock(side_effect=AssertionError("unexpected quote"))
    _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"), **kwargs)
    assert later["multisig_call_data"] == original["multisig_call_data"]


@pytest.mark.parametrize("scheme", [4, 6])
async def test_basket_approvals_consume_each_calls_amount(setup, scheme):
    chain, _ = setup
    address = register(chain, scheme, 92)
    semantic = Batch([StakeIntoBasket(address, 1), StakeIntoBasket(address, "all")])
    adapter, original = await opening(chain, semantic)
    # Even unchanged state must reproduce the exact approved basket amounts.
    _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert later["multisig_call_data"] == original["multisig_call_data"]
    origin = adapter.origin_view(chain, dev_wallet("//Bob"))
    chain.seed("System", "Account", [origin.ss58_address], {"data": {"free": 20**12}})
    _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert later["multisig_call_data"] == original["multisig_call_data"]
    adapter.semantic = Batch([StakeIntoBasket(address, 2), StakeIntoBasket(address, "all")])
    adapter.wrap_call = AsyncMock()
    with pytest.raises(ValueError, match="do not match the verified recipient"):
        await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    adapter.wrap_call.assert_not_awaited()


@pytest.mark.parametrize("tamper", ["amount", "hotkey", "netuid", "guard", "origin", "partial"])
async def test_pinned_staking_still_rejects_mismatched_payment_and_guards(setup, tamper):
    chain, codec = setup
    address = register(chain, 4, 92)
    adapter, _ = await opening(chain, AddStake(address, 1, 1))
    if tamper == "amount":
        adapter.semantic = replace(adapter.semantic, amount_tao=2)
    elif tamper == "hotkey":
        adapter.semantic = replace(adapter.semantic, hotkey_ss58=register(chain, 4, 93))
    elif tamper == "netuid":
        adapter.semantic = replace(adapter.semantic, netuid=2)
    elif tamper in ("guard", "partial"):
        decoded = codec.decode("Call", bytes.fromhex(adapter.inner_call_data[2:]))
        children = decoded["call_args"][0]["value"]
        if tamper == "guard":
            decoded = children[-1]
        else:
            for arg in children[-1]["call_args"]:
                if arg["name"] == "allow_partial":
                    arg["value"] = True
        call = codec.compose_call(
            decoded["call_module"],
            decoded["call_function"],
            {arg["name"]: _call_variants(arg["value"]) for arg in decoded["call_args"]},
        )
        adapter.inner_call_data = "0x" + call.data.hex()
    kwargs = {"proxy_for": ALICE} if tamper == "origin" else {}
    adapter.wrap_call = AsyncMock()
    with pytest.raises(ValueError, match="do not match the verified recipient"):
        await _compose_intent_call(chain, adapter, dev_wallet("//Bob"), **kwargs)
    adapter.wrap_call.assert_not_awaited()


async def test_explicit_limit_is_not_replaced_by_pinned_price(setup):
    chain, _ = setup
    address = register(chain, 4, 92)
    semantic = Batch([AddStakeLimit(address, 1, 1, 1_100_000_000), AddStake(address, 1, 1)])
    adapter, _ = await opening(chain, semantic)
    adapter.semantic = Batch(
        [
            AddStakeLimit(address, 1, 1, 1_200_000_000),
            AddStake(address, 1, 1),
        ]
    )
    adapter.wrap_call = AsyncMock()
    with pytest.raises(ValueError, match="do not match the verified recipient"):
        await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    adapter.wrap_call.assert_not_awaited()


async def test_pinned_context_does_not_leak_into_next_plan(setup):
    chain, _ = setup
    semantic = AddStake(register(chain, 4, 92), 1, 1)
    adapter, original = await opening(chain, semantic)
    await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    chain.seed_runtime(*SwapRuntimeApi.current_alpha_price, 1_010_000_000)
    fresh, _ = await _compose_intent_call(chain, semantic, dev_wallet())
    assert fresh.data.hex() != original["multisig_call_data"][2:]


DYNAMIC_OPERATIONS = [
    "crowdloan",
    "transfer",
    "transfer_hotkey",
    "lock",
    "swap_basket",
    "collateral",
    "burn",
    "withdraw",
    "take_up",
    "take_down",
]


def dynamic_intent(op, address, amount):
    from bittensor.intents.children import SetTake
    from bittensor.intents.collateral import AddCollateral
    from bittensor.intents.crowdloan import ContributeCrowdloan
    from bittensor.intents.evm import EvmWithdraw
    from bittensor.intents.governance import StakeBurn
    from bittensor.intents.lock import LockStake
    from bittensor.intents.staking import SwapBasket, TransferStake

    return {
        "crowdloan": lambda: ContributeCrowdloan(1, amount),
        "transfer": lambda: TransferStake(address, address, 1, 2, amount),
        "transfer_hotkey": lambda: TransferStake(address, ALICE, 1, 2, amount, address),
        "lock": lambda: LockStake(1, amount, address),
        "swap_basket": lambda: SwapBasket(address, 1, 2, amount),
        "collateral": lambda: AddCollateral(1, amount, address),
        "burn": lambda: StakeBurn(1, 1 if amount == "all" else amount, hotkey_ss58=address),
        "withdraw": lambda: EvmWithdraw(amount),
        "take_up": lambda: SetTake(100 if amount == "all" else int(amount) * 100, address),
        "take_down": lambda: SetTake(100 if amount == "all" else int(amount) * 100, address),
    }[op]()


def dynamic_state(chain, address, *, later, take_down=False):
    from bittensor._generated.runtime_apis import BetaBasketRuntimeApi

    value = 4_000_000_000 if later else 2_000_000_000
    chain.seed_default("System", "Account", {"data": {"free": value * 1000}})
    chain.seed_runtime(
        *StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
        lambda params: {params[0][0]: {1: {"total": value, "locked": 0, "available": value}}},
    )
    chain.seed_runtime(
        *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid, {"stake": value}
    )
    chain.seed_runtime(*BetaBasketRuntimeApi.get_validator_basket, [(1, value, value)])
    chain.seed_runtime(*SwapRuntimeApi.current_alpha_price, 1_010_000_000 if later else 10**9)
    from bittensor.receiving import parse_recipient

    chain.seed(
        "SubtensorModule",
        "Delegates",
        [parse_recipient(address).account],
        100 if later else 300 if take_down else 0,
    )


@pytest.mark.parametrize("scheme", [4, 6])
@pytest.mark.parametrize("op", DYNAMIC_OPERATIONS)
async def test_all_dynamic_inputs_survive_subsequent_approval(setup, scheme, op):
    from bittensor.intents import Transfer

    chain, _ = setup
    address = register(chain, scheme, 92)
    dynamic_state(chain, address, later=False, take_down=op == "take_down")
    # A guarded sibling also forces verification of calls without recipients.
    semantic = Batch([Transfer(address, 1), dynamic_intent(op, address, "all")])
    adapter, original = await opening(chain, semantic)
    _, same_state = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert same_state["multisig_call_data"] == original["multisig_call_data"]
    dynamic_state(chain, address, later=True)
    origin = adapter.origin_view(chain, dev_wallet("//Bob"))
    chain.seed("System", "Account", [origin.ss58_address], {"data": {"free": 20**12}})
    _, changed_state = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert changed_state["multisig_call_data"] == original["multisig_call_data"]
    assert changed_state["multisig_call_hash"] == original["multisig_call_hash"]
    chain.runtime_call = AsyncMock(side_effect=AssertionError("unexpected live quote"))
    _, no_quote = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert no_quote["multisig_call_data"] == original["multisig_call_data"]


@pytest.mark.parametrize("op", DYNAMIC_OPERATIONS)
async def test_dynamic_reconstruction_keeps_explicit_inputs_and_call_order(setup, op):
    from bittensor.intents import Transfer

    chain, _ = setup
    address = register(chain, 4, 92)
    dynamic_state(chain, address, later=False, take_down=op == "take_down")
    semantic = Batch(
        [Transfer(address, 1), dynamic_intent(op, address, 1), dynamic_intent(op, address, "all")]
    )
    adapter, original = await opening(chain, semantic)
    dynamic_state(chain, address, later=True)
    _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    assert later["multisig_call_data"] == original["multisig_call_data"]
    adapter.semantic = Batch(
        [Transfer(address, 1), dynamic_intent(op, address, 2), dynamic_intent(op, address, "all")]
    )
    adapter.wrap_call = AsyncMock()
    with pytest.raises(ValueError, match="do not match the verified recipient"):
        await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
    adapter.wrap_call.assert_not_awaited()


async def test_take_branch_order_survives_opposite_live_direction(setup):
    from bittensor.intents.children import DecreaseTake, IncreaseTake, SetTake
    from bittensor.receiving import parse_recipient

    chain, _ = setup
    address = register(chain, 4, 92)
    account = parse_recipient(address).account
    chain.seed("SubtensorModule", "Delegates", [account], 200)
    semantic = Batch(
        [
            IncreaseTake(300, address),
            SetTake(100, address),
            DecreaseTake(50, address),
            SetTake(400, address),
        ]
    )
    adapter, original = await opening(chain, semantic)
    for current in (0, 100, 400, 500):
        chain.seed("SubtensorModule", "Delegates", [account], current)
        _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"))
        assert later["multisig_call_data"] == original["multisig_call_data"]
