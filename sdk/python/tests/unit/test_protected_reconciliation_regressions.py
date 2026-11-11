"""CLI reconciliation preserves saved payloads and the cancellation target shown to users."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest
import typer

from bittensor import config
from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import protected
from bittensor.client import Client
from bittensor.result import ExtrinsicResult
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT

CHAIN_A = "0x" + "aa" * 32
CHAIN_B = "0x" + "bb" * 32
HASH_A = "0x" + "11" * 32
HASH_B = "0x" + "22" * 32
HASH_C = "0x" + "33" * 32
SAVED_CALL = "0x010203040506"


def _record(genesis_hash, *, pure=BOB_HOT):
    record = {
        "controller_wallet": "main",
        "controller_ss58": ALICE,
        "protected_ss58": pure,
        "name": "main-protected",
        "delay_blocks": 10,
        "allows": [],
        "spawner": ALICE,
        "height": 50,
        "ext_index": 3,
        "index": 0,
    }
    if genesis_hash is not None:
        record["genesis_hash"] = genesis_hash
    return record


def _saved_payload(genesis_hash):
    row = {
        "index": 1,
        "controller_wallet": "main",
        "delegate_ss58": ALICE,
        "real_ss58": BOB_HOT,
        "call_hash": HASH_A,
        "inner_op": "transfer",
        "inner_args": {"dest_ss58": BOB, "amount_tao": 50},
        "category": "transfer",
        "delay_blocks": 10,
        "announced_at_block": 100,
        "executable_at_block": 110,
        "force_proxy_type": "Any",
        "status": "pending",
        "call_data": SAVED_CALL,
    }
    if genesis_hash is not None:
        row["genesis_hash"] = genesis_hash
    return row


class _Output:
    json_mode = True

    def __init__(self):
        self.values = []
        self.messages = []
        self.errors = []

    def value(self, value):
        self.values.append(value)

    def message(self, message):
        self.messages.append(message)

    def error(self, error, **kwargs):
        self.errors.append((error, kwargs))


class _Context:
    def __init__(self):
        self.wallet_name = "main"
        self.network = "chain-a"
        self.dry_run = False
        self.output = _Output()
        self.submissions = []
        self.confirmations = []
        self.genesis_reads = []
        self.chains = {
            "chain-a": {"genesis": CHAIN_A, "pure": BOB_HOT, "announcements": []},
            "chain-b": {"genesis": CHAIN_B, "pure": BOB, "announcements": []},
        }

    def run(self, callback):
        async def block_hash(block=0):
            assert block == 0
            self.genesis_reads.append(self.network)
            return self.chains[self.network]["genesis"]

        async def block():
            return 120

        async def read(operation, **kwargs):
            chain = self.chains[self.network]
            if operation == "proxies":
                grants = (
                    [{"delegate": ALICE, "proxy_type": "Any", "delay": 10}]
                    if kwargs["coldkey_ss58"] == chain["pure"]
                    else []
                )
                return {"proxies": grants}
            assert operation == "proxy_announcements"
            assert kwargs["delegate_ss58"] == ALICE
            return {"announcements": list(chain["announcements"])}

        client = SimpleNamespace(
            _substrate=SimpleNamespace(block_hash=block_hash), block=block, read=read
        )
        return asyncio.run(callback(client))

    def submit(self, intent, **kwargs):
        self.submissions.append((self.network, intent, kwargs))
        if intent.op == "remove_proxy_announcement":
            chain = self.chains[self.network]
            chain["announcements"] = [
                item
                for item in chain["announcements"]
                if (item["real"], item["call_hash"]) != (intent.real_ss58, intent.call_hash)
            ]
        else:
            assert intent.op == "execute_proxy_announced"
        return ExtrinsicResult(success=True, events=[])

    def resolve_address(self, _parameter, _value):
        return ALICE

    def resolve_account(self, _parameter, _value):
        return ALICE

    def confirm(self, question):
        self.confirmations.append(question)


@pytest.fixture
def reconciliation_context(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    return _Context()


def _on_chain(context, *hashes):
    chain = context.chains[context.network]
    chain["announcements"] = [
        {"real": chain["pure"], "call_hash": call_hash, "height": 100} for call_hash in hashes
    ]


def test_chain_binding_lookup_uses_sdk_substrate_block_zero(reconciliation_context):
    client = Client("local", substrate=FakeSubstrate())
    genesis_hash = "0x" + "00" * 32
    expected = _record(genesis_hash)
    pa.save_protected([_record(CHAIN_A, pure=BOB), expected])
    app_ctx = SimpleNamespace(
        wallet_name="main", network="local", run=lambda callback: asyncio.run(callback(client))
    )

    assert protected._get_protected_record(app_ctx) == expected
    assert protected._chain_genesis_hash(app_ctx) == genesis_hash


def test_show_on_another_network_preserves_saved_withdrawal_for_later_execution(
    reconciliation_context,
):
    app_ctx = reconciliation_context
    # Write the on-disk schema directly so the test also exercises older clients
    # that ignore the genesis fields instead of failing on a new helper argument.
    pa.save_protected([_record(CHAIN_A), _record(CHAIN_B, pure=BOB)])
    ann.save_announcements([_saved_payload(CHAIN_A)])
    _on_chain(app_ctx, HASH_A)
    protected.run_show(app_ctx, index=None)
    assert app_ctx.output.values[-1]["pending"][0]["call_data"] == SAVED_CALL

    app_ctx.network = "chain-b"
    protected.run_show(app_ctx, index=None)

    saved = ann.load_announcements()[0]
    assert saved["status"] == "pending"
    assert saved["call_data"] == SAVED_CALL
    assert app_ctx.output.values[-1]["protected"]["protected_ss58"] == BOB
    assert app_ctx.output.values[-1]["pending"] == []

    app_ctx.network = "chain-a"
    protected.run_execute_protected(app_ctx, index=saved["index"])

    assert len(app_ctx.submissions) == 1
    network, intent, options = app_ctx.submissions[0]
    assert network == "chain-a"
    assert intent.op == "execute_proxy_announced"
    assert intent.real_ss58 == BOB_HOT
    assert intent.delegate_ss58 == ALICE
    assert intent.announced_call_data == SAVED_CALL
    assert intent.inner_args == {"dest_ss58": BOB, "amount_tao": 50}
    assert options["proxy_for"] == "self"
    assert ann.load_announcements()[0]["status"] == "executed"
    assert app_ctx.genesis_reads == ["chain-a", "chain-b", "chain-a"]


# PR #3305: cancelling a displayed index must remove that commitment, not another withdrawal.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_cancel_displayed_unknown_index_keeps_its_hash_after_an_earlier_row_disappears(
    reconciliation_context,
):
    app_ctx = reconciliation_context
    pa.save_protected([_record(CHAIN_A)])
    _on_chain(app_ctx, HASH_A, HASH_B, HASH_C)
    protected.run_show(app_ctx, index=None)
    displayed = {row["call_hash"]: row["index"] for row in app_ctx.output.values[-1]["pending"]}

    # Another device removes A after this user reads the displayed index for B.
    _on_chain(app_ctx, HASH_B, HASH_C)
    protected.run_cancel(app_ctx, index=displayed[HASH_B], yes=True)

    assert len(app_ctx.submissions) == 1
    _network, cancellation, _options = app_ctx.submissions[0]
    assert cancellation.op == "remove_proxy_announcement"
    assert cancellation.call_hash == HASH_B
    remaining = app_ctx.chains["chain-a"]["announcements"]
    assert [item["call_hash"] for item in remaining] == [HASH_C]
    protected.run_show(app_ctx, index=None)
    assert [(row["call_hash"], row["index"]) for row in app_ctx.output.values[-1]["pending"]] == [
        (HASH_C, displayed[HASH_C])
    ]


def test_legacy_binding_requires_explicit_chain_verification_and_keeps_its_payload(
    reconciliation_context, monkeypatch
):
    app_ctx = reconciliation_context
    legacy_record = _record(None)
    legacy_payload = _saved_payload(None)
    pa.save_protected([legacy_record])
    ann.save_announcements([legacy_payload])
    _on_chain(app_ctx, HASH_A)

    with pytest.raises(typer.Exit) as error:
        protected.run_show(app_ctx, index=None)
    assert error.value.exit_code == 2
    assert "no network identity" in app_ctx.output.errors[-1][0]
    assert pa.load_protected() == [legacy_record]
    assert ann.load_announcements() == [legacy_payload]
    verified = []
    monkeypatch.setattr(
        protected,
        "_verify_pure_created_provenance",
        lambda context, book: verified.append((context.network, book)),
    )

    protected.run_protect(
        app_ctx, delay_raw="10", allow=[], disallow=[], allow_key=None, bind_network=True
    )

    assert len(verified) == 1
    assert verified[0][0] == "chain-a"
    assert verified[0][1]["address"] == BOB_HOT
    assert len(app_ctx.confirmations) == 1
    assert CHAIN_A in app_ctx.confirmations[0]
    assert pa.load_protected() == [{**legacy_record, "genesis_hash": CHAIN_A}]
    assert ann.load_announcements() == [{**legacy_payload, "genesis_hash": CHAIN_A}]
    protected.run_execute_protected(app_ctx, index=legacy_payload["index"])
    assert len(app_ctx.submissions) == 1
    assert app_ctx.submissions[0][1].announced_call_data == SAVED_CALL


@pytest.mark.parametrize("finalize", [False, True])
def test_rotation_to_bound_wallet_preserves_both_protected_accounts_and_queues(
    reconciliation_context, monkeypatch, finalize
):
    app_ctx = reconciliation_context
    source = _record(CHAIN_A)
    if finalize:
        source["pending_rotate"] = {"to_wallet": "other", "to_ss58": ALICE_HOT}
    destination = {
        **_record(CHAIN_A, pure=BOB),
        "controller_wallet": "other",
        "controller_ss58": ALICE_HOT,
    }
    payloads = [
        _saved_payload(CHAIN_A),
        {
            **_saved_payload(CHAIN_A),
            "index": 2,
            "controller_wallet": "other",
            "delegate_ss58": ALICE_HOT,
            "real_ss58": BOB,
            "call_hash": HASH_B,
        },
    ]
    pa.save_protected([source, destination])
    ann.save_announcements(payloads)
    bindings_before = pa.protected_path().read_bytes()
    queue_before = ann.announcements_path().read_bytes()
    monkeypatch.setattr(app_ctx, "resolve_address", lambda *_: ALICE_HOT)
    monkeypatch.setattr(app_ctx, "resolve_account", lambda *_: ALICE_HOT)
    monkeypatch.setattr(config, "get_multisig", lambda *_: None)
    announced = []
    monkeypatch.setattr(
        protected, "_announce_as_protected", lambda *args, **kwargs: announced.append(args)
    )
    verified = []

    def completed_rotation(_context, record, **kwargs):
        verified.append((record, kwargs))
        return True

    monkeypatch.setattr(protected, "_rotation_complete_on_chain", completed_rotation)

    with pytest.raises(typer.Exit) as error:
        if finalize:
            protected.run_rotate_finalize(app_ctx, to_wallet="other")
        else:
            protected.run_rotate(app_ctx, to_wallet="other")

    assert error.value.exit_code == 2
    assert app_ctx.output.errors
    assert announced == []
    assert app_ctx.submissions == []
    assert pa.protected_path().read_bytes() == bindings_before
    assert ann.announcements_path().read_bytes() == queue_before
    if finalize:
        assert len(verified) == 1
        assert verified[0][1] == {
            "new_controller_ss58": ALICE_HOT,
            "old_controller_ss58": ALICE,
        }
