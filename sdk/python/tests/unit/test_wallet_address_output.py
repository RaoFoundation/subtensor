"""Wallet displays distinguish SS58 account IDs from receiving descriptors."""

from __future__ import annotations

import io
import json

import pytest
from rich.console import Console

from bittensor.cli.output import STYLE_NAME, Output

SS58 = "5EJPD8sby8MAVJnUS7vaaNUmToaZzeu8Kedh4yJYsRiTHWRU"
RECEIVING = (
    "tao_AgUY--fgDPaSpryoxcBQXmu0Ol_o8y2RbJPpc_xRt2wGEu0YHKcSLxwXbKawEypH4BMM"
    "ixILS-JBpDQ4Jg5gxr-vTlKcg5igOBs"
)


def _record(status="not_checked"):
    return {
        "crypto_type": "ms",
        "signing_scheme": "mldsa65",
        "account_type": "standard",
        "ss58": SS58,
        "address": RECEIVING,
        "receiving_address": RECEIVING,
        "primary_address": RECEIVING,
        "registration": status,
        "registration_network": "ws://127.0.0.1:20644",
        "registration_block_hash": "0x" + "ab" * 32 if status == "registered" else None,
    }


def _output(width=80, json_mode=False):
    output = Output(json_mode=json_mode)
    stream = io.StringIO()
    output._out = Console(file=stream, width=width, color_system=None, highlight=False)
    return output, stream


@pytest.mark.parametrize("prefix", ["", "coldkey_", "hotkey_"])
@pytest.mark.parametrize(
    "status", ["registered", "not_checked", "unregistered", "unavailable", "mismatch"]
)
@pytest.mark.parametrize("width", [60, 80])
def test_wallet_detail_addresses_are_labeled_complete_and_ordered(prefix, status, width):
    output, stream = _output(width)
    fields = {"name": "ms-test", **{prefix + key: value for key, value in _record(status).items()}}
    output.wallet_detail("Wallet", fields)
    rendered = stream.getvalue()
    assert "Account ID (SS58)" in rendered and "Receiving address" in rendered
    assert rendered.count(SS58) == 1 and rendered.count(RECEIVING) == 1
    assert rendered.index(RECEIVING) < rendered.index(SS58)
    if status != "mismatch":
        assert "Use the receiving address for transactions" in rendered
        assert "explorers display the same account as SS58" in rendered
    assert rendered.count("Address info") == 1
    assert "Address use" not in rendered and "Safety" not in rendered
    assert "mldsa65, standard" in rendered
    assert "crypto_type" not in rendered and "signing_scheme" not in rendered
    assert "ws://127.0.0.1:20644" in rendered
    assert "primary_address" not in rendered and "registration_block_hash" not in rendered
    assert "name" in rendered and "ms-test" in rendered
    if status == "registered":
        assert "finalized" in rendered
        assert "confirm registration" not in rendered
    elif status == "mismatch":
        assert "does not match this wallet" in rendered
        assert "do not use or publicly share either address" in rendered
        assert "Use the receiving address" not in rendered
    elif status == "unregistered":
        assert "finalize registration before publicly sharing either address" in rendered
    else:
        assert "confirm finalized registration before publicly sharing either address" in rendered


@pytest.mark.parametrize("width", [60, 80])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("status", ["registered", "not_checked", "unregistered", "unavailable"])
def test_wallet_tree_addresses_remain_copyable_at_narrow_widths(width, role, status):
    output, stream = _output(width)
    record = {"coldkey": "ms-test", "ss58": "ordinary-coldkey", "hotkeys": []}
    if role == "coldkey":
        record.update(_record(status))
    else:
        record["hotkeys"] = [{"name": "ms-test-hot", **_record(status)}]
    output.wallet_list("/tmp/wallets", [record])
    rendered = stream.getvalue()
    assert "Account ID (SS58):" in rendered and "Receiving address:" in rendered
    assert rendered.count(SS58) == 1 and rendered.count(RECEIVING) == 1
    assert rendered.index(RECEIVING) < rendered.index(SS58)
    assert "Use the receiving address for transactions" in rendered
    assert "explorers display the same account as SS58" in rendered
    assert rendered.count("Address info:") == 1
    assert "Address use:" not in rendered and "Safety:" not in rendered
    assert "ws://127.0.0.1:20644" in rendered


def test_wallet_detail_and_list_json_keep_full_machine_schema():
    record = _record("registered")
    output, stream = _output(json_mode=True)
    output.wallet_detail("Wallet", record)
    assert json.loads(stream.getvalue()) == record

    output, stream = _output(json_mode=True)
    output.wallet_detail("Wallet", {"name": "human"}, json_fields=record)
    assert json.loads(stream.getvalue()) == record

    output, stream = _output(json_mode=True)
    records = [{"coldkey": "ms-test", **record, "hotkeys": [{"name": "ms-test-hot", **record}]}]
    output.wallet_list("/tmp/wallets", records)
    assert json.loads(stream.getvalue())["coldkeys"] == records


def test_ordinary_wallet_detail_preserves_existing_rendering():
    fields = {"name": "alice", "coldkey_ss58": SS58, "crypto_type": "sr25519"}
    original, original_stream = _output()
    original.detail("Wallet", fields)
    current, current_stream = _output()
    current.wallet_detail("Wallet", fields)
    assert current_stream.getvalue() == original_stream.getvalue()


def test_ordinary_wallet_list_keeps_single_unlabeled_ss58():
    output, stream = _output()
    output.wallet_list("/tmp/wallets", [{"coldkey": "alice", "ss58": SS58, "hotkeys": []}])
    rendered = stream.getvalue()
    assert rendered.count(SS58) == 1
    assert "Receiving address" not in rendered and "Registration" not in rendered


def test_wallet_tree_links_only_ss58(monkeypatch):
    output, _ = _output()
    seen = []
    monkeypatch.setattr(output, "account_url", lambda address, kind: seen.append((address, kind)))
    output._wallet_node(
        "ms-test", SS58, name_style=STYLE_NAME, kind="coldkey", address_record=_record("registered")
    )
    assert seen == [(SS58, "coldkey")]
