"""Unit tests for protected-account mapping, local stores, and misuse guards."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann


@pytest.fixture()
def protected_paths(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    protected = tmp_path / "btcli_protected.json"
    announcements = tmp_path / "btcli_proxy_announcements.json"
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(protected))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(announcements))
    return protected, announcements


def test_parse_delay_rejects_zero_and_accepts_duration():
    with pytest.raises(ValueError, match=r"more than 0|greater than 0"):
        pa.parse_delay_to_blocks(0)
    with pytest.raises(ValueError, match=r"more than 0|greater than 0"):
        pa.parse_delay_to_blocks("0h")
    # 36h at 12s/block = 10800 blocks
    assert pa.parse_delay_to_blocks("36h") == 10800
    assert pa.parse_delay_to_blocks("10800") == 10800
    assert pa.parse_delay_to_blocks(10800) == 10800
    assert pa.parse_delay_to_blocks("2d") == 14400


def test_allow_items_map_and_refuse_dangerous():
    assert pa.resolve_allow_items(["unstake", "childkeys", "basket-swaps"]) == [
        "Staking",
        "ChildKeys",
        "BasketTrading",
    ]
    with pytest.raises(ValueError, match="refused"):
        pa.resolve_allow_item("transfer")
    with pytest.raises(ValueError, match="refused"):
        pa.resolve_allow_item("any")
    with pytest.raises(ValueError, match="refused"):
        pa.resolve_allow_item("nontransfer")
    with pytest.raises(ValueError, match="no suitably narrow"):
        pa.resolve_allow_item("crowdloan")


def test_instant_proxy_type_for_ops():
    record = {"allows": ["Staking", "ChildKeys"]}
    assert pa.instant_proxy_type_for("remove_stake", record) == "Staking"
    assert pa.instant_proxy_type_for("set_children", record) == "ChildKeys"
    assert pa.instant_proxy_type_for("transfer", record) is None
    assert pa.instant_proxy_type_for("add_proxy", record) is None
    # Pending allows are not active for routing.
    pending_only = {"allows": [], "pending_allows": ["Staking"]}
    assert pa.instant_proxy_type_for("remove_stake", pending_only) is None
    # Instant path only when the dispatcher holds the allow-key grant.
    other_key = {
        "allows": ["Staking"],
        "allow_key_ss58": "5AllowKey",
        "controller_ss58": "5Controller",
    }
    assert (
        pa.instant_proxy_type_for("remove_stake", other_key, dispatcher_ss58="5Controller") is None
    )
    assert (
        pa.instant_proxy_type_for("remove_stake", other_key, dispatcher_ss58="5AllowKey")
        == "Staking"
    )


def test_defeating_zero_delay_detection():
    controller = "5Controller"
    delegations = [
        {"delegate": controller, "proxy_type": "Any", "delay": 10800},
        {"delegate": controller, "proxy_type": "Any", "delay": 0},
        {"delegate": controller, "proxy_type": "Staking", "delay": 0},
        {"delegate": "5Other", "proxy_type": "Transfer", "delay": 0},
    ]
    assert pa.defeating_zero_delay_proxies(delegations, controller) == ["Any"]


def test_allow_key_change_blockers():
    old = "5AllowA"
    new = "5AllowB"
    assert (
        pa.allow_key_change_blockers(
            old_allow_key_ss58=old,
            new_allow_key_ss58=old,
            allows=["Lock"],
            pending_allows=[],
            delegations=[],
        )
        == []
    )
    blockers = pa.allow_key_change_blockers(
        old_allow_key_ss58=old,
        new_allow_key_ss58=new,
        allows=["Lock"],
        pending_allows=["Staking"],
        delegations=[
            {"delegate": old, "proxy_type": "Lock", "delay": 0},
            {"delegate": old, "proxy_type": "Any", "delay": 10800},
            {"delegate": new, "proxy_type": "ChildKeys", "delay": 0},
        ],
    )
    assert any("confirmed allows" in b for b in blockers)
    assert any("pending allows" in b for b in blockers)
    assert any("on-chain zero-delay grants" in b and "Lock@0" in b for b in blockers)
    # Delayed Any on the old key is the controller grant, not an allow-key leftover.
    assert not any("Any@" in b for b in blockers)
    assert (
        pa.allow_key_change_blockers(
            old_allow_key_ss58=old,
            new_allow_key_ss58=new,
            allows=[],
            pending_allows=[],
            delegations=[{"delegate": old, "proxy_type": "Any", "delay": 10800}],
        )
        == []
    )
    # Queuing disallow must not hide an outstanding allow announcement.
    labels = pa.pending_allow_grant_labels(
        [
            {
                "index": 3,
                "category": "allow:Lock",
                "status": "pending",
                "inner_args": {"delegate_ss58": old, "proxy_type": "Lock", "delay": 0},
            },
            {
                "index": 4,
                "category": "disallow:Lock",
                "status": "pending",
                "inner_args": {"delegate_ss58": old, "proxy_type": "Lock", "delay": 0},
            },
            {
                "index": 5,
                "category": "allow:Staking",
                "status": "pending",
                "inner_args": {"delegate_ss58": new, "proxy_type": "Staking", "delay": 0},
            },
        ],
        allow_key_ss58=old,
    )
    assert labels == ["#3 allow:Lock → 5AllowA"]
    blocked = pa.allow_key_change_blockers(
        old_allow_key_ss58=old,
        new_allow_key_ss58=new,
        allows=[],
        pending_allows=[],
        delegations=[],
        pending_allow_announcements=labels,
    )
    assert any("outstanding allow announcements" in b for b in blocked)


def test_controller_owned_allows_and_rotation_complete():
    controller = "5Controller"
    new = "5New"
    record = {"allows": ["Lock", "Staking"], "allow_key_ss58": None}
    assert pa.controller_holds_allows(record, controller)
    assert pa.controller_owned_allow_types(record, controller) == ["Lock", "Staking"]

    other_key = {**record, "allow_key_ss58": "5AllowKey"}
    assert not pa.controller_holds_allows(other_key, controller)
    assert pa.controller_owned_allow_types(other_key, controller) == []

    same_key = {**record, "allow_key_ss58": controller}
    assert pa.controller_owned_allow_types(same_key, controller) == ["Lock", "Staking"]

    mid_rotate = [
        {"delegate": controller, "proxy_type": "Any", "delay": 10800},
        {"delegate": controller, "proxy_type": "Lock", "delay": 0},
        {"delegate": new, "proxy_type": "Any", "delay": 10800},
    ]
    assert not pa.rotation_complete(
        mid_rotate,
        new_controller_ss58=new,
        old_controller_ss58=controller,
        delay_blocks=10800,
    )
    # Old Any removed but Lock left behind — still incomplete.
    leftover_lock = [
        {"delegate": new, "proxy_type": "Any", "delay": 10800},
        {"delegate": controller, "proxy_type": "Lock", "delay": 0},
    ]
    assert not pa.rotation_complete(
        leftover_lock,
        new_controller_ss58=new,
        old_controller_ss58=controller,
        delay_blocks=10800,
    )
    done = [
        {"delegate": new, "proxy_type": "Any", "delay": 10800},
        {"delegate": new, "proxy_type": "Lock", "delay": 0},
    ]
    assert pa.rotation_complete(
        done,
        new_controller_ss58=new,
        old_controller_ss58=controller,
        delay_blocks=10800,
    )


def test_protected_config_roundtrip(protected_paths):
    entry = {
        "controller_wallet": "main",
        "controller_ss58": "5Main",
        "protected_ss58": "5Pure",
        "name": "main-protected",
        "delay_blocks": 10800,
        "allows": ["Staking"],
    }
    pa.upsert_protected(entry)
    assert pa.get_by_controller("main")["protected_ss58"] == "5Pure"
    moved = pa.move_protected_controller("main", "new", controller_ss58="5New")
    assert moved["controller_wallet"] == "new"
    assert moved["controller_ss58"] == "5New"
    assert pa.get_by_controller("main") is None
    assert pa.get_by_controller("new")["protected_ss58"] == "5Pure"
    assert pa.get_by_controller("new")["controller_ss58"] == "5New"


def test_legacy_pure_derivation_matches_hashed_accounts_vector():
    # Mirrors pallets/hashed-accounts/src/mldsa_tests.rs::pure_proxy_derivation_...
    who = bytes([12] * 32)
    account = pa.derive_legacy_pure_account_id(
        who, height=17, ext_index=2, proxy_type_scale=3, index=9
    )
    assert account.hex() == ("b5cc23966887d8f2b052ab1cdd660d081b97d7cb8768d8e1f02b73cdfa5724dd")


def test_announcement_store_indexes_and_status(protected_paths):
    row = ann.add_announcement(
        controller_wallet="main",
        delegate_ss58="5Main",
        real_ss58="5Pure",
        call_hash="0x" + "ab" * 32,
        inner_op="transfer",
        inner_args={"dest_ss58": "5Dest", "amount_tao": 1},
        category="transfer",
        delay_blocks=10,
        announced_at_block=100,
        summary="transfer 1",
    )
    assert row["index"] == 1
    assert row["executable_at_block"] == 110
    assert ann.get_by_index("main", 1)["call_hash"] == row["call_hash"]
    assert len(ann.list_for_wallet("main")) == 1
    ann.mark_status("main", 1, "awaiting_approval")
    assert len(ann.list_for_wallet("main")) == 1
    assert ann.list_for_wallet("main")[0]["status"] == "awaiting_approval"
    ann.mark_status("main", 1, "cancelled")
    assert ann.list_for_wallet("main") == []
    assert ann.get_by_index("main", 1)["status"] == "cancelled"


def test_announcement_store_atomic_write(protected_paths):
    path = Path(protected_paths[1])
    ann.add_announcement(
        controller_wallet="main",
        delegate_ss58="5Main",
        real_ss58="5Pure",
        call_hash="0x" + "cd" * 32,
        inner_op="remove_stake",
        inner_args={},
        category="unstake",
        delay_blocks=1,
        announced_at_block=1,
    )
    data = json.loads(path.read_text())
    assert data["version"] == 1
    assert len(data["announcements"]) == 1
