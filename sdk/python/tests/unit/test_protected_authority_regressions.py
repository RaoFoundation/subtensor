"""Protected-account revocation must remove live authority, including remote changes."""

from __future__ import annotations

import asyncio
from copy import deepcopy
from types import SimpleNamespace

import pytest
import typer

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import protected as protected_cli
from bittensor.cli.context import AppContext
from bittensor.intents import ExecuteProxyAnnounced
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.sp_core import Keypair

GENESIS = "0x" + "a1" * 32
DELAY = 10


class _Chain:
    def __init__(self, grants):
        self.grants = deepcopy(grants)
        self.announcements = []
        self._substrate = self

    async def block_hash(self, block=0):
        return GENESIS if block == 0 else "0x" + "b1" * 32

    async def genesis_hash(self):
        return GENESIS

    async def block(self):
        return 100

    async def read(self, name, **_kwargs):
        if name == "proxies":
            return {"proxies": deepcopy(self.grants)}
        if name == "proxy_announcements":
            return {"announcements": deepcopy(self.announcements)}
        raise AssertionError(f"unexpected read: {name}")

    def apply(self, intent):
        if intent["op"] == "batch":
            for child in intent["intents"]:
                self.apply(child)
            return
        if intent["op"] == "remove_proxies":
            self.grants.clear()
            return
        grant = {
            "delegate": parse_recipient(intent["delegate_ss58"]).account,
            "proxy_type": intent["proxy_type"],
            "delay": intent["delay"],
        }
        if intent["op"] == "add_proxy":
            assert grant not in self.grants
            self.grants.append(grant)
        elif intent["op"] == "remove_proxy":
            self.grants.remove(grant)
        else:
            raise AssertionError(f"unexpected operation: {intent['op']}")


def _context(monkeypatch, chain, addresses):
    output = SimpleNamespace(
        message=lambda *_a, **_kw: None,
        error=lambda *_a, **_kw: None,
        name_address=lambda *_a, **_kw: None,
        classify_address=lambda *_a, **_kw: None,
        json_mode=True,
        value=lambda *_a, **_kw: None,
    )
    ctx = AppContext(
        network="test",
        wallet_name="old",
        hotkey_name=None,
        wallet_path=None,
        assume_yes=True,
        dry_run=False,
        output=output,
    )
    monkeypatch.setattr(ctx, "run", lambda fn: asyncio.run(fn(chain)))
    # Preserve the CLI's distinction: a wallet resolves to its full receiving
    # address; the real resolve_account/identity_address methods normalize it.
    monkeypatch.setattr(
        ctx,
        "resolve_address",
        lambda _param, value, **_kw: addresses.get(value or ctx.wallet_name, value),
    )
    return ctx


def _record(controller, pure, **updates):
    return {
        "controller_wallet": "old",
        "controller_ss58": receiving_address(controller),
        "protected_ss58": pure.ss58_address,
        "name": "old-protected",
        "delay_blocks": DELAY,
        "allows": [],
        "pending_allows": [],
        "allow_key_ss58": None,
        "genesis_hash": GENESIS,
        **updates,
    }


def _capture_announcements(monkeypatch, chain):
    queued = []

    def announce(_ctx, record, intent, *, category, row_extra=None):
        encoded = intent.to_dict()
        row = ann.add_announcement(
            controller_wallet=record["controller_wallet"],
            genesis_hash=GENESIS,
            delegate_ss58=record["controller_ss58"],
            real_ss58=record["protected_ss58"],
            call_hash="0x" + f"{len(queued) + 1:064x}",
            inner_op=intent.op,
            inner_args={key: value for key, value in encoded.items() if key != "op"},
            category=category,
            delay_blocks=DELAY,
            announced_at_block=1,
        )
        row = ann.update_announcement(
            record["controller_wallet"],
            row["index"],
            {**(row_extra or {})},
            genesis_hash=GENESIS,
        )
        queued.append((encoded, row))
        chain.announcements.append(
            {"real": record["protected_ss58"], "call_hash": row["call_hash"], "height": 1}
        )
        return row

    monkeypatch.setattr(protected_cli, "_announce_as_protected", announce)
    return queued


@pytest.fixture(autouse=True)
def _isolated_stores(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    monkeypatch.setenv("BTCLI_CONFIG_PATH", str(tmp_path / "config.json"))


# PR #3305: ordinary proxy management must not orphan or bypass a funded keyless account.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize(
    "operation", ["add_proxy", "remove_proxy", "remove_proxies", "kill_pure_proxy"]
)
def test_protected_account_refuses_ordinary_proxy_management(monkeypatch, operation):
    old = Keypair.create_from_seed(bytes([68]) * 32, 1)
    pure = Keypair.create_from_seed(bytes([69]) * 32, 1)
    chain = _Chain([{"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY}])
    ctx = _context(monkeypatch, chain, {"old": receiving_address(old)})
    pa.upsert_protected(_record(old, pure, setup={"status": "ready"}))

    with pytest.raises(typer.Exit) as rejected:
        protected_cli.maybe_route_protected(
            ctx,
            SimpleNamespace(op=operation, signer="coldkey"),
            proxy_for=None,
            user_set_proxy=False,
        )

    assert rejected.value.exit_code == 2
    assert chain.grants == [{"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY}]


# PR #3305: rotation must not strand executable calls or their reserved deposits under the old key.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_rotation_refuses_any_pending_old_controller_announcement(monkeypatch):
    old = Keypair.create_from_seed(bytes([65]) * 32, 1)
    new = Keypair.create_from_seed(bytes([66]) * 32, 1)
    pure = Keypair.create_from_seed(bytes([67]) * 32, 1)
    chain = _Chain([{"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY}])
    ctx = _context(monkeypatch, chain, {"old": receiving_address(old)})
    record = _record(old, pure, setup={"status": "ready"})
    pa.upsert_protected(record)
    row = ann.add_announcement(
        controller_wallet="old",
        genesis_hash=GENESIS,
        delegate_ss58=old.ss58_address,
        real_ss58=pure.ss58_address,
        call_hash="0x" + "44" * 32,
        inner_op="transfer",
        inner_args={"dest_ss58": new.ss58_address, "amount_tao": "1"},
        category="transfer",
        delay_blocks=DELAY,
        announced_at_block=1,
        call_data="0x0102",
    )
    chain.announcements.append(
        {"real": pure.ss58_address, "call_hash": row["call_hash"], "height": 1}
    )
    monkeypatch.setattr(
        protected_cli,
        "_resolve_rotation_destination",
        lambda *_args: ("new", new.ss58_address, new.ss58_address),
    )

    with pytest.raises(typer.Exit) as rejected:
        protected_cli.run_rotate(ctx, to_wallet="new")

    assert rejected.value.exit_code == 2
    assert chain.announcements
    assert chain.grants == [{"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY}]


# PR #3305: controller recovery must clear an attacker-planted grant instead of preserving it.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_rotation_does_not_restore_unrecorded_foreign_grants(monkeypatch):
    old = Keypair.create_from_seed(bytes([62]) * 32, 1)
    new = Keypair.create_from_seed(bytes([63]) * 32, 1)
    attacker = Keypair.create_from_seed(bytes([64]) * 32, 1)
    pure = Keypair.create_from_seed(bytes([61]) * 32, 1)
    chain = _Chain(
        [
            {"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY},
            {"delegate": attacker.ss58_address, "proxy_type": "Any", "delay": 0},
        ]
    )
    ctx = _context(monkeypatch, chain, {"old": receiving_address(old)})
    record = _record(old, pure, setup={"status": "ready"})

    children, migrated = protected_cli._rotation_batch_children(
        ctx,
        record,
        controller_ss58=old.ss58_address,
        new_ss58=new.ss58_address,
        delay=DELAY,
    )

    assert migrated == []
    assert children == [
        {"op": "remove_proxies", "from_hotkey": False},
        {
            "op": "add_proxy",
            "delegate_ss58": new.ss58_address,
            "proxy_type": "Any",
            "delay": DELAY,
            "from_hotkey": None,
        },
    ]


# PR #3305: rotation must not restore zero-delay authority to the controller it revokes.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_rotation_migrates_controller_allow_without_restoring_old_key(monkeypatch):
    old = Keypair.create_from_seed(bytes([77]) * 32, 1)
    new = Keypair.create_from_seed(bytes([78]) * 32, 1)
    pure = Keypair.create_from_seed(bytes([79]) * 32, 1)
    chain = _Chain(
        [
            {"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY},
            {"delegate": old.ss58_address, "proxy_type": "GammaSpend", "delay": 0},
        ]
    )
    ctx = _context(monkeypatch, chain, {"old": receiving_address(old)})
    record = _record(
        old,
        pure,
        allows=["GammaSpend"],
        allow_key_ss58=receiving_address(old),
    )

    children, migrated = protected_cli._rotation_batch_children(
        ctx,
        record,
        controller_ss58=receiving_address(old),
        new_ss58=new.ss58_address,
        delay=DELAY,
    )
    chain.apply({"op": "batch", "intents": children})

    assert migrated == ["GammaSpend"]
    assert not any(grant["delegate"] == old.ss58_address for grant in chain.grants)
    assert {"delegate": new.ss58_address, "proxy_type": "Any", "delay": DELAY} in chain.grants
    assert {"delegate": new.ss58_address, "proxy_type": "GammaSpend", "delay": 0} in chain.grants


# PR #3305: hashed identities must not leave a revoked controller able to spend protected funds.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("old_crypto_type,new_crypto_type", [(6, 1), (1, 6), (6, 6)])
def test_hashed_controller_rotation_revokes_every_live_grant(
    monkeypatch, old_crypto_type, new_crypto_type
):
    old = Keypair.create_from_seed(bytes([71]) * 32, old_crypto_type)
    new = Keypair.create_from_seed(bytes([72]) * 32, new_crypto_type)
    pure = Keypair.create_from_seed(bytes([73]) * 32, 1)
    # The second Any grant was created elsewhere and is absent from local
    # allows. Rotation must discover and revoke it from actual chain state.
    chain = _Chain(
        [
            {"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY},
            {"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY + 5},
        ]
    )
    ctx = _context(
        monkeypatch, chain, {"old": receiving_address(old), "new": receiving_address(new)}
    )
    record = _record(
        old,
        pure,
        allow_key_ss58=receiving_address(old),
        allow_key_address=receiving_address(old),
    )
    pa.upsert_protected(record)
    queued = _capture_announcements(monkeypatch, chain)

    # A matching new grant is insufficient while the old identity retains
    # authority, even when the local record names it by receiving descriptor.
    new_grant = {"delegate": new.ss58_address, "proxy_type": "Any", "delay": DELAY}
    chain.grants.append(new_grant)
    assert not protected_cli._rotation_complete_on_chain(
        ctx,
        record,
        new_controller_ss58=receiving_address(new),
        old_controller_ss58=receiving_address(old),
    )
    chain.grants.remove(new_grant)

    protected_cli.run_rotate(ctx, to_wallet="new")
    assert len(queued) == 1
    chain.apply(queued[0][0])
    assert not any(grant["delegate"] == old.ss58_address for grant in chain.grants)
    assert new_grant in chain.grants

    protected_cli.run_rotate_finalize(ctx, to_wallet="new")
    assert pa.get_by_controller("old", genesis_hash=GENESIS) is None
    moved = pa.get_by_controller("new", genesis_hash=GENESIS)
    assert moved["controller_ss58"] == new.ss58_address
    assert moved["allow_key_ss58"] == new.ss58_address
    # Subsequent allow updates must not use a cached receiving descriptor for
    # the old controller and silently grant its authority back.
    ctx.wallet_name = "new"
    protected_cli._protect_update(
        ctx,
        moved,
        delay_blocks=DELAY,
        allow_types=["Lock"],
        disallow_types=[],
        allow_key_ss58=None,
    )
    assert len(queued) == 2
    allow, _row = queued[1]
    assert allow["op"] == "add_proxy"
    assert parse_recipient(allow["delegate_ss58"]).account == new.ss58_address


# PR #3305: an externally activated allow must remain revocable after its announcement disappears.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("execution_timing", ["before_show", "during_disallow"])
def test_disallow_revokes_allow_executed_from_another_device(monkeypatch, execution_timing):
    old = Keypair.create_from_seed(bytes([74]) * 32, 1)
    allow = Keypair.create_from_seed(bytes([75]) * 32, 1)
    pure = Keypair.create_from_seed(bytes([76]) * 32, 1)
    allow_grant = {"delegate": allow.ss58_address, "proxy_type": "Lock", "delay": 0}
    chain = _Chain([{"delegate": old.ss58_address, "proxy_type": "Any", "delay": DELAY}])
    if execution_timing == "before_show":
        chain.grants.append(allow_grant)
    ctx = _context(monkeypatch, chain, {"old": old.ss58_address})
    record = _record(old, pure, pending_allows=["Lock"], allow_key_ss58=allow.ss58_address)
    pa.upsert_protected(record)
    allow_row = ann.add_announcement(
        controller_wallet="old",
        genesis_hash=GENESIS,
        delegate_ss58=old.ss58_address,
        real_ss58=pure.ss58_address,
        call_hash="0x" + "e1" * 32,
        inner_op="add_proxy",
        inner_args={"delegate_ss58": allow.ss58_address, "proxy_type": "Lock", "delay": 0},
        category="allow:Lock",
        delay_blocks=DELAY,
        announced_at_block=1,
    )
    if execution_timing == "during_disallow":
        chain.announcements.append(
            {"real": pure.ss58_address, "call_hash": allow_row["call_hash"], "height": 1}
        )
    # Someone else executed the allow. It is absent from Proxy.Announcements,
    # but the live delegate is still authorized to lock the account's stake.
    protected_cli.run_show(ctx, index=None)
    if execution_timing == "during_disallow":
        read = chain.read
        executed = False

        async def execute_after_first_snapshot(name, **kwargs):
            nonlocal executed
            snapshot = await read(name, **kwargs)
            if name == "proxies" and not executed:
                # A permissionless executor lands just after --disallow reads
                # live grants, before it reconciles/cancels the old commitment.
                chain.grants.append(allow_grant)
                chain.announcements.clear()
                executed = True
            return snapshot

        monkeypatch.setattr(chain, "read", execute_after_first_snapshot)
    queued = _capture_announcements(monkeypatch, chain)
    protected_cli._protect_update(
        ctx,
        pa.get_by_controller("old", genesis_hash=GENESIS),
        delay_blocks=DELAY,
        allow_types=[],
        disallow_types=["Lock"],
        allow_key_ss58=None,
    )
    assert len(queued) == 1
    removal, row = queued[0]
    assert removal["op"] == "remove_proxy"
    assert parse_recipient(removal["delegate_ss58"]).account == allow.ss58_address
    assert removal["proxy_type"] == "Lock"
    assert removal["delay"] == 0
    assert allow_grant in chain.grants
    assert "Lock" in pa.get_by_controller("old", genesis_hash=GENESIS)["allows"]

    def execute(intent, **_kwargs):
        assert isinstance(intent, ExecuteProxyAnnounced)
        chain.apply({"op": intent.inner_op, **intent.inner_args})
        chain.announcements.clear()
        return SimpleNamespace(success=True, events=[])

    monkeypatch.setattr(ctx, "submit", execute)
    protected_cli._execute_one(ctx, row, current_block=100)
    assert allow_grant not in chain.grants
    assert "Lock" not in pa.get_by_controller("old", genesis_hash=GENESIS)["allows"]
    assert "Lock" not in pa.get_by_controller("old", genesis_hash=GENESIS)["pending_allows"]
