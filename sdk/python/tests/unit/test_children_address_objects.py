"""Child recipients retain their public identity through proportion policy."""

from types import SimpleNamespace

import pytest

from bittensor.executor import _compose_intent_call
from bittensor.hashed import registration_descriptor_value
from bittensor.intents import SetChildren, build
from bittensor.keyfiles import Keypair
from bittensor.sp_core import CRYPTO_HASHED
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, dev_wallet
from tests.unit.test_hashed_receiving import registered


def recipient(kind):
    key = Keypair.create_from_uri("//Bob")
    public = Keypair(ss58_address=key.ss58_address)
    if kind == "keypair":
        return key
    if kind == "public_keypair":
        return public
    if kind == "address_view":
        return SimpleNamespace(ss58_address=key.ss58_address)
    return SimpleNamespace(coldkey=key, coldkeypub=public, hotkey=key, hotkeypub=public)


@pytest.mark.parametrize("kind", ["keypair", "public_keypair", "address_view", "wallet"])
@pytest.mark.parametrize("constructor", ["direct", "registry"])
async def test_child_address_objects_compose_identically_to_strings(kind, constructor):
    child = recipient(kind)
    rows = [["0.5", child]]
    intent = (
        SetChildren(1, rows, hotkey_ss58=ALICE)
        if constructor == "direct"
        else build("set_children", {"netuid": 1, "children": rows, "hotkey_ss58": ALICE})
    )
    assert rows == [["0.5", child]]
    assert intent.children is not rows
    assert intent.children[0][1] is child
    chain = FakeSubstrate()
    actual, extras = await _compose_intent_call(chain, intent, dev_wallet())
    expected, _ = await _compose_intent_call(
        chain, SetChildren(1, [["0.5", BOB]], hotkey_ss58=ALICE), dev_wallet()
    )
    assert tuple(actual) == tuple(expected)
    assert actual.params["children"] == [(2**63, BOB)]
    assert extras == {}
    assert not chain.submissions


# Child address objects must retain registration guards before delegation.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("state", ["registered", "unregistered", "mismatched"])
async def test_hashed_child_object_retains_descriptor_and_registration_guard(state):
    private = Keypair.create_from_seed(bytes([91]) * 32, CRYPTO_HASHED)
    child = private.public_only()
    chain = FakeSubstrate()
    chain.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    if state != "unregistered":
        registered(chain, private)
    if state == "mismatched":
        other = Keypair.create_from_seed(bytes([92]) * 32, CRYPTO_HASHED)
        from bittensor.hashed import descriptor_value

        chain.seed(
            "HashedAccounts",
            "Accounts",
            [child.ss58_address],
            {"descriptor": descriptor_value(bytes(other.hashed_descriptor))},
        )
    intent = SetChildren(1, [["0.5", child]], hotkey_ss58=ALICE)
    assert intent.children[0][1] is child
    if state != "registered":
        with pytest.raises(ValueError, match=r"not registered|does not match"):
            await _compose_intent_call(chain, intent, dev_wallet())
    else:
        call, extras = await _compose_intent_call(chain, intent, dev_wallet())
        guard, assignment = call.params["calls"]
        assert guard.function == "check_registered"
        assert guard.params["descriptor"] == registration_descriptor_value(
            bytes(child.hashed_descriptor)
        )
        assert assignment.params["children"] == [(2**63, child.ss58_address)]
        assert extras["hashed_registration_guards"] == [child.ss58_address]
    assert not chain.submissions


@pytest.mark.parametrize("shares", [["0.6", "0.6"], [2**64], [-1], ["NaN"]])
def test_public_address_objects_do_not_bypass_proportion_validation(shares):
    child = recipient("public_keypair")
    with pytest.raises(ValueError):
        SetChildren(1, [[share, child] for share in shares])


@pytest.mark.parametrize("children", [None, 1, {}, [1], [["0.5"]], [["0.5", BOB, BOB]]])
def test_invalid_child_rows_still_fail_shared_validation(children):
    with pytest.raises(ValueError):
        SetChildren(1, children)
