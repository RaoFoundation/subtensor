"""Inspect the committed bytes, even when rebuilding would resolve different inputs."""

from __future__ import annotations

import asyncio
from copy import deepcopy
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor._generated.runtime_apis import StakeInfoRuntimeApi, SwapRuntimeApi
from bittensor._transport.contract import CallBytes
from bittensor.cli import prompt
from bittensor.cli import protected as protected_cli
from bittensor.cli.context import AppContext
from bittensor.intents import AnnounceProxyCall, RemoveStake
from bittensor.intents.proxy import compose_announceable_call
from bittensor.sp_core import ss58_decode
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, dev_wallet

GENESIS = "0x" + "f1" * 32


class _HashChain(FakeSubstrate):
    def __init__(self):
        super().__init__()
        del self.block
        self._substrate = self
        self.announcements = []
        self.composed = []

    async def block_hash(self, block=0):
        return GENESIS

    async def block(self):
        return 100

    async def read(self, name, **_kwargs):
        if name == "proxies":
            return {"proxies": [{"delegate": ALICE, "proxy_type": "Any", "delay": 36}]}
        if name == "proxy_announcements":
            return {"announcements": deepcopy(self.announcements)}
        raise AssertionError(f"unexpected read: {name}")

    async def compose(self, call):
        module, function, params = call
        assert (module, function) == ("SubtensorModule", "remove_stake_limit")
        self.composed.append(deepcopy(params))
        # SCALE layout from the runtime's pallet index 7 / call index 89:
        # AccountId32, NetUid(u16), AlphaBalance(u64), TaoBalance(u64), bool.
        data = (
            bytes((7, 89))
            + bytes(ss58_decode(params["hotkey"]))
            + params["netuid"].to_bytes(2, "little")
            + params["amount_unstaked"].to_bytes(8, "little")
            + params["limit_price"].to_bytes(8, "little")
            + bytes((params["allow_partial"],))
        )
        return CallBytes(data=data, spec_version=489)

    def submit(self, intent, **_kwargs):
        assert isinstance(intent, AnnounceProxyCall)
        self.announcements.append({"real": BOB, "call_hash": intent.call_hash, "height": 100})
        return SimpleNamespace(success=True, events=[], extrinsic_id="100-0", data={})


@pytest.fixture
def saved_hash_setup(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    chain = _HashChain()
    chain.seed_runtime(*SwapRuntimeApi.current_alpha_price, 1_000_000_000)
    chain.seed_runtime(
        *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid, {"stake": 2_000_000_000}
    )
    shown, messages = [], []
    output = SimpleNamespace(
        detail=lambda _title, value: shown.append(value),
        message=messages.append,
        error=lambda *_a, **_kw: None,
        json_mode=True,
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
    monkeypatch.setattr(ctx, "wallet", Mock(return_value=dev_wallet()))
    monkeypatch.setattr(ctx, "submit", chain.submit)
    monkeypatch.setattr(prompt, "confirm_wallet", lambda *_a, **_kw: "controller")
    record = {
        "controller_wallet": "controller",
        "controller_ss58": ALICE,
        "protected_ss58": BOB,
        "name": "protected",
        "delay_blocks": 36,
        "allows": [],
        "pending_allows": [],
        "genesis_hash": GENESIS,
    }
    pa.upsert_protected(record)
    intent = RemoveStake(hotkey_ss58=ALICE_HOT, netuid=1, amount_alpha="all")
    protected_cli._submit_protected_announcement(ctx, intent, record)
    row = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    return ctx, chain, intent, row, shown, messages


@pytest.mark.parametrize("changed", ["stake", "price"])
def test_show_verifies_original_bytes_after_dynamic_inputs_change(saved_hash_setup, changed):
    ctx, chain, intent, row, shown, _messages = saved_hash_setup
    if changed == "stake":
        chain.seed_runtime(
            *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid, {"stake": 9_000_000_000}
        )
    else:
        chain.seed_runtime(*SwapRuntimeApi.current_alpha_price, 3_000_000_000)
    _call, rebuilt_hash, _extras = ctx.run(
        lambda client: compose_announceable_call(client, intent, BOB, ctx.wallet())
    )
    assert rebuilt_hash != row["call_hash"]
    before = (len(chain.composed), ctx.wallet.call_count)

    protected_cli.run_show(ctx, index=row["index"])

    assert shown[-1]["hash_match"] is True
    assert shown[-1]["recomputed_call_hash"] == row["call_hash"]
    assert (len(chain.composed), ctx.wallet.call_count) == before


def test_show_detects_tampered_saved_bytes(saved_hash_setup):
    ctx, chain, _intent, row, shown, _messages = saved_hash_setup
    original = bytes.fromhex(row["call_data"][2:])
    tampered = original[:-1] + bytes((original[-1] ^ 1,))
    ann.update_announcement(
        "controller", row["index"], {"call_data": "0x" + tampered.hex()}, genesis_hash=GENESIS
    )
    before = len(chain.composed)

    protected_cli.run_show(ctx, index=row["index"])

    assert shown[-1]["hash_match"] is False
    assert shown[-1]["recomputed_call_hash"] == "0x" + CallBytes(tampered, 489).call_hash.hex()
    assert len(chain.composed) == before


@pytest.mark.parametrize("malformed", ["", "0x", "0x0", "0xgg", "deadbeef", "0x00 01", None, 1])
def test_show_rejects_malformed_saved_bytes_without_rebuilding(saved_hash_setup, malformed):
    ctx, chain, _intent, row, shown, messages = saved_hash_setup
    ann.update_announcement(
        "controller", row["index"], {"call_data": malformed}, genesis_hash=GENESIS
    )
    before = (len(chain.composed), ctx.wallet.call_count)

    protected_cli.run_show(ctx, index=row["index"])

    assert shown[-1]["hash_match"] is False
    assert shown[-1]["recomputed_call_hash"] is None
    assert "invalid saved call data" in messages[-1]
    assert (len(chain.composed), ctx.wallet.call_count) == before


@pytest.mark.parametrize("inner_op", ["unavailable_intent", None])
def test_show_saved_bytes_needs_no_wallet_or_rebuildable_intent(saved_hash_setup, inner_op):
    ctx, chain, _intent, row, shown, _messages = saved_hash_setup
    ann.update_announcement(
        "controller",
        row["index"],
        {"inner_op": inner_op, "plaintext_missing": True},
        genesis_hash=GENESIS,
    )
    ctx.wallet.side_effect = AssertionError("inspection must not load a wallet")
    before = len(chain.composed)

    protected_cli.run_show(ctx, index=row["index"])

    assert shown[-1]["hash_match"] is True
    assert shown[-1]["recomputed_call_hash"] == row["call_hash"]
    assert len(chain.composed) == before


def test_show_legacy_row_without_saved_bytes_rebuilds(saved_hash_setup):
    ctx, chain, _intent, row, shown, _messages = saved_hash_setup
    entries = ann.load_announcements()
    entries[0].pop("call_data")
    ann.save_announcements(entries)
    before = (len(chain.composed), ctx.wallet.call_count)

    protected_cli.run_show(ctx, index=row["index"])

    assert shown[-1]["hash_match"] is True
    assert shown[-1]["recomputed_call_hash"] == row["call_hash"]
    assert (len(chain.composed), ctx.wallet.call_count) == (before[0] + 1, before[1] + 1)
