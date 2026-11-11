"""Proxy grant signer selection preserves defaults and supports explicit pure origins."""

import pytest

from bittensor.intents import AddProxy, Batch, RemoveProxies
from bittensor.intents.proxy import compose_announceable_call
from bittensor.intents.registry import build
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, dev_wallet


@pytest.mark.parametrize("proxy_type", ["Validate", "Weights", "Any", "Staking"])
@pytest.mark.parametrize("from_hotkey", [None, False, True])
def test_proxy_grant_origin_default_override_and_serialized_roundtrip(proxy_type, from_hotkey):
    intent = AddProxy(delegate_ss58=ALICE, proxy_type=proxy_type, from_hotkey=from_hotkey)
    expected = (
        "hotkey"
        if from_hotkey is True or (from_hotkey is None and proxy_type in {"Validate", "Weights"})
        else "coldkey"
    )
    assert intent.signer == expected
    encoded = intent.to_dict()
    assert encoded["from_hotkey"] is from_hotkey
    reconstructed = build(encoded["op"], encoded)
    assert reconstructed.signer == expected
    assert reconstructed.to_dict() == encoded


async def test_pure_account_rotation_batch_preserves_validate_and_weights_grants():
    batch = Batch(
        intents=[
            RemoveProxies(),
            AddProxy(delegate_ss58=ALICE, proxy_type="Any", delay=10),
            AddProxy(delegate_ss58=ALICE_HOT, proxy_type="Validate", from_hotkey=False),
            AddProxy(delegate_ss58=ALICE_HOT, proxy_type="Weights", from_hotkey=False),
        ]
    )
    restored = build("batch", batch.to_dict())
    assert restored.signer == "coldkey"
    call, _commitment, _extras = await compose_announceable_call(
        FakeSubstrate(), restored, BOB, dev_wallet()
    )

    assert (call.module, call.function) == ("Utility", "batch_all")
    children = call.params["calls"]
    assert [(child.module, child.function) for child in children] == [
        ("Proxy", "remove_proxies"),
        ("Proxy", "add_proxy"),
        ("Proxy", "add_proxy"),
        ("Proxy", "add_proxy"),
    ]
    assert [child.params for child in children[1:]] == [
        {"delegate": ALICE, "proxy_type": "Any", "delay": 10},
        {"delegate": ALICE_HOT, "proxy_type": "Validate", "delay": 0},
        {"delegate": ALICE_HOT, "proxy_type": "Weights", "delay": 0},
    ]
