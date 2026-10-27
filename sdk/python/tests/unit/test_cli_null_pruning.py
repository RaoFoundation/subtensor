"""Owner pruning must advance on-chain, not merely receive successful receipts."""

import asyncio
from dataclasses import replace
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest
import typer

from bittensor.cli import null_pruning as pruning
from bittensor.result import ChainError, ExtrinsicResult

READ_STATE = pruning.read_state
SWITCH_WAIT_REASON = pruning.switch_wait_reason


def state(**changes):
    initial = pruning.PruningState(
        block=100,
        genesis="0xgenesis",
        owner="owner",
        population=384,
        pending_target=None,
        mechanisms=1,
        minimum=64,
        maximum=2500,
        mode="Null",
        frozen=False,
    )
    return replace(initial, **changes)


class App:
    def __init__(self, initial, responses=()):
        self.state = initial
        self.responses = iter(responses)
        self.calls = []
        self.output = Mock()
        self.dry_run = False
        self.wait_reason = None

    def run(self, work):
        return asyncio.run(work(self))

    def resolve_address(self, *_args):
        return "owner"

    def resolve_dispatch_proxy(self):
        return None

    def submit(self, intent, **kwargs):
        assert kwargs["wait_for_finalization"] is True
        self.calls.append(intent)
        if self.dry_run:
            return None
        kwargs["before_submit"]()
        response = next(self.responses)
        if isinstance(response, BaseException):
            raise response
        after, result = response
        self.state = after
        kwargs["on_result"](result)
        if not result.success:
            raise typer.Exit(1)
        return result


def receipt(after, *, removed=64, cleared=0, target=256, success=True, error=None):
    return after, ExtrinsicResult(
        success=success,
        block_hash="0xfinalized",
        extrinsic_id="101-0001",
        error=error,
        events=[
            {
                "event": {
                    "module_id": "SubtensorModule",
                    "event_id": "NullUidsPruningProgress",
                    "attributes": {
                        "netuid": 1,
                        "target": target,
                        "remaining": after.population,
                        "removed": removed,
                        "cleared_commits": cleared,
                    },
                }
            }
        ],
    )


@pytest.fixture(autouse=True)
def reads(monkeypatch):
    async def read(app, _netuid):
        return app.state

    async def wait_reason(app, _netuid):
        return app.wait_reason

    monkeypatch.setattr(pruning, "read_state", read)
    monkeypatch.setattr(pruning, "switch_wait_reason", wait_reason)


def run(app, tmp_path, **kwargs):
    options = dict(
        netuid=1,
        target=256,
        switch_to_yuma=False,
        wait_timeout=30,
        poll_seconds=1,
        max_batches=256,
        state_file=tmp_path / "progress.json",
    )
    options.update(kwargs)
    pruning.run_pruning(app, **options)


def test_continuation_and_cleanup_only_batches(tmp_path):
    cleanup = state(pending_target=256)
    first = state(population=320, pending_target=256)
    last = state(population=256)
    app = App(state(), [receipt(cleanup, removed=0, cleared=2500), receipt(first), receipt(last)])
    run(app, tmp_path)
    assert [intent.target for intent in app.calls] == [256, 256, 256]
    assert not (tmp_path / "progress.json").exists()
    assert app.output.detail.call_args.args[1]["consensus"] == "Null"
    assert app.output.detail.call_args.args[1]["batches"] == 3


def test_default_target_uses_shared_yuma_ceiling(tmp_path):
    app = App(
        state(population=192, mechanisms=2),
        [receipt(state(population=128, mechanisms=2), target=128)],
    )
    run(app, tmp_path, target=None)
    assert app.calls[0].target == 128


def test_restart_resumes_pending_target_without_local_progress(tmp_path):
    app = App(state(population=320, pending_target=256), [receipt(state(population=256))])
    run(app, tmp_path, target=None)
    assert len(app.calls) == 1
    assert app.calls[0].target == 256


@pytest.mark.parametrize(
    "initial,target,message",
    [
        (state(pending_target=128), 256, "conflicting"),
        (state(mechanisms=2), 256, "target must"),
        (state(minimum=128), 64, "target must"),
        (state(owner="other"), 256, "owner coldkey"),
    ],
)
def test_invalid_requests_never_submit(tmp_path, initial, target, message):
    app = App(initial)
    with pytest.raises(ValueError, match=message):
        run(app, tmp_path, target=target)
    assert app.calls == []


def test_waits_for_admin_window_without_submitting_failures(tmp_path, monkeypatch):
    app = App(
        state(frozen=True),
        [receipt(state(population=320, pending_target=256)), receipt(state(population=256))],
    )
    sleeps = []

    def sleep(seconds):
        sleeps.append(seconds)
        app.state = replace(app.state, frozen=False)

    monkeypatch.setattr(pruning.time, "sleep", sleep)
    run(app, tmp_path)
    assert sleeps == [1]
    assert len(app.calls) == 2


def test_admin_wait_timeout_and_interrupt_are_safe(tmp_path, monkeypatch):
    app = App(state(frozen=True))
    clock = iter([0, 0, 31])
    monkeypatch.setattr(
        pruning,
        "time",
        SimpleNamespace(
            monotonic=lambda: next(clock),
            sleep=lambda _seconds: None,
        ),
    )
    with pytest.raises(ValueError, match="wait timeout"):
        run(app, tmp_path)
    assert not app.calls
    assert not (tmp_path / "progress.json").exists()


def test_interrupt_while_waiting_can_resume(tmp_path, monkeypatch):
    app = App(state(frozen=True))
    monkeypatch.setattr(pruning.time, "sleep", Mock(side_effect=KeyboardInterrupt))
    with pytest.raises(KeyboardInterrupt):
        run(app, tmp_path)
    app.state = state(population=320, pending_target=256)
    app.responses = iter([receipt(state(population=256))])
    run(app, tmp_path)
    assert len(app.calls) == 1


def test_interrupt_during_submission_blocks_uncertain_resubmission(tmp_path):
    app = App(state(), [KeyboardInterrupt()])
    with pytest.raises(KeyboardInterrupt):
        run(app, tmp_path)
    assert (tmp_path / "progress.json").exists()
    with pytest.raises(ValueError, match="unresolved submission"):
        run(app, tmp_path)
    assert len(app.calls) == 1


def test_uncertain_receipt_does_not_retry(tmp_path):
    app = App(state(), [(state(), ExtrinsicResult(success=True, message="Submitted"))])
    with pytest.raises(ValueError, match="uncertain"):
        run(app, tmp_path)
    assert (tmp_path / "progress.json").exists()
    assert len(app.calls) == 1


def test_permanent_failure_stops_and_cooldown_is_not_repeated(tmp_path):
    for name in ["TrimmingWouldExceedMaxImmunePercentage", "TxRateLimitExceeded", "BadOrigin"]:
        app = App(state(), [receipt(state(), success=False, error=ChainError("rejected", name))])
        with pytest.raises((typer.Exit, ValueError)):
            run(app, tmp_path)
        assert len(app.calls) == 1
        assert not (tmp_path / "progress.json").exists()


def test_window_race_waits_then_rechecks_state(tmp_path, monkeypatch):
    app = App(
        state(population=320),
        [
            receipt(
                state(population=320),
                success=False,
                error=ChainError("window", "AdminActionProhibitedDuringWeightsWindow"),
            ),
            receipt(state(population=256)),
        ],
    )
    sleeps = Mock()
    monkeypatch.setattr(pruning.time, "sleep", sleeps)
    run(app, tmp_path)
    sleeps.assert_called_once_with(1)


def test_repeated_no_progress_stops(tmp_path):
    unchanged = state(pending_target=256)
    app = App(state(), [receipt(unchanged, removed=0)] * 3)
    with pytest.raises(ValueError, match="no progress"):
        run(app, tmp_path)
    assert len(app.calls) == 3


def test_lost_ownership_and_mechanism_changes_stop(tmp_path):
    for changed in [state(population=320, owner="other"), state(population=320, mechanisms=2)]:
        app = App(state(), [receipt(changed)])
        with pytest.raises(ValueError, match="changed"):
            run(app, tmp_path)
        assert len(app.calls) == 1


def test_explicit_switch_waits_for_cooldown_and_verifies_mode(tmp_path, monkeypatch):
    app = App(
        state(population=256),
        [
            (
                state(population=256, mode="Yuma"),
                ExtrinsicResult(
                    success=True,
                    block_hash="0xfinalized",
                    extrinsic_id="102-0001",
                ),
            )
        ],
    )
    app.wait_reason = "consensus-switch cooldown has not elapsed"

    def sleep(_seconds):
        app.wait_reason = None

    monkeypatch.setattr(pruning.time, "sleep", sleep)
    run(app, tmp_path, switch_to_yuma=True)
    assert len(app.calls) == 1
    assert app.calls[0].name == "epoch_consensus"
    assert app.calls[0].value == "Yuma"


def test_dry_run_previews_one_batch_without_creating_broadcast_record(tmp_path):
    app = App(state(frozen=True))
    app.dry_run = True
    run(app, tmp_path)
    assert len(app.calls) == 1
    assert not (tmp_path / "progress.json").exists()


def test_batch_limit_allows_restart(tmp_path):
    app = App(state(), [receipt(state(population=320, pending_target=256))])
    with pytest.raises(ValueError, match="batch limit"):
        run(app, tmp_path, max_batches=1)
    app.responses = iter([receipt(state(population=256))])
    run(app, tmp_path)
    assert len(app.calls) == 2


def test_key_probe_never_loads_commit_values():
    from bittensor._transport.interface import SubstrateConnection

    connection = SubstrateConnection("ws://unused")
    entry = SimpleNamespace(param_types=["u16", "u64"])
    codec = Mock()
    codec.storage_entry.return_value = entry
    codec.storage_key.return_value = b"prefix"
    connection._runtimes = SimpleNamespace(codec_at=AsyncMock(return_value=codec))
    connection._session = SimpleNamespace(request=AsyncMock(return_value=["0xkey"]))
    assert asyncio.run(
        connection.map_has_entries("SubtensorModule", "TimelockedWeightCommits", [1], "0xblock")
    )
    connection._session.request.assert_awaited_once_with(
        "state_getKeysPaged",
        ["0x707265666978", 1, "0x707265666978", "0xblock"],
    )


@pytest.mark.parametrize(
    "tempo,last,pending,window,frozen",
    [
        (100, 50, 0, 20, False),
        (100, 0, 0, 20, True),
        (100, 50, 110, 20, True),
        (0, 0, 110, 20, False),
        (20, 100, 0, 20, False),
    ],
)
def test_state_reads_are_pinned_and_match_admin_window(tempo, last, pending, window, frozen):
    values = {
        "NetworksAdded": True,
        "SubnetOwner": "owner",
        "SubnetworkN": 320,
        "NullPruningTarget": 128,
        "MechanismCountCurrent": 2,
        "MinAllowedUids": 64,
        "MaxAllowedUids": 1250,
        "SubnetEpochConsensus": {"Null": None},
        "Tempo": tempo,
        "LastEpochBlock": last,
        "PendingEpochAt": pending,
        "AdminFreezeWindow": window,
        "BlockHash": "0xgenesis",
    }

    async def query(item, _params=None):
        return values[item.name]

    view = SimpleNamespace(block=100, query=AsyncMock(side_effect=query))
    client = SimpleNamespace(at=AsyncMock(return_value=view))
    actual = asyncio.run(READ_STATE(client, 1))
    assert actual.frozen is frozen
    assert actual.yuma_ceiling == 128
    assert actual.pending_target == 128
    assert actual.mode == "Null"
    client.at.assert_awaited_once()


def test_switch_probes_all_legacy_indices_and_owner_cooldown():
    values = {"LastRateLimitedBlock": 90, "Tempo": 20, "OwnerHyperparamRateLimit": 1}

    async def query(item, _params=None):
        return values[item.name]

    substrate = SimpleNamespace(
        block_hash=AsyncMock(return_value="0xblock"),
        map_has_entries=AsyncMock(return_value=False),
    )
    client = SimpleNamespace(
        _substrate=substrate,
        block=AsyncMock(return_value=100),
        at=AsyncMock(return_value=SimpleNamespace(block=100, query=AsyncMock(side_effect=query))),
    )
    assert asyncio.run(SWITCH_WAIT_REASON(client, 1)) == "consensus-switch cooldown has not elapsed"
    assert substrate.map_has_entries.await_count == 16
    assert substrate.map_has_entries.await_args_list[-1].args[2] == [15 * 4096 + 1]
    substrate.map_has_entries = AsyncMock(return_value=True)
    assert "pending timelocked" in asyncio.run(SWITCH_WAIT_REASON(client, 1))
    assert substrate.map_has_entries.await_count == 1


def test_cleanup_before_optional_switch(tmp_path, monkeypatch):
    app = App(
        state(population=256),
        [
            receipt(state(population=256), removed=0, cleared=20),
            (
                state(population=256, mode="Yuma"),
                ExtrinsicResult(
                    success=True,
                    block_hash="0xfinalized",
                    extrinsic_id="102-0001",
                ),
            ),
        ],
    )

    async def pending_once(app, _netuid):
        return "pending timelocked commitments" if not app.calls else None

    monkeypatch.setattr(pruning, "switch_wait_reason", pending_once)
    run(app, tmp_path, switch_to_yuma=True)
    assert [intent.op for intent in app.calls] == ["trim_null_subnet_batch", "set_hyperparameter"]


def test_incomplete_approval_does_not_repeat_transaction(tmp_path):
    app = App(
        state(),
        [
            (
                state(),
                ExtrinsicResult(
                    success=True,
                    block_hash="0xfinalized",
                    extrinsic_id="101-0001",
                    events=[],
                ),
            )
        ],
    )
    with pytest.raises(ValueError, match="no pruning progress event"):
        run(app, tmp_path)
    assert len(app.calls) == 1


def test_already_yuma_is_idempotent(tmp_path):
    app = App(state(population=128, maximum=128, mode="Yuma"))
    run(app, tmp_path, target=None, switch_to_yuma=True)
    assert not app.calls


def test_switch_queue_probe_works_through_client_backend():
    from bittensor.client import Client
    from tests.harness.fake_substrate import FakeSubstrate

    backend = FakeSubstrate()
    backend.seed("SubtensorModule", "TimelockedWeightCommits", [4097, 3], [])
    client = Client("local", substrate=backend)
    assert "pending timelocked" in asyncio.run(SWITCH_WAIT_REASON(client, 1))
