"""Cancel completion gates for delayed proxy announcements."""

from bittensor.cli.multisig_helpers import (
    announcement_cancel_completed,
    multisig_approval_only,
)


def _event(event_id: str) -> dict:
    return {"event": {"module_id": "Multisig", "event_id": event_id}}


def test_partial_multisig_approval_is_not_completed_cancel():
    events = [_event("NewMultisig")]
    assert multisig_approval_only(events)
    assert not announcement_cancel_completed(events, still_on_chain=True)
    assert not announcement_cancel_completed(events, still_on_chain=False)


def test_multisig_executed_requires_on_chain_clearance():
    events = [_event("MultisigApproval"), _event("MultisigExecuted")]
    assert not multisig_approval_only(events)
    assert not announcement_cancel_completed(events, still_on_chain=True)
    assert announcement_cancel_completed(events, still_on_chain=False)


def test_non_multisig_cancel_uses_on_chain_presence():
    assert announcement_cancel_completed([], still_on_chain=False)
    assert not announcement_cancel_completed([], still_on_chain=True)
