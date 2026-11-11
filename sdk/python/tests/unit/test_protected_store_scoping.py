"""Chain isolation and durable identities for protected-account local state."""

from __future__ import annotations

import pytest

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann

CHAIN_A = "0x" + "aa" * 32
CHAIN_B = "0x" + "bb" * 32


@pytest.fixture(autouse=True)
def isolated_stores(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))


def _protected(genesis_hash, *, wallet="main", pure="5Pure"):
    record = {
        "controller_wallet": wallet,
        "controller_ss58": "5Controller",
        "protected_ss58": pure,
        "delay_blocks": 10,
    }
    if genesis_hash is not None:
        record["genesis_hash"] = genesis_hash
    return pa.upsert_protected(record)


def _announce(genesis_hash, *, call_hash="0x11", real_ss58="5Pure"):
    return ann.add_announcement(
        controller_wallet="main",
        delegate_ss58="5Controller",
        real_ss58=real_ss58,
        call_hash=call_hash,
        inner_op="transfer",
        inner_args={"dest_ss58": "5Recipient", "amount_tao": 100},
        category="transfer",
        delay_blocks=10,
        announced_at_block=100,
        call_data="0x01020304",
        genesis_hash=genesis_hash,
    )


def _discover(call_hash, *, genesis_hash=CHAIN_A, height=100):
    return ann.upsert_on_chain_announcement(
        controller_wallet="main",
        genesis_hash=genesis_hash,
        delegate_ss58="5Controller",
        real_ss58="5Pure",
        call_hash=call_hash,
        delay_blocks=10,
        announced_at_block=height,
    )


def test_same_wallet_keeps_independent_chain_records_when_rebound():
    original_a = _protected(CHAIN_A)
    original_b = _protected(CHAIN_B, pure="5OtherPure")
    legacy = _protected(None, pure="5LegacyPure")

    moved = pa.move_protected_controller(
        "main", "replacement", controller_ss58="5Replacement", genesis_hash=CHAIN_A
    )

    assert moved == {
        **original_a,
        "controller_wallet": "replacement",
        "controller_ss58": "5Replacement",
    }
    assert pa.get_by_controller("main", genesis_hash=CHAIN_A) is None
    assert pa.get_by_controller("main", genesis_hash=CHAIN_B) == original_b
    assert pa.get_by_controller("main") == legacy
    assert pa.get_by_protected_ss58("5OtherPure", genesis_hash=CHAIN_A) is None
    assert pa.get_by_protected_ss58("5OtherPure", genesis_hash=CHAIN_B) == original_b
    assert not pa.remove_protected("replacement", genesis_hash=CHAIN_B)
    assert pa.get_by_controller("replacement", genesis_hash=CHAIN_A) == moved


def test_other_chain_cannot_mark_remove_or_rebind_saved_withdrawal():
    withdrawal = _announce(CHAIN_A)
    other = _announce(CHAIN_B)
    legacy = _announce(None)

    assert ann.mark_status("main", withdrawal["index"], "gone", genesis_hash=CHAIN_B) is None
    assert not ann.remove_announcement("main", withdrawal["index"], genesis_hash=CHAIN_B)
    assert ann.move_wallet_announcements("main", "new", genesis_hash=CHAIN_B) == 1

    assert ann.get_by_index("main", withdrawal["index"], genesis_hash=CHAIN_A) == withdrawal
    assert ann.list_for_wallet("main", genesis_hash=CHAIN_A) == [withdrawal]
    assert (
        ann.get_by_index("new", other["index"], genesis_hash=CHAIN_B)["call_data"]
        == other["call_data"]
    )
    assert ann.list_for_wallet("main") == [legacy]
    assert ann.get_by_index("main", withdrawal["index"]) is None


# Removing one unknown commitment must not retarget cancellation of another withdrawal.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_discovered_indices_keep_cancellation_target_after_other_calls_disappear():
    first, target, last = [_discover(call_hash) for call_hash in ("0x11", "0x22", "0x33")]
    ann.mark_status("main", first["index"], "gone", genesis_hash=CHAIN_A)

    # The next command sees a shorter/reordered on-chain list and new entries.
    assert _discover("0x33")["index"] == last["index"]
    assert _discover("0x22")["index"] == target["index"]
    new = _discover("0x44")
    assert new["index"] > last["index"]
    selected = ann.get_by_index("main", target["index"], genesis_hash=CHAIN_A)
    assert selected["call_hash"] == "0x22"
    assert selected["delegate_ss58"] == "5Controller"
    assert selected["real_ss58"] == "5Pure"
    assert selected["plaintext_missing"]


@pytest.mark.parametrize("status", ["gone", "executed", "cancelled", "awaiting_approval"])
def test_live_commitment_recovers_historical_plaintext_with_original_index(status):
    saved = _announce(CHAIN_A)
    ann.mark_status("main", saved["index"], status, genesis_hash=CHAIN_A)

    recovered = _discover(saved["call_hash"], height=200)

    assert recovered["index"] == saved["index"]
    assert recovered["inner_op"] == saved["inner_op"]
    assert recovered["inner_args"] == saved["inner_args"]
    assert recovered["call_data"] == saved["call_data"]
    assert not recovered["plaintext_missing"]
    assert recovered["status"] == "pending"
    assert recovered["executable_at_block"] == 210
    assert len(ann.list_for_wallet("main", genesis_hash=CHAIN_A)) == 1


def test_announcing_discovered_hash_attaches_plaintext_without_changing_index():
    discovered = _discover("0x11")
    saved = _announce(CHAIN_A)
    repeated = _announce(CHAIN_A)
    assert discovered["index"] == saved["index"] == repeated["index"]
    assert len(ann.list_for_wallet("main", genesis_hash=CHAIN_A)) == 1
    assert ann.get_by_index("main", saved["index"], genesis_hash=CHAIN_A)["call_data"]


def test_old_duplicate_rows_recover_bytes_without_duplicate_active_indices():
    unknown = _discover("0x11")
    duplicate = {
        **unknown,
        "index": unknown["index"] + 1,
        "inner_op": "transfer",
        "inner_args": {"amount_tao": 100},
        "call_data": "0x01020304",
    }
    ann.save_announcements([unknown, duplicate])

    recovered = _discover("0x11")

    assert recovered["index"] == unknown["index"]
    assert recovered["call_data"] == duplicate["call_data"]
    assert ann.list_for_wallet("main", genesis_hash=CHAIN_A) == [recovered]
    archived = ann.get_by_index("main", duplicate["index"], genesis_hash=CHAIN_A)
    assert archived["status"] == "superseded"
    assert archived["call_data"] == duplicate["call_data"]


def test_legacy_records_require_explicit_binding_and_keep_all_queue_history():
    legacy = _protected(None)
    pending = _announce(None)
    finished = _announce(None, call_hash="0x22")
    ann.mark_status("main", finished["index"], "executed")
    unrelated = _announce(None, call_hash="0x33", real_ss58="5OtherPure")
    assert pa.get_by_controller("main", genesis_hash=CHAIN_A) is None
    assert ann.list_for_wallet("main", genesis_hash=CHAIN_A) == []

    assert ann.bind_legacy_announcements("main", CHAIN_A, real_ss58="5Pure") == 2
    migrated = pa.bind_legacy_controller("main", CHAIN_A)

    assert migrated == {**legacy, "genesis_hash": CHAIN_A}
    assert pa.get_by_controller("main") is None
    assert ann.get_by_index("main", pending["index"], genesis_hash=CHAIN_A)["call_data"]
    assert ann.get_by_index("main", finished["index"], genesis_hash=CHAIN_A)["status"] == "executed"
    assert ann.list_for_wallet("main") == [unrelated]
    assert ann.bind_legacy_announcements("main", CHAIN_A, real_ss58="5Pure") == 0
    assert pa.bind_legacy_controller("main", CHAIN_A) == migrated


def test_legacy_binding_conflicts_preserve_both_stores_unchanged():
    _protected(None)
    _protected(CHAIN_A, pure="5OtherPure")
    _announce(None)
    _announce(CHAIN_A)
    protected_before = pa.protected_path().read_bytes()
    announcements_before = ann.announcements_path().read_bytes()

    with pytest.raises(ValueError, match="already has"):
        pa.bind_legacy_controller("main", CHAIN_A)
    with pytest.raises(ValueError, match="already exists"):
        ann.bind_legacy_announcements("main", CHAIN_A, real_ss58="5Pure")

    assert pa.protected_path().read_bytes() == protected_before
    assert ann.announcements_path().read_bytes() == announcements_before


def test_rotation_refuses_occupied_wallet_without_hiding_either_queue():
    source = _protected(CHAIN_A)
    destination = _protected(CHAIN_A, wallet="replacement", pure="5OtherPure")
    source_row = _announce(CHAIN_A)
    destination_row = _announce(CHAIN_A, call_hash="0x22", real_ss58="5OtherPure")
    destination_row = ann.update_announcement(
        "main",
        destination_row["index"],
        {"controller_wallet": "replacement"},
        genesis_hash=CHAIN_A,
    )
    protected_before = pa.protected_path().read_bytes()
    announcements_before = ann.announcements_path().read_bytes()

    with pytest.raises(ValueError, match="already controls protected account"):
        pa.move_protected_controller(
            "main", "replacement", controller_ss58="5Replacement", genesis_hash=CHAIN_A
        )

    assert pa.protected_path().read_bytes() == protected_before
    assert ann.announcements_path().read_bytes() == announcements_before
    assert pa.get_by_controller("main", genesis_hash=CHAIN_A) == source
    assert pa.get_by_controller("replacement", genesis_hash=CHAIN_A) == destination
    assert ann.list_for_wallet("main", genesis_hash=CHAIN_A) == [source_row]
    assert ann.list_for_wallet("replacement", genesis_hash=CHAIN_A) == [destination_row]


def test_rotation_ignores_destination_binding_on_another_chain():
    source = _protected(CHAIN_A)
    other_chain = _protected(CHAIN_B, wallet="replacement", pure="5OtherPure")

    moved = pa.move_protected_controller(
        "main", "replacement", controller_ss58="5Replacement", genesis_hash=CHAIN_A
    )

    assert moved["protected_ss58"] == source["protected_ss58"]
    assert moved["controller_ss58"] == "5Replacement"
    assert pa.get_by_controller("main", genesis_hash=CHAIN_A) is None
    assert pa.get_by_controller("replacement", genesis_hash=CHAIN_B) == other_chain


def test_rotation_storage_failure_preserves_source_binding(monkeypatch):
    source = _protected(CHAIN_A)
    before = pa.protected_path().read_bytes()

    def fail_replace(*_args):
        raise OSError("cannot replace protected store")

    monkeypatch.setattr(pa.os, "replace", fail_replace)
    with pytest.raises(OSError, match="cannot replace"):
        pa.move_protected_controller(
            "main", "replacement", controller_ss58="5Replacement", genesis_hash=CHAIN_A
        )

    assert pa.protected_path().read_bytes() == before
    assert pa.get_by_controller("main", genesis_hash=CHAIN_A) == source
    assert pa.get_by_controller("replacement", genesis_hash=CHAIN_A) is None


def test_rotation_to_same_pure_preserves_one_destination_binding():
    source = _protected(CHAIN_A)
    _protected(CHAIN_A, wallet="replacement")

    moved = pa.move_protected_controller(
        "main", "replacement", controller_ss58="5Replacement", genesis_hash=CHAIN_A
    )

    assert moved == {
        **source,
        "controller_wallet": "replacement",
        "controller_ss58": "5Replacement",
    }
    assert pa.load_protected() == [moved]
