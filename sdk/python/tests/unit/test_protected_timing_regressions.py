"""Reported cancellation windows must match the proxy pallet's earliest execution."""

from __future__ import annotations

import asyncio
import json
from copy import deepcopy
from hashlib import blake2b
from types import SimpleNamespace

import pytest
import typer

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import prompt
from bittensor.cli import protected as protected_cli
from bittensor.cli.context import AppContext
from bittensor.intents import AnnounceProxyCall, ExecuteProxyAnnounced, Transfer
from bittensor.intents.registry import build as build_intent
from bittensor.sp_core import Keypair

GENESIS = "0x" + "c1" * 32


def _payload(intent):
    return json.dumps(intent.to_dict(), sort_keys=True, default=str).encode()


class _TimingChain:
    """RPC fixture preserving the pallet's duplicate-consumption semantics."""

    def __init__(self, controller, pure, delay):
        self.controller = controller
        self.pure = pure
        self.grants = [{"delegate": controller, "proxy_type": "Any", "delay": delay}]
        self.announcements = []
        self.current_block = 100
        self.balances = {pure: 10_000_000_000}
        self._substrate = self

    async def genesis_hash(self):
        return GENESIS

    async def block_hash(self, block=0):
        return GENESIS if block == 0 else "0x" + "d1" * 32

    async def block(self):
        return self.current_block

    async def read(self, name, **_kwargs):
        if name == "proxies":
            return {"proxies": deepcopy(self.grants)}
        if name == "proxy_announcements":
            return {"announcements": deepcopy(self.announcements)}
        raise AssertionError(f"unexpected read: {name}")

    def _apply(self, encoded):
        if encoded["op"] == "batch":
            for child in encoded["intents"]:
                self._apply(child)
        elif encoded["op"] in ("add_proxy", "remove_proxy"):
            grant = {
                "delegate": encoded["delegate_ss58"],
                "proxy_type": encoded["proxy_type"],
                "delay": encoded["delay"],
            }
            if encoded["op"] == "add_proxy":
                assert grant not in self.grants
                self.grants.append(grant)
            else:
                self.grants.remove(grant)
        elif encoded["op"] == "transfer":
            intent = build_intent("transfer", {k: v for k, v in encoded.items() if k != "op"})
            amount = intent.amount_tao.rao
            self.balances[self.pure] -= amount
            self.balances[intent.dest_ss58] = self.balances.get(intent.dest_ss58, 0) + amount
        else:
            raise AssertionError(f"unexpected operation: {encoded['op']}")

    def submit(self, intent, **_kwargs):
        if isinstance(intent, AnnounceProxyCall):
            self.announcements.append(
                {
                    "real": intent.real_ss58,
                    "call_hash": intent.call_hash,
                    "height": self.current_block,
                }
            )
        else:
            assert isinstance(intent, ExecuteProxyAnnounced)
            payload = bytes.fromhex(intent.announced_call_data.removeprefix("0x"))
            call_hash = "0x" + blake2b(payload, digest_size=32).hexdigest()
            delay = min(g["delay"] for g in self.grants if g["proxy_type"] == "Any")
            matured = [
                row
                for row in self.announcements
                if row["real"] == intent.real_ss58
                and row["call_hash"] == call_hash
                and self.current_block - row["height"] >= delay
            ]
            assert matured, "proxy_announced requires at least one matured commitment"
            # pallets/proxy removes every matching matured entry, retaining
            # younger duplicates. The payload dispatches only once.
            self.announcements = [row for row in self.announcements if row not in matured]
            self._apply({"op": intent.inner_op, **intent.inner_args})
        return SimpleNamespace(
            success=True, events=[], extrinsic_id=f"{self.current_block}-0", data={}
        )


@pytest.fixture
def timing_setup(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    controller = Keypair.create_from_seed(bytes([81]) * 32, 1).ss58_address
    pure = Keypair.create_from_seed(bytes([82]) * 32, 1).ss58_address
    destination = Keypair.create_from_seed(bytes([83]) * 32, 1).ss58_address
    chain = _TimingChain(controller, pure, delay=36)
    shown = []
    messages = []
    output = SimpleNamespace(
        json_mode=True,
        value=shown.append,
        message=messages.append,
        error=lambda *_a, **_kw: None,
    )
    ctx = AppContext(
        network="test",
        wallet_name="controller",
        hotkey_name=None,
        wallet_path=None,
        assume_yes=True,
        dry_run=False,
        output=output,
    )
    monkeypatch.setattr(ctx, "run", lambda fn: asyncio.run(fn(chain)))
    monkeypatch.setattr(ctx, "wallet", lambda: SimpleNamespace())
    monkeypatch.setattr(ctx, "submit", chain.submit)
    monkeypatch.setattr(prompt, "confirm_wallet", lambda *_a, **_kw: "controller")

    async def compose(_substrate, intent, _real, _wallet):
        payload = _payload(intent)
        digest = blake2b(payload, digest_size=32).digest()
        return SimpleNamespace(data=payload, call_hash=digest), "0x" + digest.hex(), {}

    monkeypatch.setattr(protected_cli, "compose_announceable_call", compose)
    record = {
        "controller_wallet": "controller",
        "controller_ss58": controller,
        "protected_ss58": pure,
        "name": "protected",
        "delay_blocks": 36,
        "allows": [],
        "pending_allows": [],
        "genesis_hash": GENESIS,
    }
    pa.upsert_protected(record)
    return ctx, chain, record, destination, shown, messages


# PR #3305: a later duplicate must not hide an already executable transfer from its owner.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("heights", [(100, 130), (130, 100), (100, 100)])
def test_duplicate_hash_uses_earliest_window_and_preserves_younger_commitment(
    timing_setup, heights
):
    ctx, chain, record, destination, shown, _messages = timing_setup
    transfer = Transfer(dest_ss58=destination, amount_tao=1)
    protected_cli._submit_protected_announcement(ctx, transfer, record)
    row = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    chain.announcements = [
        {"real": chain.pure, "call_hash": row["call_hash"], "height": height} for height in heights
    ]
    chain.current_block = 136

    protected_cli.run_show(ctx, index=None)
    pending = shown[-1]["pending"]
    assert len(pending) == 2
    assert sorted(row["announced_at_block"] for row in pending) == sorted(heights)
    assert len({row["index"] for row in pending}) == 2
    assert len({(row["announced_at_block"], row["announcement_ordinal"]) for row in pending}) == 2
    for pending_row in pending:
        height = pending_row["announced_at_block"]
        assert pending_row["executable_at_block"] == height + 36
        assert pending_row["status"] == ("ready" if height == 100 else "waiting")

    # A retry after a lost receipt recovers both concrete commitments instead
    # of reserving a third deposit for the same call hash.
    protected_cli._submit_protected_announcement(ctx, transfer, record)
    assert len(chain.announcements) == 2

    ready = next(row for row in pending if row["status"] == "ready")
    protected_cli.run_execute_protected(ctx, index=ready["index"])
    assert chain.balances[destination] == 1_000_000_000
    expected_remaining = [height for height in heights if height == 130]
    assert [item["height"] for item in chain.announcements] == expected_remaining
    protected_cli.run_show(ctx, index=None)
    later = shown[-1]["pending"]
    assert len(later) == len(expected_remaining)
    if later:
        original_later = next(row for row in pending if row["announced_at_block"] == 130)
        assert later[0]["index"] == original_later["index"]
        assert later[0]["executable_at_block"] == 166
        assert later[0]["status"] == "waiting"
        chain.current_block = 166
        protected_cli.run_execute_protected(ctx, index=None)
        assert chain.balances[destination] == 1_000_000_000
        assert chain.announcements
        protected_cli.run_execute_protected(ctx, index=later[0]["index"])
        assert chain.balances[destination] == 2_000_000_000


# PR #3305: a lost submission receipt must not allow an identical mempool retry.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_plaintext_is_reserved_before_submit_and_blocks_unknown_retry(timing_setup, monkeypatch):
    ctx, _chain, record, destination, _shown, _messages = timing_setup
    transfer = Transfer(dest_ss58=destination, amount_tao=1)

    def lose_receipt(*_args, **_kwargs):
        raise OSError("connection lost after broadcast")

    monkeypatch.setattr(ctx, "submit", lose_receipt)
    with pytest.raises(OSError, match="connection lost"):
        protected_cli._submit_protected_announcement(ctx, transfer, record)

    reserved = ann.list_for_wallet("controller", genesis_hash=GENESIS)
    assert len(reserved) == 1
    assert reserved[0]["status"] == "submitting"
    assert reserved[0]["call_data"]
    with pytest.raises(typer.Exit) as rejected:
        protected_cli._submit_protected_announcement(ctx, transfer, record)
    assert rejected.value.exit_code == 2


# PR #3305: declining before broadcast must not permanently block retry or rotation.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_declined_announcement_releases_local_reservation(timing_setup, monkeypatch):
    ctx, chain, record, destination, _shown, _messages = timing_setup
    transfer = Transfer(dest_ss58=destination, amount_tao=1)

    def decline(*_args, **_kwargs):
        raise typer.Exit(130)

    monkeypatch.setattr(ctx, "submit", decline)
    with pytest.raises(typer.Exit) as declined:
        protected_cli._submit_protected_announcement(ctx, transfer, record)
    assert declined.value.exit_code == 130
    assert ann.list_for_wallet("controller", genesis_hash=GENESIS) == []
    assert chain.announcements == []

    monkeypatch.setattr(ctx, "submit", chain.submit)
    protected_cli._submit_protected_announcement(ctx, transfer, record)
    assert len(chain.announcements) == 1


# PR #3305: overlapping delay updates must never advertise a longer cancellation window than live.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("submit_method", ["daily", "settings"])
def test_earlier_delay_update_controls_subsequent_announcement_window(timing_setup, submit_method):
    ctx, chain, record, destination, _shown, messages = timing_setup
    for target in (24, 72):
        protected_cli._protect_update(
            ctx,
            pa.get_by_controller("controller", genesis_hash=GENESIS),
            delay_blocks=target,
            allow_types=[],
            disallow_types=[],
            allow_key_ss58=None,
        )
    queued = ann.list_for_wallet("controller", genesis_hash=GENESIS)
    assert len(queued) == 2
    chain.current_block = 136
    protected_cli.run_execute_protected(ctx, index=queued[0]["index"])
    active = pa.get_by_controller("controller", genesis_hash=GENESIS)
    assert chain.grants == [{"delegate": chain.controller, "proxy_type": "Any", "delay": 24}]
    assert active["delay_blocks"] == 24
    assert active["pending_delay_blocks"] == 72

    # Even a caller retaining the pre-execution snapshot must display the live
    # 24-block window rather than the old 36-block value or latest pending 72.
    transfer = Transfer(dest_ss58=destination, amount_tao=1)
    if submit_method == "daily":
        protected_cli._submit_protected_announcement(ctx, transfer, record)
    else:
        protected_cli._announce_as_protected(ctx, record, transfer, category="transfer")
    latest = ann.list_for_wallet("controller", genesis_hash=GENESIS)[-1]
    assert latest["delay_blocks"] == 24
    assert latest["announced_at_block"] == 136
    assert latest["executable_at_block"] == 160
    assert "executes after block 160" in messages[-1]
    assert pa.get_by_controller("controller", genesis_hash=GENESIS)["pending_delay_blocks"] == 72


# PR #3305: a delay shortened while signing must be reflected in the displayed cancellation window.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("submit_method", ["daily", "settings"])
def test_announcement_receipt_refreshes_delay_changed_while_signing(
    timing_setup, monkeypatch, submit_method
):
    ctx, chain, record, destination, _shown, messages = timing_setup
    submit = chain.submit

    def submit_after_delay_change(intent, **kwargs):
        result = submit(intent, **kwargs)
        assert isinstance(intent, AnnounceProxyCall)
        # Another device executes an already matured delay update while this
        # signing/receipt flow is in progress. The pre-submit read saw 36.
        chain.grants[0]["delay"] = 24
        return result

    monkeypatch.setattr(ctx, "submit", submit_after_delay_change)
    transfer = Transfer(dest_ss58=destination, amount_tao=1)
    if submit_method == "daily":
        protected_cli._submit_protected_announcement(ctx, transfer, record)
    else:
        protected_cli._announce_as_protected(ctx, record, transfer, category="transfer")

    row = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    assert row["delay_blocks"] == 24
    assert row["executable_at_block"] == 124
    assert "executes after block 124" in messages[-1]
