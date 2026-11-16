"""Deferred crowdloan calls retain protected-account authority restrictions."""

from types import SimpleNamespace
from unittest.mock import Mock

import pytest
import typer

from bittensor._policy import evaluate_policy
from bittensor.cli.protected import _protected_management_path, _refuse_protected_management


# Deferred crowdloan finalization must not remove a pure account's controllers.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize(
    "operation", ["remove_proxies", "add_proxy", "remove_proxy", "kill_pure_proxy"]
)
def test_cli_refuses_management_stored_inside_a_crowdloan(operation):
    intent = {
        "op": "create_crowdloan",
        "call": {
            "op": "multisig_execute",
            "account_mode": "safe",
            "call": {"op": "batch", "intents": [{"op": operation}]},
        },
    }
    context = SimpleNamespace(output=SimpleNamespace(error=Mock()))
    with pytest.raises(typer.Exit) as caught:
        _refuse_protected_management(context, intent, "pure-account")
    assert caught.value.exit_code == 2
    context.output.error.assert_called_once()
    assert operation in context.output.error.call_args.args[0]
    assert _protected_management_path(intent) == (
        f"create_crowdloan.multisig_execute.batch[0].{operation}"
    )


def test_optional_crowdloan_target_has_no_hidden_authority():
    intent = {"op": "create_crowdloan", "target_ss58": "recipient", "call": None}
    context = SimpleNamespace(output=SimpleNamespace(error=Mock()))
    _refuse_protected_management(context, intent, "pure-account")
    context.output.error.assert_not_called()
    assert _protected_management_path(intent) is None


def test_decoded_crowdloan_call_and_grants_are_visible_to_shared_policy():
    leaf = {"pallet": "Proxy", "call": "add_proxy", "params": {"proxy_type": "Any", "delay": 0}}
    call = {"pallet": "Crowdloan", "call": "create", "params": {"call": leaf}}
    assert evaluate_policy("protected.call_leaves", {"call": call}) == [leaf]
    assert evaluate_policy(
        "protected.announcement_grants",
        {
            "inner_op": "create_crowdloan",
            "inner_args": {
                "call": {
                    "op": "add_proxy",
                    "delegate_ss58": "delegate",
                    "proxy_type": "Any",
                    "delay": 0,
                }
            },
        },
    ) == [{"delegate_ss58": "delegate", "proxy_type": "Any", "delay": 0}]
