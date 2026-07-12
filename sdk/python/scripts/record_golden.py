"""Record golden vectors from the transport against a live localnet.

Produces ``tests/fixtures/golden.json`` (run from the repo root): a corpus of byte-exact artifacts
(storage keys, composed calls, signing payloads, extrinsic encodings, multisig
derivations, raw+decoded storage/runtime-call/event data, and the raw metadata
blobs they were produced against). The offline golden tests
(test_codec_golden.py, test_storage_golden.py) replay these against the codec
using only the recorded metadata — no node needed.

The original corpus was recorded from the pre-rewrite transport
(async-substrate-interface fork) and the rewrite was proven byte-identical
against it. Re-recording with this script therefore captures the same wire
formats; only re-record deliberately (e.g. after a runtime upgrade changes
metadata) and eyeball the diff.

Usage:
    python scripts/record_golden.py [ws-endpoint]    # default ws://127.0.0.1:9944

The script submits a real //Alice transfer (to record receipt/event vectors),
so it refuses non-local endpoints unless RECORD_GOLDEN_ALLOW_REMOTE=1 is set.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

from bittensor._transport.codec import strip_option_opaque_metadata
from bittensor._transport.interface import SubstrateConnection
from bittensor._transport.runtime_api import encode_runtime_api_params
from bittensor.keyfiles import Keypair

ENDPOINT = sys.argv[1] if len(sys.argv) > 1 else "ws://127.0.0.1:9944"
OUT = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "golden.json"

_LOCAL_HOSTS = {"127.0.0.1", "localhost", "::1"}
if (
    urlparse(ENDPOINT).hostname not in _LOCAL_HOSTS
    and os.environ.get("RECORD_GOLDEN_ALLOW_REMOTE") != "1"
):
    sys.exit(
        f"refusing to record against non-local endpoint {ENDPOINT}: this script submits "
        "a real //Alice transfer. Set RECORD_GOLDEN_ALLOW_REMOTE=1 if that chain is a "
        "dev chain and you mean it."
    )

SS58_FORMAT = 42

ALICE = Keypair.create_from_uri("//Alice")
BOB = Keypair.create_from_uri("//Bob")
CHARLIE = Keypair.create_from_uri("//Charlie")
ALICE_HOT = Keypair.create_from_uri("//Alice//hot")

# A fixed, obviously-fake sr25519 signature: makes extrinsic encodings
# deterministic (the chain never sees these bytes).
FAKE_SIG = bytes(range(1, 65))


def jsonable(value: Any) -> Any:
    """Scrub a decoded SCALE value into JSON-native types, bytes as 0x-hex."""
    if isinstance(value, (bytes, bytearray)):
        return "0x" + bytes(value).hex()
    if isinstance(value, (list, tuple)):
        return [jsonable(v) for v in value]
    if isinstance(value, dict):
        return {str(k): jsonable(v) for k, v in value.items()}
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return repr(value)


STORAGE_KEY_CASES = [
    ("System", "Account", [ALICE.ss58_address]),
    ("System", "Events", []),
    ("Timestamp", "Now", []),
    ("SubtensorModule", "Tempo", [1]),
    ("SubtensorModule", "NetworksAdded", [3]),
    ("SubtensorModule", "Bonds", [1, 0]),
    ("SubtensorModule", "Keys", [1, 0]),
    ("SubtensorModule", "Alpha", [ALICE_HOT.ss58_address, ALICE.ss58_address, 1]),
    ("SubtensorModule", "BlocksSinceLastStep", [7]),
    ("Multisig", "Multisigs", [ALICE.ss58_address, "0x" + "ab" * 32]),
]

CALL_CASES = [
    ("Balances", "transfer_keep_alive", {"dest": BOB.ss58_address, "value": 12345}),
    ("Balances", "transfer_allow_death", {"dest": BOB.ss58_address, "value": 1}),
    (
        "SubtensorModule",
        "add_stake",
        {"hotkey": ALICE_HOT.ss58_address, "netuid": 1, "amount_staked": 10**9},
    ),
    (
        "SubtensorModule",
        "remove_stake",
        {"hotkey": ALICE_HOT.ss58_address, "netuid": 1, "amount_unstaked": 5},
    ),
    ("SubtensorModule", "burned_register", {"netuid": 1, "hotkey": ALICE_HOT.ss58_address}),
    (
        "SubtensorModule",
        "set_weights",
        {"netuid": 1, "dests": [0, 1], "weights": [100, 200], "version_key": 0},
    ),
    ("System", "remark", {"remark": "0x00010203"}),
]

CONSTANT_CASES = [
    ("Balances", "ExistentialDeposit"),
    ("Aura", "SlotDuration"),
    ("System", "SS58Prefix"),
]

STORAGE_VALUE_CASES = [
    ("System", "Account", [ALICE.ss58_address]),
    ("SubtensorModule", "Tempo", [1]),
    ("SubtensorModule", "NetworksAdded", [1]),
    ("SubtensorModule", "NetworksAdded", [9999]),  # None path (map miss)
    ("SubtensorModule", "TotalNetworks", []),
    ("SubtensorModule", "SubnetOwner", [1]),
    ("Timestamp", "Now", []),
    ("System", "Number", []),
]

QUERY_MAP_CASES = [
    ("SubtensorModule", "NetworksAdded", []),
    ("SubtensorModule", "Tempo", []),
    ("SubtensorModule", "Keys", [1]),  # double map, first key fixed
    ("System", "Account", []),
]

RUNTIME_CALL_CASES = [
    ("NeuronInfoRuntimeApi", "get_neurons_lite", [1]),
    ("SubnetInfoRuntimeApi", "get_subnet_hyperparams", [1]),
    (
        "StakeInfoRuntimeApi",
        "get_stake_info_for_hotkey_coldkey_netuid",
        [ALICE_HOT.ss58_address, ALICE.ss58_address, 1],
    ),
    ("AccountNonceApi", "account_nonce", [ALICE.ss58_address]),
]


async def record(sub: SubstrateConnection) -> dict[str, Any]:
    rpc = sub.rpc_request
    codec = await sub._runtimes.codec_at(None)
    golden: dict[str, Any] = {"endpoint": ENDPOINT}

    # -- network / runtime identity -------------------------------------------
    genesis = await sub.genesis_hash()
    golden["network"] = {
        "genesis_hash": genesis,
        "spec_version": codec.spec_version,
        "transaction_version": codec.transaction_version,
        "ss58_format": SS58_FORMAT,
    }

    # -- raw metadata blobs (enable fully offline replay) ---------------------
    head = await sub.get_chain_head()
    v15_raw = await rpc("state_call", ["Metadata_metadata_at_version", "0x0f000000", head])
    v14_raw = await rpc("state_getMetadata", [head])
    golden["metadata"] = {"block_hash": head, "v15_hex": v15_raw, "v14_hex": v14_raw}
    assert strip_option_opaque_metadata(bytes.fromhex(v15_raw[2:])) is not None

    # -- storage keys ----------------------------------------------------------
    keys = []
    for pallet, fn, params in STORAGE_KEY_CASES:
        entry = codec.storage_entry(pallet, fn)
        key = codec.storage_key(entry, list(params))
        keys.append(
            {
                "pallet": pallet,
                "storage_function": fn,
                "params": jsonable(params),
                "key_hex": "0x" + key.hex(),
                "value_scale_type": entry.value_type,
            }
        )
    golden["storage_keys"] = keys

    # -- composed calls --------------------------------------------------------
    def compose(module: str, function: str, params: dict):
        return codec.compose_call(module, function, params)

    calls = []
    for module, function, params in CALL_CASES:
        call = compose(module, function, params)
        calls.append(
            {
                "module": module,
                "function": function,
                "params": jsonable(params),
                "data_hex": "0x" + codec.call_data(call).hex(),
            }
        )
    # Nested composition cases.
    inner = compose("System", "remark", {"remark": "0xdeadbeef"})
    sudo = compose("Sudo", "sudo", {"call": inner})
    calls.append(
        {
            "module": "Sudo",
            "function": "sudo",
            "params": {"call": {"module": "System", "function": "remark", "remark": "0xdeadbeef"}},
            "data_hex": "0x" + codec.call_data(sudo).hex(),
        }
    )
    t1 = compose("Balances", "transfer_keep_alive", {"dest": BOB.ss58_address, "value": 1})
    t2 = compose("Balances", "transfer_keep_alive", {"dest": CHARLIE.ss58_address, "value": 2})
    batch = compose("Utility", "batch", {"calls": [t1, t2]})
    calls.append(
        {
            "module": "Utility",
            "function": "batch",
            "params": {"calls": ["transfer 1 to bob", "transfer 2 to charlie"]},
            "data_hex": "0x" + codec.call_data(batch).hex(),
        }
    )
    proxied = compose(
        "Proxy", "proxy", {"real": ALICE.ss58_address, "force_proxy_type": "Transfer", "call": t1}
    )
    calls.append(
        {
            "module": "Proxy",
            "function": "proxy",
            "params": {
                "real": ALICE.ss58_address,
                "force_proxy_type": "Transfer",
                "call": "transfer 1 to bob",
            },
            "data_hex": "0x" + codec.call_data(proxied).hex(),
        }
    )
    golden["calls"] = calls

    # -- signature payloads (immortal + mortal) --------------------------------
    transfer = compose(
        "Balances", "transfer_keep_alive", {"dest": BOB.ss58_address, "value": 12345}
    )
    payloads = []
    immortal = codec.signature_payload(
        transfer,
        era="00",
        nonce=7,
        tip=0,
        tip_asset_id=None,
        genesis_hash=genesis,
        era_block_hash=genesis,
    )
    payloads.append(
        {
            "call_data_hex": "0x" + codec.call_data(transfer).hex(),
            "era": "00",
            "nonce": 7,
            "tip": 0,
            "payload_hex": "0x" + immortal.hex(),
        }
    )
    current = await sub.get_block_number(await sub.get_chain_head())
    mortal_era = {"period": 64, "current": current}
    birth_hash = await sub.get_block_hash(codec.era_birth(dict(mortal_era), current))
    mortal = codec.signature_payload(
        transfer,
        era=dict(mortal_era),
        nonce=3,
        tip=0,
        tip_asset_id=None,
        genesis_hash=genesis,
        era_block_hash=birth_hash,
    )
    payloads.append(
        {
            "call_data_hex": "0x" + codec.call_data(transfer).hex(),
            "era": {"period": 64, "current": current},
            "era_birth_block_hash": birth_hash,
            "nonce": 3,
            "tip": 0,
            "payload_hex": "0x" + mortal.hex(),
        }
    )
    golden["signature_payloads"] = payloads

    # -- deterministic signed extrinsic encodings (fixed fake signature) -------
    extrinsics = []
    for era, nonce in ((None, 0), ({"period": 64, "current": current}, 11)):
        ext = await sub.sign_without_nonce_tracking(
            transfer,
            ALICE,
            era=dict(era) if isinstance(era, dict) else era,
            nonce=nonce,
            signature=b"\x01" + FAKE_SIG,  # 65 bytes: [version, sig]; 01 = sr25519
        )
        extrinsics.append(
            {
                "call_data_hex": "0x" + codec.call_data(transfer).hex(),
                "address": ALICE.ss58_address,
                "public_key_hex": "0x" + ALICE.public_key.hex(),
                "crypto_type": ALICE.crypto_type,
                "era": jsonable(era) if era else "00",
                "nonce": nonce,
                "tip": 0,
                "signature_hex": "0x" + FAKE_SIG.hex(),
                "signature_version": 1,
                "extrinsic_hex": ext.data_hex,
                "extrinsic_hash": ext.extrinsic_hash,
            }
        )
    golden["extrinsics"] = extrinsics

    # -- multisig derivation ----------------------------------------------------
    multisig = []
    for signers, threshold in (
        ([ALICE.ss58_address, BOB.ss58_address], 2),
        ([ALICE.ss58_address, BOB.ss58_address, CHARLIE.ss58_address], 2),
        ([CHARLIE.ss58_address, ALICE.ss58_address, BOB.ss58_address], 3),
    ):
        account = sub.generate_multisig_account(list(signers), threshold)
        multisig.append(
            {
                "signatories": list(signers),
                "threshold": threshold,
                "ss58_address": account.ss58_address,
            }
        )
    golden["multisig"] = multisig

    # -- ss58 vectors ------------------------------------------------------------
    golden["ss58"] = [
        {"public_key_hex": "0x" + kp.public_key.hex(), "address": kp.ss58_address}
        for kp in (ALICE, BOB, CHARLIE, ALICE_HOT)
    ]

    # -- constants (decoded) ------------------------------------------------------
    golden["constants"] = [
        {"module": module, "name": name, "decoded": jsonable(await sub.get_constant(module, name))}
        for module, name in CONSTANT_CASES
    ]

    # -- submit one real transfer so events/receipt/block vectors exist ----------
    live_transfer = compose(
        "Balances", "transfer_keep_alive", {"dest": BOB.ss58_address, "value": 10**9}
    )
    signed = await sub.create_signed_extrinsic(live_transfer, ALICE)
    report = await sub.submit_extrinsic(
        signed, wait_for_inclusion=True, wait_for_finalization=False
    )
    assert report.is_success, report.error_message
    pin = report.block_hash
    golden["receipt"] = {
        "block_hash": pin,
        "extrinsic_idx": report.extrinsic_idx,
        "total_fee_amount": report.total_fee_amount,
        "is_success": True,
        "triggered_events": jsonable(report.triggered_events),
    }

    # -- storage values (raw + decoded, pinned) -----------------------------------
    values = []
    for pallet, fn, params in STORAGE_VALUE_CASES:
        entry = codec.storage_entry(pallet, fn)
        key_hex = "0x" + codec.storage_key(entry, list(params)).hex()
        raw = await rpc("state_getStorage", [key_hex, pin])
        decoded = await sub.query(pallet, fn, list(params), block_hash=pin)
        values.append(
            {
                "pallet": pallet,
                "storage_function": fn,
                "params": jsonable(params),
                "block_hash": pin,
                "key_hex": key_hex,
                "raw_hex": raw,
                "decoded": jsonable(decoded),
            }
        )
    golden["storage_values"] = values

    # -- raw events at the inclusion block ----------------------------------------
    events_entry = codec.storage_entry("System", "Events")
    events_key = "0x" + codec.storage_key(events_entry, []).hex()
    golden["events"] = {
        "block_hash": pin,
        "raw_hex": await rpc("state_getStorage", [events_key, pin]),
        "decoded": jsonable(await sub.get_events(block_hash=pin)),
    }

    # -- query maps (pinned) -------------------------------------------------------
    maps = []
    for pallet, fn, params in QUERY_MAP_CASES:
        result = await sub.query_map(pallet, fn, list(params), block_hash=pin, page_size=50)
        pairs = []
        async for k, v in result:
            pairs.append([jsonable(k), jsonable(v)])
            if len(pairs) >= 25:
                break
        entry = codec.storage_entry(pallet, fn)
        prefix_hex = "0x" + codec.storage_key(entry, list(params)).hex()
        raw_keys = await rpc("state_getKeysPaged", [prefix_hex, 25, None, pin])
        raw_changes: list = []
        if raw_keys:
            for group in await rpc("state_queryStorageAt", [raw_keys, pin]) or []:
                raw_changes.extend(group["changes"])
        maps.append(
            {
                "pallet": pallet,
                "storage_function": fn,
                "params": jsonable(params),
                "block_hash": pin,
                "prefix_hex": prefix_hex,
                "raw_keys": raw_keys,
                "raw_changes": raw_changes,
                "pairs": pairs,
            }
        )
    golden["query_maps"] = maps

    # -- runtime API calls (raw + decoded, pinned) ----------------------------------
    runtime_calls = []
    for api, method, params in RUNTIME_CALL_CASES:
        encoded_params = encode_runtime_api_params(codec, api, method, list(params))
        raw = await rpc("state_call", [f"{api}_{method}", encoded_params or "0x", pin])
        decoded = await sub.runtime_call(api, method, list(params), block_hash=pin)
        runtime_calls.append(
            {
                "api": api,
                "method": method,
                "params": jsonable(params),
                "block_hash": pin,
                "encoded_params_hex": encoded_params,
                "raw_hex": raw,
                "decoded": jsonable(decoded),
            }
        )
    golden["runtime_calls"] = runtime_calls

    # -- block decode (the block containing our transfer) ----------------------------
    raw_block = await rpc("chain_getBlock", [pin])
    decoded_block = await sub.get_block(block_hash=pin, ignore_decoding_errors=True)
    golden["block"] = {
        "block_hash": pin,
        "raw": raw_block,
        "decoded_header": jsonable(decoded_block.header),
        "decoded_extrinsics": jsonable(decoded_block.extrinsics),
    }

    # -- payment info shape ------------------------------------------------------------
    info = await sub.get_payment_info(transfer, ALICE)
    golden["payment_info"] = {"keys": sorted(info.keys()), "weight": jsonable(info.get("weight"))}

    return golden


async def main() -> None:
    sub = SubstrateConnection(ENDPOINT, ss58_format=SS58_FORMAT)
    try:
        await sub.initialize()
        golden = await record(sub)
    finally:
        await sub.close()

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(golden, indent=1))
    size_mb = OUT.stat().st_size / 1e6
    counts = {
        k: (len(v) if isinstance(v, list) else 1) for k, v in golden.items() if k != "endpoint"
    }
    print(f"wrote {OUT} ({size_mb:.1f} MB): {counts}")


if __name__ == "__main__":
    asyncio.run(main())
