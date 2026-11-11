"""Pending authority follows actual proxy and multisig commitments."""

from __future__ import annotations

import asyncio
from copy import deepcopy
from types import SimpleNamespace

import pytest
import typer

from bittensor import config
from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import protected
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT

GENESIS = "0x" + "e1" * 32
CALL_HASH = "0x" + "11" * 32
MULTISIG_HASH = "0x" + "22" * 32
OTHER_HASH = "0x" + "33" * 32


class _Chain:
    def __init__(self):
        self._substrate = self
        self.grants = [{"delegate": ALICE, "proxy_type": "Any", "delay": 10}]
        self.announcements = []
        self.pending_multisig = {}
        self.multisig_reads = []
        self.multisig_error = False
        self.history_error = False
        self.history_events = []
        self.extrinsics = []
        self.on_multisig_read = None

    async def block_hash(self, block=0):
        return GENESIS if block == 0 else "0x" + "a1" * 32

    async def block(self):
        return 120

    async def events(self, _hash):
        if self.history_error:
            raise RuntimeError("history pruned")
        return deepcopy(self.history_events)

    async def block_info(self, _height):
        return SimpleNamespace(extrinsics=deepcopy(self.extrinsics))

    async def read(self, operation, **kwargs):
        if operation == "proxies":
            return {"proxies": deepcopy(self.grants)}
        if operation == "proxy_announcements":
            return {"announcements": deepcopy(self.announcements)}
        assert operation == "multisig"
        self.multisig_reads.append(kwargs)
        if self.multisig_error:
            raise RuntimeError("RPC unavailable")
        if self.on_multisig_read:
            self.on_multisig_read()
        return deepcopy(self.pending_multisig.get(kwargs["call_hash"]))


@pytest.fixture
def lifecycle(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    monkeypatch.setenv("BTCLI_MULTISIG_CACHE", str(tmp_path / "multisig.json"))
    monkeypatch.setattr(config, "get_multisig", lambda _name: None)
    chain = _Chain()
    messages, errors = [], []
    addresses = {"old": ALICE, "first": BOB, "latest": ALICE_HOT}
    ctx = SimpleNamespace(
        wallet_name="old",
        network="test",
        dry_run=False,
        multisig_wallet_name=None,
        output=SimpleNamespace(
            message=messages.append, error=lambda text, **_kw: errors.append(text)
        ),
        run=lambda callback: asyncio.run(callback(chain)),
        resolve_address=lambda _param, value: addresses[value or "old"],
        resolve_account=lambda _param, value: addresses[value or "old"],
    )
    record = {
        "controller_wallet": "old",
        "controller_ss58": ALICE,
        "protected_ss58": BOB_HOT,
        "delay_blocks": 10,
        "genesis_hash": GENESIS,
        "allows": [],
        "pending_allows": ["Staking"],
    }
    pa.upsert_protected(record)
    return ctx, chain, record, messages, errors


def _row(*, awaiting=False, known_identity=True, inner_op="add_proxy", inner_args=None):
    row = ann.add_announcement(
        controller_wallet="old",
        delegate_ss58=ALICE,
        real_ss58=BOB_HOT,
        call_hash=CALL_HASH,
        inner_op=inner_op,
        inner_args=inner_args or {"delegate_ss58": ALICE, "proxy_type": "Staking", "delay": 0},
        category="allow:Staking" if inner_op == "add_proxy" else "batch",
        delay_blocks=10,
        announced_at_block=100,
        call_data="0x123456",
        genesis_hash=GENESIS,
    )
    updates = {}
    if awaiting:
        updates["status"] = "awaiting_approval"
    if known_identity:
        updates["multisig_announcement"] = {"account_ss58": ALICE, "call_hash": MULTISIG_HASH}
    return ann.update_announcement("old", row["index"], updates, genesis_hash=GENESIS)


def _event(*, call_hash=MULTISIG_HASH, approval=False, tuple_attributes=False):
    attributes = {"approving": BOB, "multisig": ALICE, "call_hash": call_hash}
    if approval:
        attributes = {
            "approving": BOB,
            "timepoint": {"height": 90, "index": 1},
            "multisig": ALICE,
            "call_hash": call_hash,
        }
    return {
        "extrinsic_idx": 0,
        "event": {
            "module_id": "Multisig",
            "event_id": "MultisigApproval" if approval else "NewMultisig",
            "attributes": list(attributes.values()) if tuple_attributes else attributes,
        },
    }


def _stored():
    return pa.get_by_controller("old", genesis_hash=GENESIS)


def _saved(row):
    return ann.get_by_index("old", row["index"], genesis_hash=GENESIS)


@pytest.mark.parametrize("approval,tuple_attributes", [(False, False), (True, True)])
def test_approval_receipt_records_underlying_multisig_hash(lifecycle, approval, tuple_attributes):
    ctx, _chain, _record, _messages, _errors = lifecycle
    row = _row(known_identity=False)
    result = SimpleNamespace(
        events=[_event(approval=approval, tuple_attributes=tuple_attributes)], data={}
    )

    updated = protected._mark_announcement_from_submit(ctx, row, result)

    assert updated["status"] == "awaiting_approval"
    assert updated["multisig_announcement"]["call_hash"] == MULTISIG_HASH
    assert _saved(row)["multisig_announcement"]["account_ss58"] == ALICE
    if approval:
        assert updated["multisig_announcement"]["timepoint"] == {"height": 90, "index": 1}


def test_abandoned_multisig_releases_pending_grant_and_keeps_plaintext(lifecycle):
    ctx, chain, record, _messages, _errors = lifecycle
    row = _row(awaiting=True)

    assert protected._reconcile_pending(ctx, record) == []

    assert chain.multisig_reads == [{"account_ss58": ALICE, "call_hash": MULTISIG_HASH}]
    assert _saved(row)["status"] == "gone"
    assert _saved(row)["call_data"] == "0x123456"
    assert record["pending_allows"] == []
    assert _stored()["pending_allows"] == []


def test_cancel_clears_already_abandoned_multisig_without_submitting(lifecycle):
    ctx, _chain, _record, _messages, _errors = lifecycle
    row = _row(awaiting=True)
    ctx.submit = lambda *_a, **_kw: pytest.fail("nothing remains on chain to cancel")

    assert protected._cancel_one(ctx, row)
    assert _saved(row)["status"] == "gone"
    assert _stored()["pending_allows"] == []


@pytest.mark.parametrize("failure", ["live", "rpc", "pruned", "unrelated-event"])
def test_uncertain_or_live_multisig_keeps_pending_grant(lifecycle, failure):
    ctx, chain, record, _messages, _errors = lifecycle
    row = _row(awaiting=True, known_identity=failure in ("live", "rpc"))
    if failure == "live":
        chain.pending_multisig[MULTISIG_HASH] = {"timepoint": {"height": 100, "index": 0}}
    elif failure == "rpc":
        chain.multisig_error = True
    elif failure == "pruned":
        chain.history_error = True
    else:
        chain.history_events = [_event()]
        config.save_multisig_cache_entry(
            MULTISIG_HASH,
            {
                "target": "Proxy.announce",
                "params": {"real": BOB_HOT, "call_hash": OTHER_HASH},
            },
        )

    pending = protected._reconcile_pending(ctx, record)

    assert [item["index"] for item in pending] == [row["index"]]
    assert pending[0]["approval_state"] == ("pending" if failure == "live" else "unknown")
    assert _saved(row)["status"] == "awaiting_approval"
    assert _stored()["pending_allows"] == ["Staking"]


@pytest.mark.parametrize("source", ["cache", "opening-extrinsic"])
def test_legacy_cancelled_approval_recovers_exact_hash_from_inclusion_block(lifecycle, source):
    ctx, chain, record, _messages, _errors = lifecycle
    row = _row(awaiting=True, known_identity=False)
    chain.history_events = [_event()]
    if source == "cache":
        config.save_multisig_cache_entry(
            MULTISIG_HASH,
            {
                "target": "Proxy.announce",
                "params": {"real": {"Id": BOB_HOT}, "call_hash": CALL_HASH},
            },
        )
    else:
        chain.extrinsics = [
            {
                "call": {
                    "call_module": "Multisig",
                    "call_function": "as_multi",
                    "call_args": [
                        {
                            "name": "call",
                            "value": {
                                "call_module": "Proxy",
                                "call_function": "announce",
                                "call_args": [
                                    {"name": "real", "value": {"Id": BOB_HOT}},
                                    {"name": "call_hash", "value": CALL_HASH},
                                ],
                            },
                        }
                    ],
                }
            }
        ]

    assert protected._reconcile_pending(ctx, record) == []
    assert _saved(row)["status"] == "gone"
    assert _saved(row)["multisig_announcement"]["call_hash"] == MULTISIG_HASH
    assert chain.multisig_reads == [{"account_ss58": ALICE, "call_hash": MULTISIG_HASH}]


def test_threshold_reached_during_reconciliation_does_not_drop_announced_call(lifecycle):
    ctx, chain, record, _messages, _errors = lifecycle
    row = _row(awaiting=True)
    chain.on_multisig_read = lambda: chain.announcements.append(
        {
            "real": BOB_HOT,
            "call_hash": CALL_HASH,
            "height": 120,
        }
    )

    assert protected._reconcile_pending(ctx, record)[0]["index"] == row["index"]
    assert _saved(row)["status"] == "awaiting_approval"
    assert _stored()["pending_allows"] == ["Staking"]
    assert protected._reconcile_pending(ctx, record)[0]["on_chain"]
    assert _saved(row)["status"] == "pending"


def test_remote_executed_allow_no_longer_blocks_rotation_and_is_migrated(lifecycle, monkeypatch):
    ctx, chain, _record, _messages, _errors = lifecycle
    row = _row()
    chain.grants.append({"delegate": ALICE, "proxy_type": "Staking", "delay": 0})
    queued = []
    monkeypatch.setattr(
        protected,
        "_announce_as_protected",
        lambda _ctx, record, intent, **kw: queued.append((record, intent, kw)),
    )

    protected.run_rotate(ctx, to_wallet="first")

    assert _saved(row)["status"] == "gone"
    assert _stored()["pending_allows"] == []
    assert len(queued) == 1
    record, intent, kwargs = queued[0]
    assert record["allows"] == ["Staking"]
    assert kwargs["row_extra"]["rotate_migrated_allows"] == ["Staking"]
    assert any(
        child.get("delegate_ss58") == BOB and child.get("proxy_type") == "Staking"
        for child in intent.intents
    )


def test_nested_pending_grant_still_blocks_rotation(lifecycle):
    ctx, chain, _record, _messages, errors = lifecycle
    row = _row(
        inner_op="batch",
        inner_args={
            "intents": [
                {
                    "op": "batch",
                    "intents": [
                        {
                            "op": "add_proxy",
                            "delegate_ss58": ALICE,
                            "proxy_type": "Staking",
                            "delay": 0,
                        }
                    ],
                }
            ]
        },
    )
    chain.announcements = [{"real": BOB_HOT, "call_hash": CALL_HASH, "height": 100}]

    with pytest.raises(typer.Exit):
        protected.run_rotate(ctx, to_wallet="first")

    assert "allow grants are still pending" in errors[-1]
    assert _saved(row)["status"] == "pending"


@pytest.mark.parametrize("live_destination,success", [(BOB, True), (ALICE_HOT, False)])
def test_finalize_respects_explicit_destination_over_latest_pointer(
    lifecycle, live_destination, success
):
    ctx, chain, record, _messages, _errors = lifecycle
    pa.upsert_protected({**record, "pending_rotate": {"to_wallet": "latest", "to_ss58": ALICE_HOT}})
    chain.grants = [{"delegate": live_destination, "proxy_type": "Any", "delay": 10}]

    if success:
        protected.run_rotate_finalize(ctx, to_wallet="first")
        assert _stored() is None
        assert pa.get_by_controller("first", genesis_hash=GENESIS)["controller_ss58"] == BOB
    else:
        before = pa.protected_path().read_bytes()
        with pytest.raises(typer.Exit):
            protected.run_rotate_finalize(ctx, to_wallet="first")
        assert pa.protected_path().read_bytes() == before
    assert pa.get_by_controller("latest", genesis_hash=GENESIS) is None


def test_finalize_refuses_surviving_old_controller_grant(lifecycle):
    ctx, chain, _record, _messages, _errors = lifecycle
    chain.grants = [
        {"delegate": BOB, "proxy_type": "Any", "delay": 10},
        {"delegate": ALICE, "proxy_type": "Staking", "delay": 0},
    ]
    before = pa.protected_path().read_bytes()
    with pytest.raises(typer.Exit):
        protected.run_rotate_finalize(ctx, to_wallet="first")
    assert pa.protected_path().read_bytes() == before


def test_dry_reconcile_projects_changes_without_writing_stores(lifecycle):
    ctx, chain, record, _messages, _errors = lifecycle
    ctx.dry_run = True
    _row(awaiting=True)
    chain.grants[0]["delay"] = 20
    chain.announcements = [{"real": BOB_HOT, "call_hash": OTHER_HASH, "height": 110}]
    before = (pa.protected_path().read_bytes(), ann.announcements_path().read_bytes())

    pending = protected._reconcile_pending(ctx, record)

    assert [row["call_hash"] for row in pending] == [OTHER_HASH]
    assert pending[0]["index"] is None
    assert pending[0]["provisional"]
    assert pending[0]["executable_at_block"] == 130
    assert (pa.protected_path().read_bytes(), ann.announcements_path().read_bytes()) == before


@pytest.mark.parametrize("unknown", [False, True])
@pytest.mark.parametrize("json_mode", [False, True])
def test_show_does_not_call_unannounced_multisig_ready(lifecycle, unknown, json_mode):
    ctx, chain, _record, messages, _errors = lifecycle
    _row(awaiting=True)
    chain.multisig_error = unknown
    chain.pending_multisig[MULTISIG_HASH] = {"timepoint": {"height": 100, "index": 0}}
    displayed = []
    ctx.output.json_mode = json_mode
    ctx.output.value = displayed.append
    ctx.output.columns = lambda _title, _headers, rows, _raw: displayed.extend(rows)

    protected.run_show(ctx, index=None)

    expected = "approval_unknown" if unknown else "awaiting_approval"
    if json_mode:
        assert displayed[0]["pending"][0]["status"] == expected
        assert displayed[0]["pending"][0]["countdown"] == "not announced"
    else:
        assert displayed[0][2:] == ["after approval", "not announced", expected]
    if unknown:
        assert any("archive RPC" in message for message in messages)


def test_cancel_unknown_approval_reports_uncertainty_and_preserves_store(lifecycle):
    ctx, chain, _record, _messages, errors = lifecycle
    row = _row(awaiting=True)
    chain.multisig_error = True
    before = (pa.protected_path().read_bytes(), ann.announcements_path().read_bytes())

    assert not protected._cancel_one(ctx, row)

    assert "cannot verify" in errors[-1]
    assert "archive RPC" in errors[-1]
    assert (pa.protected_path().read_bytes(), ann.announcements_path().read_bytes()) == before


def test_dry_finalize_verifies_explicit_destination_without_moving_binding(lifecycle):
    ctx, chain, _record, messages, _errors = lifecycle
    ctx.dry_run = True
    chain.grants = [{"delegate": BOB, "proxy_type": "Any", "delay": 10}]
    before = pa.protected_path().read_bytes()

    protected.run_rotate_finalize(ctx, to_wallet="first")

    assert pa.protected_path().read_bytes() == before
    assert any("would move" in message for message in messages)


# PR #3305: a preview must not advertise an ephemeral index that later cancels another withdrawal.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("json_mode", [False, True])
def test_dry_discovery_never_advertises_reusable_numeric_cancellation_indexes(lifecycle, json_mode):
    ctx, chain, _record, messages, _errors = lifecycle
    ctx.dry_run = True
    chain.announcements = [
        {"real": BOB_HOT, "call_hash": call_hash, "height": 100}
        for call_hash in (CALL_HASH, MULTISIG_HASH, OTHER_HASH)
    ]
    displayed = []
    ctx.output.json_mode = json_mode
    ctx.output.value = displayed.append
    ctx.output.columns = lambda _title, _headers, rows, _raw: displayed.extend(rows)

    protected.run_show(ctx, index=None)

    if json_mode:
        assert [row["index"] for row in displayed[0]["pending"]] == [None, None, None]
    else:
        assert [row[0] for row in displayed] == ["unsaved", "unsaved", "unsaved"]
    assert ann.load_announcements() == []
    assert any("without `--dry-run`" in message for message in messages)

    # A disappears before the first real read. Only that read assigns stable
    # indexes; the preview offered no numeric ID for B that can now identify C.
    chain.announcements = chain.announcements[1:]
    ctx.dry_run = False
    rows = protected._reconcile_pending(ctx, _stored())
    assert [(row["call_hash"], row["index"]) for row in rows] == [
        (MULTISIG_HASH, 1),
        (OTHER_HASH, 2),
    ]
    assert (
        ann.get_by_index("old", rows[0]["index"], genesis_hash=GENESIS)["call_hash"]
        == MULTISIG_HASH
    )


def test_execute_dry_run_handles_unsaved_not_ready_discovery(lifecycle):
    """Auditor LOW: dry-run discoveries use index=None; execute must not TypeError."""
    ctx, chain, _record, messages, _errors = lifecycle
    ctx.dry_run = True
    # Not ready yet: height 115 + delay 10 → executable_at 125 > block 120.
    chain.announcements = [{"real": BOB_HOT, "call_hash": OTHER_HASH, "height": 115}]

    protected.run_execute_protected(ctx, index=None)

    assert any("unsaved" in message and "not ready" in message for message in messages)
    assert ann.load_announcements() == []
