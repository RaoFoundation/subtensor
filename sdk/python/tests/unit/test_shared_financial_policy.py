"""Exact financial vectors shared by the Python, native JS and real WASM bindings."""

import json
from pathlib import Path

import pytest

from bittensor._policy import evaluate_policy
from bittensor.emission_weights import parse_share
from bittensor.intents._money import gamma_units
from bittensor.limit_orders import order_terms

VECTORS = json.loads(
    (Path(__file__).parents[3] / "bittensor-core/tests/fixtures/policy-financial.json").read_text()
)


@pytest.mark.parametrize("vector", VECTORS, ids=lambda row: row["operation"])
def test_financial_policy_cross_binding_vector(vector):
    assert evaluate_policy(vector["operation"], vector["input"]) == vector["expected"]


def test_financial_python_wrappers_preserve_exact_values_and_economic_object():
    assert gamma_units("18446744073.709551615") == 2**64 - 1
    assert parse_share("50%") == (2**64 - 1) // 2
    economic = {
        "signer": "owner",
        "hotkey": "hotkey",
        "fee_recipient": "fee",
        "netuid": 7,
        "amount": 9007199254740993,
    }
    assert order_terms({"V2": economic}) is economic
    with pytest.raises(ValueError, match="fee headroom"):
        evaluate_policy("financial.intent.free_amount", {"free": 500001, "existential_deposit": 1})
