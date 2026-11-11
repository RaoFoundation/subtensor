"""Protected setup recognizes the runtime's destination-and-policy V2 commitment."""

from __future__ import annotations

import asyncio
import json
from hashlib import blake2b
from types import SimpleNamespace

import pytest
import typer

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import prompt
from bittensor.cli import protected as protected_cli
from bittensor.cli.context import AppContext
from bittensor.client import Client
from bittensor.intents import AnnounceColdkeySwap, Transfer
from bittensor.sp_core import Keypair, ss58_decode
from tests.conftest import golden_codec
from tests.harness.fake_substrate import FakeSubstrate
from tests.unit.test_protected_setup_resume import GENESIS, _SetupChain


def _runtime_v2_commitment(destination, import_source_policies):
    # Rust coldkey_swap_v2_announcement_hash hashes the SCALE tuple
    # (&[u8; 22], &AccountId32, bool): fixed bytes have no compact length prefix.
    encoded = golden_codec().encode(
        "([u8; 22], AccountId32, bool)",
        [list(b"coldkey-swap-v2-policy"), destination, import_source_policies],
    )
    assert len(encoded) == 55
    return "0x" + blake2b(encoded, digest_size=32).hexdigest()


class _V2SetupChain(_SetupChain):
    """Use the actual SDK intent and read around runtime-shaped storage."""

    def __init__(self, controller):
        super().__init__(controller)
        self.storage = FakeSubstrate()
        self.client = Client("local", substrate=self.storage)
        self.import_source_policies = False
        self.built_swap_calls = []
        self.swap_fees_paid = 0

    def seed_swap(self, *, destination=None, committed_policy=False, stored_policy=False):
        commitment = _runtime_v2_commitment(destination or self.pure, committed_policy)
        self.swap = {"execute_block": 200, "new_coldkey_hash": commitment}
        self.storage.seed(
            "SubtensorModule", "ColdkeySwapAnnouncements", [self.controller], (200, commitment)
        )
        self.storage.seed(
            "SubtensorModule", "ColdkeySwapV2PolicySnapshots", [self.controller], stored_policy
        )

    def announce_swap(self):
        self.pending_multisig = False
        self.swap_fees_paid += 1
        self.seed_swap(
            committed_policy=self.import_source_policies,
            stored_policy=self.import_source_policies,
        )

    async def read(self, name, **kwargs):
        # Retain the existing transport-failure injection, then exercise the
        # production typed read rather than hand-writing its result dictionary.
        result = await super().read(name, **kwargs)
        if name == "coldkey_swap_announcement":
            return await self.client.read(name, **kwargs)
        return result

    def submit(self, intent, **kwargs):
        if isinstance(intent, AnnounceColdkeySwap):
            built = asyncio.run(intent.build(self.storage, SimpleNamespace()))
            assert built.module == "SubtensorModule"
            assert built.function == "announce_coldkey_swap_v2"
            assert built.params == {"new_coldkey": self.pure}
            self.built_swap_calls.append(built)
        return super().submit(intent, **kwargs)


@pytest.fixture
def v2_setup(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    monkeypatch.setenv("BTCLI_PROXIES_PATH", str(tmp_path / "proxies.json"))
    controller = Keypair.create_from_seed(bytes([141]) * 32, 1).ss58_address
    chain = _V2SetupChain(controller)
    output = SimpleNamespace(messages=[], errors=[], json_mode=True)
    output.message = output.messages.append
    output.error = lambda message, **_kw: output.errors.append(message)
    ctx = AppContext(
        network="test",
        wallet_name="owner",
        hotkey_name=None,
        wallet_path=None,
        assume_yes=True,
        dry_run=False,
        output=output,
    )
    monkeypatch.setattr(ctx, "run", lambda callback: asyncio.run(callback(chain)))
    monkeypatch.setattr(ctx, "wallet", lambda: SimpleNamespace())
    monkeypatch.setattr(ctx, "resolve_address", lambda _param, value: value or controller)
    monkeypatch.setattr(ctx, "submit", chain.submit)
    monkeypatch.setattr(prompt, "confirm_wallet", lambda *_a, **_kw: "owner")

    async def compose(_substrate, intent, _real, _wallet):
        payload = json.dumps(intent.to_dict(), sort_keys=True).encode()
        digest = blake2b(payload, digest_size=32).digest()
        call_hash = "0x" + digest.hex()
        chain.payloads[call_hash] = intent.to_dict()
        return SimpleNamespace(data=payload, call_hash=digest), call_hash, {}

    monkeypatch.setattr(protected_cli, "compose_announceable_call", compose)
    return ctx, chain


def _protect(ctx):
    protected_cli.run_protect(ctx, delay_raw=None, allow=[], disallow=[], allow_key=None)


def _stored():
    return pa.get_by_controller("owner", genesis_hash=GENESIS)


def _assert_setup_ready_and_routes(ctx, chain):
    record = _stored()
    assert record is not None
    assert not pa.protected_setup_pending(record)
    assert record["coldkey_swap"]["execute_block"] == 200
    assert record["coldkey_swap"]["new_coldkey_ss58"] == chain.pure
    # Verify the user-visible consequence: setup no longer refuses every
    # subsequent protected transaction as "incomplete".
    route = protected_cli.maybe_route_protected(
        ctx,
        Transfer(dest_ss58=chain.controller, amount_tao=1),
        proxy_for=None,
        user_set_proxy=False,
    )
    assert route == (None, None, True)
    queued = ann.list_for_wallet("owner", genesis_hash=GENESIS)
    assert queued[-1]["real_ss58"] == chain.pure
    assert queued[-1]["inner_op"] == "transfer"


# PR #3305: real V2 migration commitments must finish setup and enable protected transactions.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("import_source_policies", [False, True])
def test_real_v2_announcement_finishes_protected_setup(v2_setup, import_source_policies):
    ctx, chain = v2_setup
    chain.import_source_policies = import_source_policies

    _protect(ctx)

    assert len(chain.built_swap_calls) == 1
    assert chain.swap_fees_paid == 1
    assert (
        chain.swap["new_coldkey_hash"]
        != "0x" + blake2b(bytes(ss58_decode(chain.pure)), digest_size=32).hexdigest()
    )
    _assert_setup_ready_and_routes(ctx, chain)


# PR #3305: a lost V2 migration receipt must resume setup without another migration payment.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("import_source_policies", [False, True])
def test_v2_setup_resumes_after_receipt_loss_without_second_payment(
    v2_setup, import_source_policies
):
    ctx, chain = v2_setup
    chain.import_source_policies = import_source_policies
    chain.lose_swap_receipt = True
    with pytest.raises(OSError, match="migration receipt interrupted"):
        _protect(ctx)
    assert chain.swap_fees_paid == 1
    assert pa.protected_setup_pending(_stored())

    _protect(ctx)

    assert len(chain.built_swap_calls) == 1
    assert chain.swap_fees_paid == 1
    assert len([intent for intent in chain.submitted if intent.op == "create_pure_proxy"]) == 1
    _assert_setup_ready_and_routes(ctx, chain)


def test_coldkey_swap_read_preserves_false_policy_snapshot(v2_setup):
    _ctx, chain = v2_setup
    chain.seed_swap(committed_policy=False, stored_policy=False)

    announcement = asyncio.run(
        chain.client.read("coldkey_swap_announcement", coldkey_ss58=chain.controller)
    )

    assert announcement["import_source_policies"] is False
    assert announcement["new_coldkey_hash"] == _runtime_v2_commitment(chain.pure, False)


@pytest.mark.parametrize("mismatch", ["destination", "policy", "missing", "string", "integer"])
def test_v2_setup_refuses_unverifiable_commitment_without_reannouncing(v2_setup, mismatch):
    ctx, chain = v2_setup
    chain.fail_swap = True
    with pytest.raises(typer.Exit):
        _protect(ctx)
    assert pa.protected_setup_pending(_stored())
    destination = (
        Keypair.create_from_seed(bytes([142]) * 32, 1).ss58_address
        if mismatch == "destination"
        else chain.pure
    )
    policy = {
        "destination": False,
        "policy": True,
        "missing": None,
        "string": "false",
        "integer": 0,
    }[mismatch]
    chain.seed_swap(destination=destination, committed_policy=False, stored_policy=policy)
    submitted_before = list(chain.submitted)

    with pytest.raises(typer.Exit):
        _protect(ctx)

    assert chain.submitted == submitted_before
    assert chain.swap_fees_paid == 0
    assert pa.protected_setup_pending(_stored())
    assert ctx.output.errors


@pytest.mark.parametrize("explicit_snapshot", [False, True])
def test_coldkey_swap_read_pins_commitment_and_policy_to_one_block(
    v2_setup, monkeypatch, explicit_snapshot
):
    _ctx, chain = v2_setup
    chain.seed_swap(committed_policy=False, stored_policy=False)
    chain.storage.block = 100
    queries = []
    query = chain.storage.query

    async def advancing_head(module, item, params=None, block_hash=None):
        queries.append((item, block_hash))
        chain.storage.block += 1
        return await query(module, item, params, block_hash)

    monkeypatch.setattr(chain.storage, "query", advancing_head)

    async def read():
        view = await chain.client.at(50) if explicit_snapshot else chain.client
        return await view.read("coldkey_swap_announcement", coldkey_ss58=chain.controller)

    announcement = asyncio.run(read())

    assert announcement["import_source_policies"] is False
    assert {item for item, _hash in queries} == {
        "ColdkeySwapAnnouncements",
        "ColdkeySwapDisputes",
        "ColdkeySwapV2PolicySnapshots",
    }
    assert {block_hash for _item, block_hash in queries} == {
        f"0x{(50 if explicit_snapshot else 100):064x}",
    }


def test_coldkey_swap_read_supports_runtime_without_policy_snapshot_metadata(v2_setup):
    _ctx, chain = v2_setup
    storage = FakeSubstrate()
    legacy_hash = "0x" + blake2b(bytes(ss58_decode(chain.pure)), digest_size=32).hexdigest()
    storage.seed(
        "SubtensorModule", "ColdkeySwapAnnouncements", [chain.controller], (200, legacy_hash)
    )
    storage.missing_storage.add(("SubtensorModule", "ColdkeySwapV2PolicySnapshots"))
    client = Client("local", substrate=storage)

    announcement = asyncio.run(
        client.read("coldkey_swap_announcement", coldkey_ss58=chain.controller)
    )

    assert announcement["import_source_policies"] is None
    assert announcement["new_coldkey_hash"] == legacy_hash
    assert announcement["execute_block"] == 200


def test_policy_snapshot_rpc_failure_is_not_treated_as_absent_metadata(v2_setup, monkeypatch):
    _ctx, chain = v2_setup
    chain.seed_swap(committed_policy=False, stored_policy=False)
    query = chain.storage.query

    async def failed_snapshot(module, item, params=None, block_hash=None):
        if item == "ColdkeySwapV2PolicySnapshots":
            raise OSError("policy snapshot RPC unavailable")
        return await query(module, item, params, block_hash)

    monkeypatch.setattr(chain.storage, "query", failed_snapshot)

    with pytest.raises(OSError, match="policy snapshot RPC unavailable"):
        asyncio.run(chain.client.read("coldkey_swap_announcement", coldkey_ss58=chain.controller))
