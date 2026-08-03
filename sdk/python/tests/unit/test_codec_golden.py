"""Codec golden tests: the new RuntimeCodec must reproduce, byte for byte, the
artifacts recorded from the old transport (tests/fixtures/golden.json) — using
only the raw metadata bytes stored in the fixture. No network."""

from __future__ import annotations

import pytest

from bittensor._transport import codec as codec_mod
from codegen.emit_python import emit_storage
from tests.conftest import GOLDEN_FIXTURE, golden
from tests.conftest import golden_codec as codec

pytestmark = pytest.mark.skipif(not GOLDEN_FIXTURE.exists(), reason="golden fixture not recorded")


def _rebuild_call(case: dict):
    """Re-compose the calls the recorder built (incl. the nested ones)."""
    g = golden()
    bob = g["ss58"][1]["address"]
    charlie = g["ss58"][2]["address"]
    alice = g["ss58"][0]["address"]
    c = codec()
    if case["module"] == "Sudo":
        inner = c.compose_call("System", "remark", {"remark": "0xdeadbeef"})
        return c.compose_call("Sudo", "sudo", {"call": inner})
    if case["module"] == "Utility":
        t1 = c.compose_call("Balances", "transfer_keep_alive", {"dest": bob, "value": 1})
        t2 = c.compose_call("Balances", "transfer_keep_alive", {"dest": charlie, "value": 2})
        return c.compose_call("Utility", "batch", {"calls": [t1, t2]})
    if case["module"] == "Proxy":
        t1 = c.compose_call("Balances", "transfer_keep_alive", {"dest": bob, "value": 1})
        return c.compose_call(
            "Proxy", "proxy", {"real": alice, "force_proxy_type": "Transfer", "call": t1}
        )
    return c.compose_call(case["module"], case["function"], case["params"])


def test_composed_calls_are_byte_identical():
    for case in golden()["calls"]:
        call = _rebuild_call(case)
        assert "0x" + codec().call_data(call).hex() == case["data_hex"], (
            f"{case['module']}.{case['function']} encoding diverged"
        )


def test_signature_payloads_match():
    g = golden()
    c = codec()
    transfer = c.compose_call(
        "Balances", "transfer_keep_alive", {"dest": g["ss58"][1]["address"], "value": 12345}
    )
    immortal, mortal = g["signature_payloads"]
    payload = c.signature_payload(
        transfer,
        era="00",
        nonce=immortal["nonce"],
        tip=0,
        tip_asset_id=None,
        genesis_hash=g["network"]["genesis_hash"],
        era_block_hash=g["network"]["genesis_hash"],
    )
    assert "0x" + payload.hex() == immortal["payload_hex"]

    era = {"period": mortal["era"]["period"], "current": mortal["era"]["current"]}
    payload = c.signature_payload(
        transfer,
        era=era,
        nonce=mortal["nonce"],
        tip=0,
        tip_asset_id=None,
        genesis_hash=g["network"]["genesis_hash"],
        era_block_hash=mortal["era_birth_block_hash"],
    )
    assert "0x" + payload.hex() == mortal["payload_hex"]


def test_signature_payload_parts_concatenate_to_payload():
    g = golden()
    c = codec()
    transfer = c.compose_call(
        "Balances", "transfer_keep_alive", {"dest": g["ss58"][1]["address"], "value": 12345}
    )
    immortal = g["signature_payloads"][0]
    call_data, extra, additional = c.signature_payload_parts(
        transfer,
        era="00",
        nonce=immortal["nonce"],
        tip=0,
        tip_asset_id=None,
        genesis_hash=g["network"]["genesis_hash"],
        era_block_hash=g["network"]["genesis_hash"],
    )
    assert "0x" + (call_data + extra + additional).hex() == immortal["payload_hex"]
    assert "0x" + call_data.hex() == immortal["call_data_hex"]
    # The implied section carries spec/tx version + genesis + era block hash
    # (+ the CheckMetadataHash Option byte); it never travels in the extrinsic.
    assert len(additional) >= 72


def test_signed_extrinsic_encoding_matches():
    g = golden()
    c = codec()
    transfer = c.compose_call(
        "Balances", "transfer_keep_alive", {"dest": g["ss58"][1]["address"], "value": 12345}
    )
    for case in g["extrinsics"]:
        era = case["era"] if case["era"] == "00" else dict(case["era"])
        data, ext_hash = c.encode_signed_extrinsic(
            transfer,
            public_key=bytes.fromhex(case["public_key_hex"][2:]),
            signature=bytes.fromhex(case["signature_hex"][2:]),
            signature_version=case["signature_version"],
            era=era,
            nonce=case["nonce"],
            tip=case["tip"],
            tip_asset_id=None,
        )
        assert "0x" + data.hex() == case["extrinsic_hex"]
        assert ext_hash == case["extrinsic_hash"]


def test_extrinsic_decode_roundtrip():
    g = golden()
    c = codec()
    raw_extrinsics = g["block"]["raw"]["block"]["extrinsics"]
    decoded_old = g["block"]["decoded_extrinsics"]
    for raw, old in zip(raw_extrinsics, decoded_old):
        new = c.decode_extrinsic(raw)
        assert new == old


def test_multisig_derivation_matches():
    g = golden()
    for case in g["multisig"]:
        account = codec_mod.multisig_account(
            case["signatories"], case["threshold"], g["network"]["ss58_format"]
        )
        assert account.ss58_address == case["ss58_address"]
        assert account.threshold == case["threshold"]


def test_ss58_roundtrip_matches():
    g = golden()
    for case in g["ss58"]:
        pub = case["public_key_hex"][2:]
        assert codec_mod.ss58_encode(bytes.fromhex(pub), 42) == case["address"]
        assert codec_mod.ss58_decode(case["address"]) == pub


def test_constants_decode():
    g = golden()
    for case in g["constants"]:
        assert codec().constant(case["module"], case["name"]) == case["decoded"]


def test_storage_value_decode_matches():
    g = golden()
    c = codec()
    for case in g["storage_values"]:
        entry = c.storage_entry(case["pallet"], case["storage_function"])
        raw_hex = case["raw_hex"]
        if raw_hex is not None:
            value = c.decode(entry.decode_type(True), bytes.fromhex(raw_hex[2:]))
        else:
            value = c.decode(entry.decode_type(False), entry.default_bytes)
        assert value == case["decoded"], f"{case['pallet']}.{case['storage_function']} diverged"


def test_events_decode_matches():
    g = golden()
    c = codec()
    entry = c.storage_entry("System", "Events")
    raw = bytes.fromhex(g["events"]["raw_hex"][2:])
    assert c.decode(entry.value_type, raw) == g["events"]["decoded"]


def test_runtime_api_map_present():
    api_map = codec().runtime_api_map
    assert "NeuronInfoRuntimeApi" in api_map
    assert "get_neurons_lite" in api_map["NeuronInfoRuntimeApi"]


def test_metadata_ir_shape():
    ir = codec().metadata_ir()
    assert ir.spec_version == golden()["network"]["spec_version"]
    names = {p.name for p in ir.pallets}
    assert {"System", "SubtensorModule", "Balances"} <= names
    subtensor_pallet = next(p for p in ir.pallets if p.name == "SubtensorModule")
    add_stake = next(call for call in subtensor_pallet.calls if call.name == "add_stake")
    assert [a.name for a in add_stake.args] == ["hotkey", "netuid", "amount_staked"]
    # Type identity per arg: named registry types by path (including the
    # NetUid/TaoBalance newtypes the runtime carries), primitives by name.
    assert add_stake.args[0].type_ident == "AccountId32"
    assert add_stake.args[1].type_ident == "NetUid"
    assert add_stake.args[2].type_ident == "TaoBalance"
    assert [e.index for e in subtensor_pallet.errors] == list(range(len(subtensor_pallet.errors)))
    storage_by_name = {s.name: s for s in subtensor_pallet.storage}
    assert "Tempo" in storage_by_name
    # Storage entries carry their VALUE's type identity (map storages: the
    # value after all keys), including the PerU16/TaoBalance newtypes.
    assert storage_by_name["Tempo"].value_type_ident == "u16"
    assert storage_by_name["Delegates"].value_type_ident == "PerU16"
    assert storage_by_name["MinBurn"].value_type_ident == "TaoBalance"
    assert any(api.name == "NeuronInfoRuntimeApi" for api in ir.runtime_apis)
    assert ir.to_dict()["spec_version"] == ir.spec_version  # JSON-serializable


def test_emitted_storage_descriptors_carry_value_idents():
    content = emit_storage(codec().metadata_ir())
    # The Item tuple grows a third, defaulted field, so the committed (old)
    # two-field descriptors keep constructing and consumers can getattr it.
    assert "value_type_ident: str = ''" in content
    assert "Tempo = Item('SubtensorModule', 'Tempo', 'u16')" in content
    compile(content, "storage.py", "exec")
