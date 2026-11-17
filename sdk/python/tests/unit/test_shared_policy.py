"""The same immutable policy decisions run through every SDK binding."""

from __future__ import annotations

import json
from pathlib import Path

import bittensor_core
import pytest

from bittensor._policy import evaluate_policy, evaluate_policy_preserving_values

CORPUS = json.loads(
    (Path(__file__).parents[3] / "bittensor-core/tests/fixtures/sdk-policy.json").read_text()
)


@pytest.mark.parametrize("vector", CORPUS, ids=lambda vector: vector["name"])
def test_python_binding_executes_shared_policy_corpus(vector):
    key = "error" if "error" in vector else "value"
    result = json.loads(
        bittensor_core.policy_evaluate(vector["operation"], json.dumps(vector["input"]))
    )
    assert result == {"version": 1, key: vector[key]}


def test_python_policy_adapter_preserves_wide_integers_and_binary():
    assert (
        evaluate_policy(
            "validation.amount.units", {"left": 2**100, "right": str(2**100), "mode": "same"}
        )
        is True
    )
    data = bytes.fromhex("04010100000000000000")
    for value in (data, bytearray(data), memoryview(data)):
        assert evaluate_policy("validation.timelock.envelope", {"data": value}) == {
            "inner": "0x01",
            "round": "1",
        }
    with pytest.raises(ValueError):
        evaluate_policy("validation.weights.clip", {"weights": [float("inf")], "limit": 1})


def test_structural_policy_preserves_enum_keys_resembling_opaque_value_tokens():
    pallet = "__bittensor_policy_value__0"
    result = evaluate_policy_preserving_values(
        "protected.decoded_call", {"call": {pallet: {"transfer": {"value": 2**100}}}}
    )
    assert result["pallet"] == pallet
    assert result["params"]["value"] == 2**100
