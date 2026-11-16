import pytest

from bittensor import proxy_announcements as announcements


def row(index, status="pending"):
    return {
        "index": index,
        "controller_wallet": "alice",
        "delegate_ss58": "controller",
        "real_ss58": "pure",
        "genesis_hash": "0x" + "01" * 32,
        "call_hash": "0x" + "02" * 32,
        "status": status,
    }


# An absent payment or hash-based cancellation must remain protected from automatic replay.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("status", ["gone", "cancelled"])
def test_retired_payment_requires_repeat_approval_in_python(tmp_path, monkeypatch, status):
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    original, replacement = row(1), row(2)
    announcements.save_announcements([original, replacement])
    announcements.mark_status("alice", 1, status, genesis_hash=original["genesis_hash"])
    assert announcements.load_announcements()[0]["execution_uncertain"] is True
    assert announcements.executed_commitment_index(replacement) == 1
    # Old journals did not persist the marker. They must get the same protection.
    announcements.save_announcements([row(1, status), replacement])
    assert announcements.executed_commitment_index(replacement) == 1
    announcements.save_announcements([{**row(1, status), "execution_uncertain": None}, replacement])
    assert announcements.executed_commitment_index(replacement) == 1


def test_definite_announcement_failure_remains_retryable(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    original, replacement = row(1, "failed"), row(2)
    announcements.save_announcements([original, replacement])
    assert announcements.executed_commitment_index(replacement) is None
    original["execution_uncertain"] = True
    announcements.save_announcements([original, replacement])
    assert announcements.executed_commitment_index(replacement) == 1


# Legacy failed-action receipts cannot rule out an earlier permissionless payment.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("uncertainty", [{}, {"execution_uncertain": None}])
def test_legacy_failed_action_preserves_replay_guard(tmp_path, monkeypatch, uncertainty):
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    original = {
        **row(1, "failed"),
        **uncertainty,
        "action_receipt": {"status": "finalized", "success": False},
    }
    replacement = row(2)
    announcements.save_announcements([original, replacement])
    assert announcements.executed_commitment_index(replacement) == 1
