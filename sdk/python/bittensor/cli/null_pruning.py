"""Sequential, restartable owner pruning using the normal CLI transaction flow."""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

import typer

from .. import config
from .._generated import storage as st
from ..executor import _find_event
from ..intents import SetHyperparameter, TrimNullSubnetBatch


@dataclass(frozen=True)
class PruningState:
    block: int
    genesis: str
    owner: str
    population: int
    pending_target: int | None
    mechanisms: int
    minimum: int
    maximum: int
    mode: str
    frozen: bool

    @property
    def yuma_ceiling(self) -> int:
        return 256 // self.mechanisms


def mode_name(value) -> str:
    if isinstance(value, dict):
        value = next(iter(value))
    return {0: "Yuma", 1: "Null"}.get(value, str(value))


async def read_state(client, netuid: int) -> PruningState:
    view = await client.at()
    names = (
        "NetworksAdded",
        "SubnetOwner",
        "SubnetworkN",
        "NullPruningTarget",
        "MechanismCountCurrent",
        "MinAllowedUids",
        "MaxAllowedUids",
        "SubnetEpochConsensus",
        "Tempo",
        "LastEpochBlock",
        "PendingEpochAt",
    )
    values = await asyncio.gather(
        *(view.query(getattr(st.SubtensorModule, name), [netuid]) for name in names),
        view.query(st.SubtensorModule.AdminFreezeWindow),
        view.query(st.System.BlockHash, [0]),
    )
    (
        exists,
        owner,
        n,
        target,
        count,
        minimum,
        maximum,
        mode,
        tempo,
        last,
        pending,
        window,
        genesis,
    ) = values
    if not exists:
        raise ValueError(f"subnet {netuid} does not exist")
    if not 1 <= int(count) <= 16:
        raise ValueError("invalid mechanism count; cannot derive the Yuma ceiling")
    frozen = bool(tempo) and (
        int(pending or 0) > view.block or max(0, int(last) + int(tempo) - view.block) < int(window)
    )
    return PruningState(
        view.block,
        str(genesis),
        str(owner),
        int(n),
        None if target is None else int(target),
        int(count),
        int(minimum),
        int(maximum),
        mode_name(mode),
        frozen,
    )


async def switch_wait_reason(client, netuid: int) -> str | None:
    """Bound the queue probe to one map entry per mechanism, including legacy indices."""
    block_hash = await client._substrate.block_hash(await client.block())
    for mechanism in range(16):
        index = mechanism * 4096 + netuid
        has_entries = await client._substrate.map_has_entries(
            "SubtensorModule",
            "TimelockedWeightCommits",
            [index],
            block_hash,
        )
        if has_entries:
            return "pending timelocked commitments must drain before switching"
    view = await client.at()
    last, tempo, epochs = await asyncio.gather(
        view.query(
            st.SubtensorModule.LastRateLimitedBlock,
            [{"OwnerHyperparamUpdate": [netuid, "EpochConsensus"]}],
        ),
        view.query(st.SubtensorModule.Tempo, [netuid]),
        view.query(st.SubtensorModule.OwnerHyperparamRateLimit),
    )
    if last and view.block - int(last) < int(tempo) * int(epochs):
        return "consensus-switch cooldown has not elapsed"
    return None


class SubmissionJournal:
    """An unresolved broadcast must survive Ctrl-C, process death and restarts."""

    def __init__(self, path: Path):
        self.path = path

    @contextmanager
    def locked(self):
        # Advisory OS locks are released on process death, unlike a PID lockfile.
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.path.with_suffix(".lock").open("a+b") as lock:
            if os.name == "nt":
                import msvcrt

                lock.write(b"0")
                lock.flush()
                lock.seek(0)
                try:
                    msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
                except OSError as error:
                    raise ValueError("another pruning workflow is running") from error
            else:
                import fcntl

                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except OSError as error:
                    raise ValueError("another pruning workflow is running") from error
            yield

    def check(self):
        if self.path.exists():
            raise ValueError(
                f"unresolved submission recorded in {self.path}; do not resubmit. "
                "Reconcile its inclusion or expiry on-chain before removing that record"
            )

    def begin(self, state: PruningState, target: int, operation: str, netuid: int):
        self.check()
        payload = {
            "netuid": netuid,
            "genesis": state.genesis,
            "owner": state.owner,
            "block": state.block,
            "target": target,
            "population": state.population,
            "operation": operation,
        }
        # Exclusive creation prevents an overlapping invocation from broadcasting.
        with self.path.open("x") as file:
            json.dump(payload, file)
            file.flush()
            os.fsync(file.fileno())

    def finish(self, result):
        # submit waits for finalization. A pool/transport failure without an
        # including block does not establish that delivery failed.
        if result.block_hash and result.extrinsic_id:
            self.path.unlink(missing_ok=True)


def run_pruning(
    app_ctx,
    *,
    netuid: int,
    target: int | None,
    switch_to_yuma: bool,
    wait_timeout: float,
    poll_seconds: float,
    max_batches: int,
    state_file: Path | None = None,
):
    if netuid == 0:
        raise ValueError("root cannot select Null consensus")
    state = app_ctx.run(lambda client: read_state(client, netuid))
    actor = app_ctx.resolve_dispatch_proxy() or app_ctx.resolve_address("coldkey_ss58", None)
    if actor != state.owner:
        raise ValueError("the signing origin must be the subnet owner coldkey")
    if target is None:
        target = state.pending_target if state.pending_target is not None else state.yuma_ceiling
    identity = hashlib.sha256(f"{state.genesis}:{netuid}".encode()).hexdigest()[:20]
    journal = SubmissionJournal(
        state_file or config.config_path().parent / "null-pruning" / f"{identity}.json"
    )
    with journal.locked():
        journal.check()
        app_ctx.output.message(
            "Pruning deregisters participants and can change survivor UIDs. "
            f"Target {target}; Yuma ceiling {state.yuma_ceiling}. "
            "Each transaction uses the normal confirmation and signing flow."
        )
        deadline = None
        stalled = 0
        batches = 0
        mechanism_count = state.mechanisms
        genesis = state.genesis

        def wait(reason):
            nonlocal deadline
            if deadline is None:
                deadline = time.monotonic() + wait_timeout
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError(f"wait timeout: {reason}; rerun to resume from on-chain state")
            app_ctx.output.step("waiting", reason, state="info")
            time.sleep(min(poll_seconds, remaining))

        def submit(intent):
            received = []

            def on_result(result):
                received.append(result)
                journal.finish(result)

            try:
                result = app_ctx.submit(
                    intent,
                    wait_for_finalization=True,
                    summary_note="Pruning deregisters participants and may change survivor UIDs.",
                    before_submit=lambda: journal.begin(state, target, intent.op, netuid),
                    on_result=on_result,
                )
            except typer.Exit:
                result = received[-1] if received else None
                if (
                    result is not None
                    and result.block_hash
                    and result.extrinsic_id
                    and result.error is not None
                    and result.error.name
                    in {
                        "AdminActionProhibitedDuringWeightsWindow",
                        "TxRateLimitExceeded",
                    }
                ):
                    return result
                if journal.path.exists():
                    app_ctx.output.message(
                        f"Delivery is unresolved; recovery record: {journal.path}. "
                        "Reconcile inclusion or expiry before restarting."
                    )
                raise
            if result is not None and (not result.block_hash or not result.extrinsic_id):
                raise ValueError("submission inclusion is uncertain; stopping without retry")
            return result

        while True:
            state = app_ctx.run(lambda client: read_state(client, netuid))
            if state.genesis != genesis:
                raise ValueError("RPC chain changed; stopping")
            if state.owner != actor:
                raise ValueError("subnet ownership changed; stopping")
            if state.mechanisms != mechanism_count:
                raise ValueError(
                    "mechanism count changed; restart to validate the new Yuma ceiling"
                )
            if state.pending_target is not None and state.pending_target != target:
                raise ValueError(f"conflicting on-chain pruning target {state.pending_target}")
            if state.mode == "Yuma":
                if state.population > state.yuma_ceiling:
                    raise ValueError(
                        "Yuma population exceeds its ceiling; cannot prune using Null batches"
                    )
                app_ctx.output.detail(
                    "Null pruning", {"status": "already Yuma", "population": state.population}
                )
                return
            if not state.minimum <= target <= min(state.maximum, state.yuma_ceiling):
                raise ValueError(
                    f"target must be between {state.minimum} and "
                    f"{min(state.maximum, state.yuma_ceiling)} for return to Yuma"
                )
            if state.mode != "Null":
                raise ValueError(f"unsupported epoch consensus {state.mode}")
            complete = state.population <= target and state.pending_target is None
            if complete:
                if not switch_to_yuma:
                    app_ctx.output.detail(
                        "Null pruning",
                        {
                            "status": "complete",
                            "population": state.population,
                            "target": target,
                            "consensus": "Null",
                            "batches": batches,
                        },
                    )
                    return
                reason = app_ctx.run(lambda client: switch_wait_reason(client, netuid))
                if reason and reason.startswith("pending timelocked"):
                    complete = False  # cancel remaining queues through bounded batches
                elif reason and not app_ctx.dry_run:
                    wait(reason)
                    continue
            if state.frozen and not app_ctx.dry_run:
                wait("administration window is closed")
                continue
            if not complete and batches >= max_batches:
                raise ValueError("batch limit reached; rerun to resume from on-chain state")
            intent = (
                SetHyperparameter(netuid=netuid, name="epoch_consensus", value="Yuma")
                if complete
                else TrimNullSubnetBatch(netuid=netuid, target=target)
            )
            result = submit(intent)
            if result is None:  # dry-run stops after previewing one transaction
                return
            if not result.success:
                # A finalized window race can be retried after reading fresh state.
                # Initial trimming cooldowns require an explicit later restart;
                # repeatedly submitting paid failures would not make progress.
                error_name = result.error.name if result.error is not None else None
                if error_name == "TxRateLimitExceeded":
                    raise ValueError("owner cooldown has not elapsed; rerun after the cooldown")
                if error_name != "AdminActionProhibitedDuringWeightsWindow":
                    raise ValueError("transaction failed; stopping without retry")
                wait("administration window closed before inclusion")
                continue
            after = app_ctx.run(lambda client: read_state(client, netuid))
            if complete:
                if after.mode != "Yuma":
                    raise ValueError("receipt did not switch consensus; stopping")
                app_ctx.output.detail(
                    "Null pruning",
                    {
                        "status": "complete",
                        "population": after.population,
                        "target": target,
                        "consensus": "Yuma",
                        "batches": batches,
                    },
                )
                return
            batches += 1
            progress = _find_event(result.events, "SubtensorModule", "NullUidsPruningProgress")
            if not isinstance(progress, dict):
                raise ValueError(
                    "receipt has no pruning progress event; stopping without repeating it"
                )
            if int(progress["netuid"]) != netuid or int(progress["target"]) != target:
                raise ValueError("receipt pruning target does not match this workflow")
            removed = int(progress["removed"])
            cleared = int(progress["cleared_commits"])
            cleaned = after.population <= target and after.pending_target is None
            stalled = (
                stalled + 1
                if after.population >= state.population and cleared == 0 and not cleaned
                else 0
            )
            app_ctx.output.detail(
                "Pruning progress",
                {
                    "status": "finalized",
                    "batch": batches,
                    "population": after.population,
                    "target": target,
                    "removed_uids": removed,
                    "cleared_commits": cleared,
                    "extrinsic": result.extrinsic_id,
                },
            )
            if stalled >= 3:
                raise ValueError("three batches made no progress; stopping")
            deadline = None
