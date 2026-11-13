"""Show detail must hash saved call bytes, not rebuild dynamic args."""

from __future__ import annotations

import asyncio
from hashlib import blake2b
from types import SimpleNamespace

import pytest

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import protected
from tests.harness.samples import ALICE, BOB, BOB_HOT

CHAIN = "0x" + "aa" * 32
# Distinct from the bytes' true hash so a rebuild-based check would be tempting.
SAVED_CALL = "0x010203040506"
CALL_HASH = "0x" + blake2b(bytes.fromhex(SAVED_CALL[2:]), digest_size=32).hexdigest()


class _Output:
    json_mode = True

    def __init__(self):
        self.details = []
        self.messages = []

    def detail(self, title, value):
        self.details.append((title, value))

    def message(self, message):
        self.messages.append(message)

    def value(self, value):
        pass

    def error(self, error, **kwargs):
        raise AssertionError((error, kwargs))


class _Context:
    def __init__(self):
        self.wallet_name = "main"
        self.network = "local"
        self.dry_run = False
        self.output = _Output()
        self._rebuilds = 0

    def wallet(self):
        raise AssertionError("show must not rebuild intents when call_data is saved")

    def run(self, callback):
        self._rebuilds += 1

        async def block_hash(block=0):
            return CHAIN

        async def block():
            return 150

        async def read(operation, **kwargs):
            if operation == "proxies":
                return {"proxies": [{"delegate": ALICE, "proxy_type": "Any", "delay": 10}]}
            assert operation == "proxy_announcements"
            return {"announcements": [{"real": BOB_HOT, "call_hash": CALL_HASH, "height": 100}]}

        client = SimpleNamespace(
            _substrate=SimpleNamespace(block_hash=block_hash),
            block=block,
            read=read,
        )
        return asyncio.run(callback(client))


@pytest.fixture
def show_ctx(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    pa.save_protected(
        [
            {
                "controller_wallet": "main",
                "controller_ss58": ALICE,
                "protected_ss58": BOB_HOT,
                "name": "main-protected",
                "delay_blocks": 10,
                "allows": [],
                "spawner": ALICE,
                "height": 50,
                "ext_index": 3,
                "index": 0,
                "genesis_hash": CHAIN,
            }
        ]
    )
    # Dynamic args that would rebuild to different SCALE if shown against live state.
    ann.save_announcements(
        [
            {
                "index": 1,
                "controller_wallet": "main",
                "delegate_ss58": ALICE,
                "real_ss58": BOB_HOT,
                "call_hash": CALL_HASH,
                "inner_op": "transfer",
                "inner_args": {"dest_ss58": BOB, "amount": "all"},
                "category": "transfer",
                "delay_blocks": 10,
                "announced_at_block": 100,
                "executable_at_block": 110,
                "force_proxy_type": "Any",
                "status": "pending",
                "call_data": SAVED_CALL,
                "genesis_hash": CHAIN,
            }
        ]
    )
    return _Context()


def test_show_detail_hashes_saved_call_data_not_rebuilt_args(show_ctx):
    protected.run_show(show_ctx, index=1)

    # wallet() raises if compose/rebuild path is used; chain reads may call run().
    title, detail = show_ctx.output.details[-1]
    assert title == "announcement #1"
    assert detail["hash_match"] is True
    assert detail["recomputed_call_hash"].lower() == CALL_HASH.lower()
    assert detail["call_data"] == SAVED_CALL
    assert show_ctx._rebuilds >= 1
