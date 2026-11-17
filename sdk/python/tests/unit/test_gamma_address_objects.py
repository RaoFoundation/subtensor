"""Gamma address objects reach shared policy as public address values."""

from types import SimpleNamespace

import pytest

from bittensor import Client
from bittensor.intents import REGISTRY, MintGamma, MoveGamma, SpendGamma, build
from bittensor.keyfiles import Keypair
from bittensor.result import BittensorError
from bittensor.sp_core import CRYPTO_HASHED
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB_HOT, dev_wallet, seed_gamma


def address_object(kind, uri):
    key = Keypair.create_from_uri(uri)
    if kind == "keypair":
        return key
    if kind == "public_keypair":
        return Keypair(ss58_address=key.ss58_address)
    if kind == "wallet":
        return SimpleNamespace(
            coldkey=key,
            coldkeypub=Keypair(ss58_address=key.ss58_address),
            hotkey=key,
            hotkeypub=Keypair(ss58_address=key.ss58_address),
        )
    return SimpleNamespace(ss58_address=key.ss58_address)


@pytest.mark.parametrize("kind", ["keypair", "public_keypair", "address_view", "wallet"])
@pytest.mark.parametrize("constructor", ["direct", "registry"])
@pytest.mark.parametrize(
    ("op", "fields"),
    [
        ("mint_gamma", {"spender_ss58": "//Alice"}),
        ("mint_gamma", {"service_hotkey_ss58": "//Alice//hot"}),
        ("mint_gamma_with_stake", {"spender_ss58": "//Alice"}),
        ("mint_gamma_with_stake", {"service_hotkey_ss58": "//Alice//hot"}),
        ("set_gamma_deposit_spender", {"spender_ss58": "//Alice"}),
        ("set_gamma_deposit_spender", {"service_hotkey_ss58": "//Alice//hot"}),
        (
            "move_gamma",
            {"origin_hotkey_ss58": "//Alice//hot", "destination_hotkey_ss58": "//Bob//hot"},
        ),
    ],
)
async def test_address_objects_compose_the_same_gamma_call_as_strings(
    kind, constructor, op, fields
):
    args = {"netuid": 1, "hotkey_ss58": BOB_HOT}
    if op == "mint_gamma":
        args["amount_tao"] = "1"
    elif op == "mint_gamma_with_stake":
        args.update(amount_alpha="1", origin_hotkey_ss58=ALICE_HOT)
    elif op == "move_gamma":
        del args["hotkey_ss58"]
        args["amount_gamma"] = "0.5"
    objects = {name: address_object(kind, uri) for name, uri in fields.items()}
    strings = {name: Keypair.create_from_uri(uri).ss58_address for name, uri in fields.items()}
    typed = (
        REGISTRY[op](**args, **objects) if constructor == "direct" else build(op, args | objects)
    )
    plain = build(op, args | strings)
    # Keep scalar inputs until the executor resolves receiving descriptors and guards.
    assert all(getattr(typed, name) is value for name, value in objects.items())
    substrate = FakeSubstrate()
    seed_gamma(substrate, ALICE)
    substrate.seed("SubtensorModule", "Services", [1, ALICE_HOT], {"spender": ALICE})
    client = Client("local", substrate=substrate)
    expected = await client.plan(plain, dev_wallet(), shielded=False)
    actual = await client.plan(typed, dev_wallet(), shielded=False)
    assert expected.ok, expected.violations
    assert actual.ok, actual.violations
    assert tuple(actual.call) == tuple(expected.call)
    assert actual.args == expected.args
    assert not substrate.submissions


@pytest.mark.parametrize("kind", ["keypair", "public_keypair", "address_view", "wallet"])
@pytest.mark.parametrize("constructor", ["direct", "registry"])
async def test_nested_spend_addresses_normalize_without_mutating_caller_input(kind, constructor):
    hotkey = address_object(kind, "//Alice//hot")
    row = {"hotkey_ss58": hotkey, "amount_gamma": "0.500000001", "reference": "ab" * 32}
    spends = [row]
    args = {"netuid": 1, "spends": spends}
    intent = SpendGamma(**args) if constructor == "direct" else build("spend_gamma", args)
    assert spends == [row]
    assert row == {"hotkey_ss58": hotkey, "amount_gamma": "0.500000001", "reference": "ab" * 32}
    assert intent.spends is not spends and intent.spends[0] is not row
    call = await intent.build(FakeSubstrate(), dev_wallet())
    assert tuple(call) == (
        "SubtensorModule",
        "spend_gamma",
        {"netuid": 1, "spends": [(ALICE_HOT, 500_000_001, "0x" + "ab" * 32)]},
    )


def test_address_coercion_preserves_core_argument_validation():
    key = Keypair.create_from_uri("//Alice")
    with pytest.raises(BittensorError, match="name a spender or a service, not both"):
        MintGamma(1, BOB_HOT, "1", spender_ss58=key, service_hotkey_ss58=key)
    with pytest.raises(BittensorError, match="entries require"):
        SpendGamma(1, [{"hotkey_ss58": key, "amount_gamma": "1", "unexpected": True}])
    with pytest.raises(BittensorError, match="origin and destination hotkeys are the same"):
        MoveGamma(1, key, Keypair(ss58_address=key.ss58_address), "1")


def test_mint_retains_protected_descriptor_for_executor_recipient_guards():
    key = Keypair.create_from_seed(bytes([17]) * 32, CRYPTO_HASHED)
    descriptor = bytes(key.hashed_descriptor)
    intent = MintGamma(1, BOB_HOT, "1", spender_ss58=key)
    assert intent.spender_ss58 is key
    assert bytes(intent.spender_ss58.hashed_descriptor) == descriptor
