"""Wallet display always keeps receiving descriptors primary across registration states."""

from __future__ import annotations

import asyncio
import json
import subprocess
from contextlib import nullcontext
from functools import cache
from types import SimpleNamespace

import pytest

from bittensor.cli import wallet_addresses
from bittensor.cli.wallet_addresses import (
    address_fields,
    check_registration,
    lookup_registration,
    public_key_from_file,
    query_registration,
)
from bittensor.hashed import descriptor_storage, descriptor_value
from bittensor.keyfiles import KeyfileError
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair, serialized_keypair_to_keyfile_data

FINALIZED_HASH = "0x" + "ab" * 32
REGISTERED_CODES = (4, 6, 7, 8)


@cache
def public_key(code, seed=31):
    return Keypair.create_from_seed(bytes([seed]) * 32, code).public_only()


class PublicMetadataOnly:
    """Permit public identity data and fail if display asks for signing material."""

    def __init__(self, public):
        self.crypto_type = public.crypto_type
        self.ss58_address = public.ss58_address
        if public.crypto_type in REGISTERED_CODES:
            self.hashed_descriptor = bytes(public.hashed_descriptor)

    def __getattr__(self, name):
        raise AssertionError(f"display accessed non-public field {name}")


class RegistrationChain:
    def __init__(self, records=None, *, block_hash=FINALIZED_HASH, error_at=None):
        self.records = records or {}
        self.finalized_hash = block_hash
        self.error_at = error_at
        self.calls = []

    async def get_finalized_block_number(self):
        self.calls.append(("finalized_block_number",))
        if self.error_at == "finalized_block_number":
            raise ConnectionError("test RPC unavailable")
        return 37

    async def get_block_hash(self, number):
        self.calls.append(("block_hash", number))
        assert number == 37, "display must resolve the finalized block, not best head"
        if self.error_at == "block_hash":
            raise ConnectionError("test finalized hash unavailable")
        return self.finalized_hash

    async def query(self, module, storage, params=None, *, block_hash=None):
        self.calls.append(("query", module, storage, params, block_hash))
        assert module == "HashedAccounts"
        assert block_hash == FINALIZED_HASH, "every registration read must be finalized"
        assert storage in ("Accounts", "MlDsaAccounts")
        if self.error_at == "query":
            raise ConnectionError("test storage unavailable")
        result = self.records.get((storage, params[0]))
        if isinstance(result, Exception):
            raise result
        return result


def registry_record(public):
    return {"descriptor": descriptor_value(bytes(public.hashed_descriptor))}


def registry_key(public):
    return descriptor_storage(bytes(public.hashed_descriptor)), public.ss58_address


@pytest.mark.parametrize("code", REGISTERED_CODES)
@pytest.mark.parametrize("role", ("", "coldkey", "hotkey"))
def test_registered_wallet_fields_include_both_addresses_without_secrets(code, role):
    public = public_key(code)
    fields = address_fields(PublicMetadataOnly(public), network="test", role=role)
    prefix = f"{role}_" if role else ""
    expected_address = receiving_address(public)
    assert fields[prefix + "ss58"] == public.ss58_address
    assert fields[prefix + "address"] == expected_address
    assert fields[prefix + "receiving_address"] == expected_address
    assert fields[prefix + "primary_address"] == expected_address
    assert fields[prefix + "registration"] == "not_checked"
    assert fields[prefix + "registration_network"] == "test"
    assert fields[prefix + "registration_block_hash"] is None


@pytest.mark.parametrize("code", (0, 1))
@pytest.mark.parametrize("role", ("", "coldkey", "hotkey"))
def test_ordinary_wallet_fields_keep_ss58_without_registration(code, role):
    public = public_key(code)
    fields = address_fields(PublicMetadataOnly(public), network="test", role=role)
    assert fields == {f"{role}_ss58" if role else "ss58": public.ss58_address}


@pytest.mark.parametrize("code", REGISTERED_CODES)
async def test_matching_finalized_registration_keeps_receiving_address_primary(code):
    public = public_key(code)
    fields = address_fields(public, network="test")
    chain = RegistrationChain({registry_key(public): registry_record(public)})

    await query_registration(chain, [fields])

    assert fields["registration"] == "registered"
    assert fields["registration_block_hash"] == FINALIZED_HASH
    assert fields["primary_address"] == receiving_address(public)
    assert fields["ss58"] == public.ss58_address
    # The complete descriptor stays available to payment and public recovery consumers.
    assert fields["address"] == fields["receiving_address"] == receiving_address(public)
    assert fields["registration_network"] == "test"
    assert chain.calls == [
        ("finalized_block_number",),
        ("block_hash", 37),
        ("query", "HashedAccounts", registry_key(public)[0], [public.ss58_address], FINALIZED_HASH),
    ]


async def test_mixed_wallets_use_one_finalized_snapshot_and_each_correct_registry():
    keys = [public_key(code) for code in REGISTERED_CODES]
    records = [address_fields(public, network="test") for public in keys]
    ordinary = address_fields(public_key(1), network="test")
    chain = RegistrationChain({registry_key(key): registry_record(key) for key in keys})

    await query_registration(chain, [ordinary, *records])

    assert chain.calls.count(("finalized_block_number",)) == 1
    assert chain.calls.count(("block_hash", 37)) == 1
    assert len([call for call in chain.calls if call[0] == "query"]) == 4
    assert all(record["registration"] == "registered" for record in records)
    assert all(record["registration_block_hash"] == FINALIZED_HASH for record in records)
    assert ordinary == {"ss58": public_key(1).ss58_address}


@pytest.mark.parametrize("code", REGISTERED_CODES)
async def test_missing_registration_keeps_complete_receiving_address_primary(code):
    public = public_key(code)
    fields = address_fields(public, network="test")

    await query_registration(RegistrationChain(), [fields])

    assert fields["registration"] == "unregistered"
    assert fields["registration_block_hash"] == FINALIZED_HASH
    assert fields["primary_address"] == receiving_address(public)
    assert fields["ss58"] == public.ss58_address


@pytest.mark.parametrize("code", REGISTERED_CODES)
async def test_different_registered_descriptor_keeps_receiving_address_primary(code):
    public = public_key(code)
    fields = address_fields(public, network="test")
    chain = RegistrationChain({registry_key(public): registry_record(public_key(code, 32))})

    await query_registration(chain, [fields])

    assert fields["registration"] == "mismatch"
    assert fields["primary_address"] == receiving_address(public)


@pytest.mark.parametrize("bad_record", ({}, {"descriptor": {}}, {"descriptor": None}, "invalid"))
async def test_malformed_registry_result_is_unavailable_not_registered(bad_record):
    public = public_key(8)
    fields = address_fields(public, network="test")
    chain = RegistrationChain({registry_key(public): bad_record})

    await query_registration(chain, [fields])

    assert fields["registration"] == "unavailable"
    assert fields["primary_address"] == receiving_address(public)


@pytest.mark.parametrize("error_at", ("finalized_block_number", "block_hash", "query"))
async def test_rpc_failure_does_not_make_local_wallet_display_fail(error_at):
    public = public_key(8)
    fields = address_fields(public, network="ws://127.0.0.1:1")
    chain = RegistrationChain(error_at=error_at)

    await query_registration(chain, [fields])

    assert fields["registration"] == "unavailable"
    assert fields["primary_address"] == receiving_address(public)
    assert fields["registration_network"] == "ws://127.0.0.1:1"


@pytest.mark.parametrize("missing_hash", (None, ""))
async def test_missing_finalized_hash_never_reads_best_head_registration(missing_hash):
    public = public_key(8)
    fields = address_fields(public, network="test")
    chain = RegistrationChain(
        {registry_key(public): registry_record(public)}, block_hash=missing_hash
    )

    await query_registration(chain, [fields])

    assert fields["registration"] == "unavailable"
    assert fields["primary_address"] == receiving_address(public)
    assert not any(call[0] == "query" for call in chain.calls)


async def test_one_unavailable_registry_does_not_hide_other_wallet_registration():
    keys = [public_key(4), public_key(8)]
    records = [address_fields(key, network="test") for key in keys]
    chain = RegistrationChain(
        {
            registry_key(keys[0]): registry_record(keys[0]),
            registry_key(keys[1]): ValueError("storage absent on this runtime"),
        }
    )

    await query_registration(chain, records)

    assert records[0]["registration"] == "registered"
    assert records[0]["primary_address"] == receiving_address(keys[0])
    assert records[1]["registration"] == "unavailable"
    assert records[1]["primary_address"] == receiving_address(keys[1])


@pytest.mark.parametrize("second_network", ("test", "ws://127.0.0.1:20644"))
async def test_registration_is_rechecked_when_network_or_localnet_state_changes(second_network):
    public = public_key(8)
    first = address_fields(public, network="test")
    await query_registration(
        RegistrationChain({registry_key(public): registry_record(public)}), [first]
    )
    assert first["registration"] == "registered"

    second = address_fields(public, network=second_network)
    second_chain = RegistrationChain()
    await query_registration(second_chain, [second])

    assert second["registration"] == "unregistered"
    assert second["primary_address"] == receiving_address(public)
    assert second["registration_network"] == second_network
    assert second_chain.calls


async def test_ordinary_keys_need_no_registration_queries():
    fields = address_fields(public_key(1), network="test")
    chain = RegistrationChain(error_at="finalized_block_number")

    await query_registration(chain, [fields])

    assert fields == {"ss58": public_key(1).ss58_address}
    assert chain.calls == []


def display_context(network="ws://127.0.0.1:20644"):
    return SimpleNamespace(
        network=network,
        retry_forever=True,
        fallback_endpoints="wss://unrelated-network.example:443",
        archive_endpoints="wss://another-network.example:443",
        output=SimpleNamespace(activity=lambda *_: nullcontext()),
    )


def test_offline_display_resets_previous_success_without_contacting_rpc(monkeypatch):
    public = public_key(8)
    fields = address_fields(public, network="test")
    fields.update(
        registration="registered",
        primary_address=public.ss58_address,
        registration_block_hash=FINALIZED_HASH,
    )
    monkeypatch.setattr(
        wallet_addresses.subprocess,
        "run",
        lambda *_args, **_kwargs: pytest.fail("offline display must not connect"),
    )

    check_registration(display_context(), [fields], offline=True)

    assert fields["registration"] == "not_checked"
    assert fields["registration_network"] == "ws://127.0.0.1:20644"
    assert fields["registration_block_hash"] is None
    assert fields["primary_address"] == receiving_address(public)


@pytest.mark.parametrize("empty", (True, False))
def test_no_descriptor_wallets_do_not_open_connection(monkeypatch, empty):
    fields = [] if empty else [address_fields(public_key(1), network="test")]
    monkeypatch.setattr(
        wallet_addresses.subprocess,
        "run",
        lambda *_args, **_kwargs: pytest.fail("ordinary wallet display must not connect"),
    )

    check_registration(display_context(), fields)


class DisplayConnection(RegistrationChain):
    def __init__(self, *args, initialize_error=None, stall_at=None, **kwargs):
        super().__init__(*args, **kwargs)
        self.initialize_error = initialize_error
        self.stall_at = stall_at
        self.closed = False
        self.cancelled = False

    async def initialize(self):
        if self.initialize_error:
            raise self.initialize_error
        if self.stall_at == "initialize":
            await self.stall()

    async def query(self, *args, **kwargs):
        if self.stall_at == "query":
            await self.stall()
        return await super().query(*args, **kwargs)

    async def stall(self):
        try:
            await asyncio.Event().wait()
        except asyncio.CancelledError:
            self.cancelled = True
            raise

    async def close(self):
        self.closed = True


async def test_selected_endpoint_is_pinned_without_fallbacks(monkeypatch):
    public = public_key(8)
    fields = address_fields(public, network="test")
    chain = DisplayConnection({registry_key(public): registry_record(public)})
    opened = []

    def connect(endpoint, **options):
        opened.append((endpoint, options))
        return chain

    monkeypatch.setattr(wallet_addresses, "SubstrateConnection", connect)

    await lookup_registration(display_context().network, [fields])

    assert fields["registration"] == "registered"
    assert fields["registration_network"] == "ws://127.0.0.1:20644"
    assert fields["primary_address"] == receiving_address(public)
    assert opened[0][0] == "ws://127.0.0.1:20644"
    assert opened[0][1]["fallback_urls"] == []
    assert opened[0][1]["max_retries"] == 1
    assert chain.closed


@pytest.mark.parametrize("stall_at", ("initialize", "query"))
async def test_registration_timeout_cancels_work_and_closes_connection(monkeypatch, stall_at):
    public = public_key(8)
    fields = address_fields(public, network="test")
    chain = DisplayConnection(stall_at=stall_at)
    monkeypatch.setattr(wallet_addresses, "SubstrateConnection", lambda *_a, **_k: chain)
    monkeypatch.setattr(wallet_addresses, "REGISTRATION_TIMEOUT", 0.02)

    with pytest.raises(TimeoutError):
        await lookup_registration(display_context().network, [fields])

    assert chain.cancelled
    assert chain.closed
    assert fields["registration"] == "unavailable"
    assert fields["registration_block_hash"] is None
    assert fields["primary_address"] == receiving_address(public)


async def test_failed_initialization_still_closes_connection_and_keeps_wallet_visible(monkeypatch):
    public = public_key(8)
    fields = address_fields(public, network="test")
    chain = DisplayConnection(initialize_error=ConnectionError("test connect failure"))
    monkeypatch.setattr(wallet_addresses, "SubstrateConnection", lambda *_a, **_k: chain)

    with pytest.raises(ConnectionError, match="test connect failure"):
        await lookup_registration(display_context().network, [fields])

    assert chain.closed
    assert fields["registration"] == "unavailable"
    assert fields["primary_address"] == receiving_address(public)


def test_later_failed_lookup_cannot_reuse_earlier_registration_success(monkeypatch):
    public = public_key(8)
    fields = address_fields(public, network="test")

    def run(command, *, input, **options):
        request = json.loads(input)
        if request["network"] != "ws://127.0.0.1:20644":
            raise subprocess.TimeoutExpired(command, options["timeout"])
        registered = {
            **request["records"][0],
            "registration": "registered",
            "primary_address": public.ss58_address,
            "registration_block_hash": FINALIZED_HASH,
            "registration_network": request["network"],
        }
        return SimpleNamespace(stdout=json.dumps([registered]))

    monkeypatch.setattr(wallet_addresses.subprocess, "run", run)

    check_registration(display_context(), [fields])
    assert fields["registration"] == "registered"
    assert fields["primary_address"] == receiving_address(public)
    check_registration(display_context("ws://127.0.0.1:20645"), [fields])

    assert fields["registration"] == "unavailable"
    assert fields["registration_network"] == "ws://127.0.0.1:20645"
    assert fields["registration_block_hash"] is None
    assert fields["primary_address"] == receiving_address(public)


def test_lookup_worker_gets_only_public_identities_and_hard_timeout(monkeypatch):
    public = public_key(8)
    fields = address_fields(public, network="test")
    fields.update(coldkey_mnemonic="must not leave parent", hotkeys=[{"seed": "private"}])
    seen = []

    def run(command, *, input, timeout, **options):
        request = json.loads(input)
        seen.append((command, request, timeout, options))
        rows = [
            {
                **record,
                "registration": "registered",
                "primary_address": record["ss58"],
                "registration_block_hash": FINALIZED_HASH,
                "registration_network": request["network"],
            }
            for record in request["records"]
        ]
        return SimpleNamespace(stdout=json.dumps(rows))

    monkeypatch.setattr(wallet_addresses.subprocess, "run", run)

    check_registration(display_context(), [fields])

    command, request, timeout, options = seen[0]
    assert command[-2:] == ["-m", "bittensor.cli.wallet_addresses"]
    assert request == {
        "network": "ws://127.0.0.1:20644",
        "records": [{"ss58": public.ss58_address, "receiving_address": receiving_address(public)}],
    }
    assert timeout == wallet_addresses.REGISTRATION_TIMEOUT + 1.0
    assert options["check"] is True
    assert fields["registration"] == "registered"
    # Ignore a stale worker's SS58 promotion, even when its identity matches.
    assert fields["primary_address"] == receiving_address(public)
    assert fields["coldkey_mnemonic"] == "must not leave parent"


@pytest.mark.parametrize("failure", ("timeout", "exit", "invalid_json", "wrong_identity"))
def test_failed_or_unrelated_worker_result_keeps_receiving_address_primary(monkeypatch, failure):
    public = public_key(8)
    fields = address_fields(public, network="test")

    def run(command, *, input, **options):
        if failure == "timeout":
            raise subprocess.TimeoutExpired(command, options["timeout"])
        if failure == "exit":
            raise subprocess.CalledProcessError(1, command)
        if failure == "invalid_json":
            return SimpleNamespace(stdout="not JSON")
        request = json.loads(input)
        unrelated = {
            **request["records"][0],
            "ss58": public_key(8, 32).ss58_address,
            "registration": "registered",
            "primary_address": public_key(8, 32).ss58_address,
            "registration_block_hash": FINALIZED_HASH,
            "registration_network": request["network"],
        }
        return SimpleNamespace(stdout=json.dumps([unrelated]))

    monkeypatch.setattr(wallet_addresses.subprocess, "run", run)

    check_registration(display_context(), [fields])

    assert fields["registration"] == "unavailable"
    assert fields["registration_block_hash"] is None
    assert fields["primary_address"] == receiving_address(public)


@pytest.mark.parametrize("code", (0, 1, *REGISTERED_CODES))
def test_public_reader_preserves_legacy_leading_space_address(tmp_path, code):
    public = public_key(code)
    data = json.loads(bytes(serialized_keypair_to_keyfile_data(public)))
    data[" ss58Address"] = data.pop("ss58Address")
    path = tmp_path / "public.json"
    path.write_text(json.dumps(data))

    restored = public_key_from_file(path)

    assert restored.ss58_address == public.ss58_address
    assert restored.crypto_type == public.crypto_type
    if code in REGISTERED_CODES:
        assert receiving_address(restored) == receiving_address(public)


@pytest.mark.parametrize("code", (0, 1, *REGISTERED_CODES))
def test_public_reader_prefers_canonical_address_over_legacy_alias(tmp_path, code):
    public = public_key(code)
    data = json.loads(bytes(serialized_keypair_to_keyfile_data(public)))
    data[" ss58Address"] = public_key(code, 32).ss58_address
    path = tmp_path / "public.json"
    path.write_text(json.dumps(data))

    restored = public_key_from_file(path)

    assert restored.ss58_address == public.ss58_address


@pytest.mark.parametrize("code", REGISTERED_CODES)
@pytest.mark.parametrize("canonical_present", (False, True))
def test_public_reader_never_discards_mismatched_stored_hashed_identity(
    tmp_path, code, canonical_present
):
    public = public_key(code)
    data = json.loads(bytes(serialized_keypair_to_keyfile_data(public)))
    data[" ss58Address"] = data.pop("ss58Address")
    if canonical_present:
        # A valid legacy address must not override a mismatched canonical one.
        data["ss58Address"] = public_key(code, 32).ss58_address
    else:
        data[" ss58Address"] = public_key(code, 32).ss58_address
    path = tmp_path / "public.json"
    path.write_text(json.dumps(data))

    with pytest.raises(KeyfileError, match="ss58Address does not match hashedDescriptor"):
        public_key_from_file(path)
