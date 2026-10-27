"""Natural ordering for wallet and hotkey names (``coldkey2`` before ``coldkey10``)."""

from __future__ import annotations

import json
from pathlib import Path

from bittensor.wallets import list_wallets, list_wallets_detailed, natural_name_key


def test_natural_name_key_orders_numbered_suffixes():
    names = ["coldkey10", "coldkey2", "coldkey1", "coldkey11", "coldkey"]
    assert sorted(names, key=natural_name_key) == [
        "coldkey",
        "coldkey1",
        "coldkey2",
        "coldkey10",
        "coldkey11",
    ]


def test_natural_name_key_is_case_insensitive():
    names = ["Coldkey10", "coldkey2", "COLDKEY1"]
    assert sorted(names, key=natural_name_key) == [
        "COLDKEY1",
        "coldkey2",
        "Coldkey10",
    ]


def test_natural_name_key_compares_digit_and_text_chunks():
    # Must not raise TypeError when a digit run and a text run meet.
    names = ["2wallet", "awallet", "wallet"]
    assert sorted(names, key=natural_name_key) == ["2wallet", "awallet", "wallet"]


def _write_pub(path: Path, ss58: str) -> None:
    path.write_text(json.dumps({"ss58Address": ss58, "cryptoType": 1}))


def test_list_wallets_uses_natural_order(tmp_path: Path):
    for name in ("coldkey10", "coldkey2", "coldkey1"):
        hotkeys = tmp_path / name / "hotkeys"
        hotkeys.mkdir(parents=True)
        for hk in ("hotkey10", "hotkey2", "hotkey1"):
            (hotkeys / hk).write_text("{}")

    listed = list_wallets(str(tmp_path))
    assert list(listed) == ["coldkey1", "coldkey2", "coldkey10"]
    assert listed["coldkey1"] == ["hotkey1", "hotkey2", "hotkey10"]


def test_list_wallets_detailed_uses_natural_order(tmp_path: Path):
    for i, name in enumerate(("coldkey10", "coldkey2", "coldkey1"), start=1):
        wallet = tmp_path / name
        (wallet / "hotkeys").mkdir(parents=True)
        _write_pub(wallet / "coldkeypub.txt", f"ck{i}")
        for j, hk in enumerate(("hotkey10", "hotkey2", "hotkey1"), start=1):
            _write_pub(wallet / "hotkeys" / hk, f"{name}-hk{j}")

    detailed = list_wallets_detailed(str(tmp_path))
    assert [ck.name for ck in detailed] == ["coldkey1", "coldkey2", "coldkey10"]
    assert [hk.name for hk in detailed[0].hotkeys] == [
        "hotkey1",
        "hotkey2",
        "hotkey10",
    ]
