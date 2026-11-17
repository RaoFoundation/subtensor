"""Shared transaction rules exercised through the Python native adapter."""

import pytest

from bittensor._policy import evaluate_policy
from bittensor._transport.extrinsics import _normalize_signature
from bittensor._transport.utils.receipt import nested_dispatch_error
from bittensor.account_mode import validate_account_mode
from bittensor.hashed import descriptor_value
from bittensor.intents import registry
from bittensor.intents._shared import metadata


def test_shared_exact_spend_and_unbounded_policy():
    result = evaluate_policy(
        "transaction.intent_policy",
        {
            "max_spend_rao": 9007199254740994,
            "spend_rao": 9007199254740993,
            "additional_spend_rao": 2,
            "allowed_netuids": [1],
            "netuids": [2],
        },
    )
    assert result["violations"] == [
        {"code": "spend", "actual": "9007199254740995", "cap": "9007199254740994"},
        {"code": "netuid", "netuid": 2},
    ]
    transfer = registry.build(
        "transfer", {"dest_ss58": "recipient", "amount_tao": "9007199.254740993"}
    )
    assert transfer.spend().rao == 9007199254740993
    assert metadata(transfer)["spend_rao"] == "9007199254740993"


@pytest.mark.asyncio
async def test_core_call_shapes_keep_exact_python_codec_integers():
    class Substrate:
        async def compose(self, call):
            return call

    intent = registry.build(
        "transfer", {"dest_ss58": "recipient", "amount_tao": "9007199.254740993"}
    )
    call = await intent.build(Substrate(), None)
    assert call.params["value"] == 9007199254740993
    assert isinstance(call.params["value"], int)
    assert call.function == "transfer_keep_alive"


def test_shared_signature_policy_retains_python_legacy_prefix_behavior():
    # Python has always permitted the actual scheme prefix to select Ed25519
    # even when the declared default is Sr25519; TS strict mode stays distinct.
    assert _normalize_signature(bytes([0]) + bytes(64), 1) == (bytes(64), 0)
    assert _normalize_signature(bytes(64), 1) == (bytes(64), 1)


def test_shared_descriptor_and_account_modes_fail_closed():
    assert descriptor_value(bytes([1, 3]) + bytes([7]) * 32)["scheme"] == "Ed25519"
    with pytest.raises(ValueError):
        descriptor_value(bytes([1, 3]) + bytes(32))
    assert validate_account_mode("safe") == "safe"
    with pytest.raises(ValueError):
        validate_account_mode("unexpected")


def test_shared_receipt_classification_retains_later_nested_failure():
    failure = {"Module": {"index": 7, "error": "0x01000000"}}
    events = [
        {"invalid": True},
        {
            "event": {
                "module_id": "Proxy",
                "event_id": "ProxyExecuted",
                "attributes": [{"Err": failure}],
            }
        },
    ]
    assert nested_dispatch_error(events) == failure


def test_shared_authority_and_receiving_rules_preserve_finality():
    assert (
        evaluate_policy(
            "transaction.authority_nonce", {"finalized": 4, "pending": 5, "requested": 4}
        )
        == "pending"
    )
    assert evaluate_policy("transaction.receiving_direct", {"op": "swap_coldkey_announced"}) is True
    result = evaluate_policy(
        "transaction.receiving_registration",
        {"op": "associate_hotkey", "field": "hotkey_ss58", "registered": False},
    )
    assert result["may_register"] is False


def test_shared_http_trust_retention_and_child_bounds():
    assert (
        evaluate_policy(
            "transaction.http_admission", {"phase": "scheme", "declared": 1, "expected": 4}
        )
        == "mismatch"
    )
    assert (
        evaluate_policy(
            "transaction.http_admission",
            {"phase": "receiver", "self": "one", "receiver": "two", "required": True},
        )
        == "mismatch"
    )
    assert (
        evaluate_policy(
            "transaction.http_admission",
            {"phase": "retention", "retention": "11.999", "max_age": 10, "allowed_skew": 2},
        )
        == "too_short"
    )
    with pytest.raises(ValueError, match="exceed"):
        evaluate_policy("transaction.children", {"children": [["0.8", "one"], ["0.8", "two"]]})
    assert evaluate_policy("transaction.take", {"op": "set_take", "take": 100, "current": 101}) == {
        "call": "decrease_take",
        "take": "100",
    }
