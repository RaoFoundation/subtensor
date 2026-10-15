"""Nested childkey identities retain receiving descriptors through composition."""

from dataclasses import replace

import pytest

from bittensor.executor import _compose_intent_call
from bittensor.hashed import descriptor_value, prepare_recipient_intent
from bittensor.intents import Batch
from bittensor.intents.children import SetChildren
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from bittensor.wallet import Wallet
from tests.harness.samples import ALICE, BOB, dev_wallet
from tests.unit import test_multisig_pinned_staking as pinned_staking
from tests.unit.test_hashed_receiving import registered
from tests.unit.test_hashed_receiving import setup as setup

pinned_setup = pinned_staking.setup


@pytest.mark.parametrize("scheme", [4, 6])
@pytest.mark.parametrize("state", ["registered", "unregistered", "disabled", "mismatched"])
async def test_children_receiving_addresses_keep_registration_guards(setup, scheme, state):
    chain, wallet, _, _ = setup
    child = Keypair.create_from_seed(bytes([91]) * 32, scheme)
    if state != "unregistered":
        registered(chain, child)
    if state == "disabled":
        chain.seed_constant("HashedAccounts", "Enabled", False)
    elif state == "mismatched":
        other = Keypair.create_from_seed(bytes([92]) * 32, scheme)
        chain.seed(
            "HashedAccounts",
            "Accounts",
            [child.ss58_address],
            {"descriptor": descriptor_value(bytes(other.hashed_descriptor))},
        )
    address = receiving_address(child)
    intent = SetChildren(1, [[0.5, address], [0.25, BOB]], hotkey_ss58=ALICE)
    if state != "registered":
        match = {
            "unregistered": "recipient is not registered",
            "disabled": "not supported",
            "mismatched": "does not match",
        }[state]
        with pytest.raises(ValueError, match=match):
            await _compose_intent_call(chain, intent, wallet)
    else:
        prepared, recipients = await prepare_recipient_intent(chain, intent)
        assert intent.children[0][1] == address
        assert prepared.children == [
            [intent.children[0][0], child.ss58_address],
            [intent.children[1][0], BOB],
        ]
        assert recipients["children[0][1]"].descriptor == bytes(child.hashed_descriptor)
        call, extras = await _compose_intent_call(chain, intent, wallet)
        guard, assignment = call.params["calls"]
        assert (guard.module, guard.function) == ("HashedAccounts", "check_registered")
        assert guard.params["descriptor"] == descriptor_value(bytes(child.hashed_descriptor))
        assert assignment.function == "set_children"
        assert assignment.params["children"] == [tuple(entry) for entry in prepared.children]
        assert extras["hashed_registration_guards"] == [child.ss58_address]
    assert not chain.submissions


@pytest.mark.parametrize("scheme", [4, 6])
@pytest.mark.parametrize("state", ["registered", "unregistered", "disabled"])
@pytest.mark.parametrize("proxy", [False, True])
async def test_local_child_ss58_keeps_descriptor_context(setup, tmp_path, scheme, state, proxy):
    chain, _, _, _ = setup
    key = Keypair.create_from_seed(bytes([91]) * 32, scheme)
    wallet = Wallet("sender", path=str(tmp_path))
    wallet.coldkeypub_file.set_keypair(Keypair(ss58_address=ALICE), encrypt=False)
    Wallet("child", path=str(tmp_path)).hotkeypub_file.set_keypair(key.public_only(), encrypt=False)
    if state == "registered":
        registered(chain, key)
    if state == "disabled":
        chain.seed_constant("HashedAccounts", "Enabled", False)
    intent = Batch([SetChildren(1, [[0.5, key.ss58_address]], hotkey_ss58=ALICE)])
    kwargs = {"proxy_for": BOB} if proxy else {}
    if state != "registered":
        match = "not enabled" if state == "disabled" else "recipient is not registered"
        with pytest.raises(ValueError, match=match):
            await _compose_intent_call(chain, intent, wallet, **kwargs)
    else:
        call, extras = await _compose_intent_call(chain, intent, wallet, **kwargs)
        batch = call.params["call"] if proxy else call
        guard, assignment = batch.params["calls"]
        assert guard.function == "check_registered"
        assert assignment.function == "set_children"
        assert extras["hashed_registration_guards"] == [key.ss58_address]
    assert not chain.submissions


@pytest.mark.parametrize("scheme", [4, 6])
@pytest.mark.parametrize("proxy", [False, True])
async def test_child_multisig_approvals_keep_guarded_call(pinned_setup, scheme, proxy):
    chain, codec = pinned_setup
    address = pinned_staking.register(chain, scheme, 92)
    kwargs = {"proxy_for": ALICE} if proxy else {}
    semantic = SetChildren(1, [[0.5, address], [0.25, BOB]], hotkey_ss58=ALICE)
    adapter, original = await pinned_staking.opening(chain, semantic, **kwargs)
    _, later = await _compose_intent_call(chain, adapter, dev_wallet("//Bob"), **kwargs)
    assert later["multisig_call_data"] == original["multisig_call_data"]
    decoded = codec.decode("Call", bytes.fromhex(later["multisig_call_data"][2:]))
    assert decoded["call_module"] == ("Proxy" if proxy else "Utility")
    adapter.semantic = replace(semantic, children=[[0.25, address], [0.25, BOB]])
    with pytest.raises(ValueError, match="do not match the verified recipient"):
        await _compose_intent_call(chain, adapter, dev_wallet("//Bob"), **kwargs)


async def test_plain_children_and_revocation_keep_direct_calls(setup):
    chain, wallet, _, _ = setup
    for children in ([], [[0.5, BOB]]):
        call, extras = await _compose_intent_call(
            chain, SetChildren(1, children, hotkey_ss58=ALICE), wallet
        )
        assert (call.module, call.function) == ("SubtensorModule", "set_children")
        assert extras == {}
