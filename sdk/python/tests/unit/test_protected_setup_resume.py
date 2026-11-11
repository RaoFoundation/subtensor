"""Interrupted protected setup resumes without duplicate grants or migration fees."""

from __future__ import annotations

import asyncio
import json
from copy import deepcopy
from hashlib import blake2b
from types import SimpleNamespace

import pytest
import typer

from bittensor import config as cfg
from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import prompt
from bittensor.cli import protected as protected_cli
from bittensor.cli.context import AppContext
from bittensor.intents import AnnounceColdkeySwap, AnnounceProxyCall, CreatePureProxy, Transfer
from bittensor.sp_core import Keypair, ss58_decode

GENESIS = "0x" + "e1" * 32


class _SetupChain:
    def __init__(self, controller):
        self.controller = controller
        self.pure = pa.derive_legacy_pure_ss58(
            controller, height=100, ext_index=0, proxy_type="Any", index=0
        )
        self.grants = []
        self.announcements = []
        self.swap = None
        self.payloads = {}
        self.submitted = []
        self.fail_allow = None
        self.lose_allow_receipt = None
        self.fail_swap = False
        self.lose_swap_status = False
        self.lose_swap_receipt = False
        self.fail_render = None
        self._status_failure_due = False
        self.swap_approval_only = False
        self.pending_multisig = False
        self.fail_multisig_read = False
        self._substrate = self

    async def block_hash(self, block=0):
        return GENESIS if block == 0 else "0x" + "e2" * 32

    async def block(self):
        return 100

    async def events(self, _block_hash):
        return [
            {
                "extrinsic_idx": 0,
                "event": {
                    "module_id": "Proxy",
                    "event_id": "PureCreated",
                    "attributes": {"pure": self.pure, "who": self.controller},
                },
            }
        ]

    async def read(self, name, **_kwargs):
        if name == "proxies":
            return {"proxies": deepcopy(self.grants)}
        if name == "proxy_announcements":
            return {"announcements": deepcopy(self.announcements)}
        if name == "coldkey_swap_announcement":
            if self._status_failure_due:
                self._status_failure_due = False
                raise OSError("migration status RPC interrupted")
            return deepcopy(self.swap)
        if name == "multisig":
            assert _kwargs == {"account_ss58": self.controller, "call_hash": "0x" + "e3" * 32}
            if self.fail_multisig_read:
                raise OSError("multisig RPC unavailable")
            return {"timepoint": {"height": 100, "index": 0}} if self.pending_multisig else None
        raise AssertionError(f"unexpected read {name}")

    def announce_swap(self):
        self.pending_multisig = False
        self.swap = {
            "execute_block": 200,
            "new_coldkey_hash": "0x"
            + blake2b(bytes(ss58_decode(self.pure)), digest_size=32).hexdigest(),
            "disputed": False,
            "recovery_locked": False,
        }

    def submit(self, intent, **kwargs):
        data = {}
        events = []
        if isinstance(intent, CreatePureProxy):
            self.grants.append(
                {"delegate": self.controller, "proxy_type": "Any", "delay": intent.delay}
            )
            data = {
                "pure_proxy": self.pure,
                "spawner": self.controller,
                "height": 100,
                "ext_index": 0,
            }
        elif isinstance(intent, AnnounceProxyCall):
            payload = self.payloads[intent.call_hash]
            if self.fail_allow is not None and self.fail_allow == payload.get("proxy_type"):
                self.fail_allow = None
                return SimpleNamespace(success=False, events=[], data={}, extrinsic_id=None)
            self.announcements.append(
                {"real": self.pure, "call_hash": intent.call_hash, "height": 100}
            )
            if self.lose_allow_receipt is not None and self.lose_allow_receipt == payload.get(
                "proxy_type"
            ):
                self.lose_allow_receipt = None
                self.submitted.append(intent)
                raise OSError("allow receipt interrupted")
        elif isinstance(intent, AnnounceColdkeySwap):
            if self.fail_swap:
                self.fail_swap = False
                return SimpleNamespace(success=False, events=[], data={}, extrinsic_id=None)
            if self.swap_approval_only:
                self.pending_multisig = True
                events = [
                    {
                        "event": {
                            "module_id": "Multisig",
                            "event_id": "NewMultisig",
                            "attributes": {
                                "multisig": self.controller,
                                "call_hash": "0x" + "e3" * 32,
                            },
                        }
                    }
                ]
            else:
                self.announce_swap()
                self._status_failure_due = self.lose_swap_status
                self.lose_swap_status = False
                if self.lose_swap_receipt:
                    self.lose_swap_receipt = False
                    self.submitted.append(intent)
                    raise OSError("migration receipt interrupted")
        else:
            raise AssertionError(f"unexpected submit {intent.op}")
        self.submitted.append(intent)
        result = SimpleNamespace(success=True, events=events, data=data, extrinsic_id="100-0")
        if kwargs.get("on_result"):
            kwargs["on_result"](result)
        if self.fail_render == intent.op:
            self.fail_render = None
            raise OSError("receipt rendering interrupted")
        return result


@pytest.fixture
def setup_context(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    monkeypatch.setenv("BTCLI_PROXIES_PATH", str(tmp_path / "proxies.json"))
    controller = Keypair.create_from_seed(bytes([111]) * 32, 1).ss58_address
    chain = _SetupChain(controller)
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
    monkeypatch.setattr(ctx, "run", lambda fn: asyncio.run(fn(chain)))
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


def _protect(ctx, *, allow=(), delay=None):
    protected_cli.run_protect(ctx, delay_raw=delay, allow=list(allow), disallow=[], allow_key=None)


def _record():
    return pa.get_by_controller("owner", genesis_hash=GENESIS)


def _submitted(chain, op):
    return [intent for intent in chain.submitted if intent.op == op]


# PR #3305: changing an allow must never silently shorten a seven-day protection window.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_omitted_delay_preserves_live_seven_day_window(setup_context):
    ctx, chain = setup_context
    _protect(ctx, delay="7d")
    delay = pa.parse_delay_to_blocks("7d")
    assert chain.grants[0]["delay"] == delay
    # The cached value may be stale; the currently active grant is authoritative.
    pa.upsert_protected({**_record(), "delay_blocks": 36})
    _protect(ctx, allow=["lock"])
    queued = ann.list_for_wallet("owner", genesis_hash=GENESIS)
    assert [row["category"] for row in queued] == ["allow:Lock"]
    assert queued[0]["delay_blocks"] == delay
    assert _record()["delay_blocks"] == delay


# PR #3305: interrupted setup must not enable routing or abandon the requested asset migration.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize(
    "failure",
    [
        "allow",
        "allow_receipt",
        "swap",
        "swap_receipt",
        "swap_status",
        "book",
        "create_render",
        "swap_render",
    ],
)
def test_setup_resumes_missing_steps_without_duplicate_chain_calls(
    setup_context, monkeypatch, failure
):
    ctx, chain = setup_context
    if failure == "allow":
        chain.fail_allow = "Staking"
    elif failure == "allow_receipt":
        chain.lose_allow_receipt = "Staking"
    elif failure == "swap":
        chain.fail_swap = True
    elif failure == "swap_status":
        chain.lose_swap_status = True
    elif failure == "swap_receipt":
        chain.lose_swap_receipt = True
    elif failure == "create_render":
        chain.fail_render = "create_pure_proxy"
    elif failure == "swap_render":
        chain.fail_render = "announce_coldkey_swap"
    else:
        original_add = cfg.add_proxy

        def fail_book_once(entry):
            monkeypatch.setattr(cfg, "add_proxy", original_add)
            raise OSError("book write interrupted")

        monkeypatch.setattr(cfg, "add_proxy", fail_book_once)
    with pytest.raises((typer.Exit, OSError)):
        _protect(ctx, allow=["lock", "unstake"], delay="7d")
    interrupted = _record()
    assert interrupted is not None
    assert pa.protected_setup_pending(interrupted)
    with pytest.raises(typer.Exit):
        protected_cli.maybe_route_protected(
            ctx,
            Transfer(dest_ss58=chain.controller, amount_tao=1),
            proxy_for=None,
            user_set_proxy=False,
        )

    # Retry without restating the original options; saved intent and live facts
    # must recover both desired grants and the coldkey migration.
    _protect(ctx)
    assert len(_submitted(chain, "create_pure_proxy")) == 1
    assert len(_submitted(chain, "announce_proxy_call")) == 2
    assert len(_submitted(chain, "announce_coldkey_swap")) == 1
    assert len(chain.announcements) == 2
    assert not pa.protected_setup_pending(_record())
    assert _record()["pending_allows"] == ["Lock", "Staking"]
    assert _record()["coldkey_swap"]["execute_block"] == 200
    assert _record()["delay_blocks"] == pa.parse_delay_to_blocks("7d")


def test_setup_reuses_book_pure_and_its_original_delay(setup_context):
    ctx, chain = setup_context
    delay = pa.parse_delay_to_blocks("7d")
    chain.grants = [{"delegate": chain.controller, "proxy_type": "Any", "delay": delay}]
    cfg.add_proxy(
        {
            "name": pa.default_protected_name("owner"),
            "address": chain.pure,
            "genesis_hash": GENESIS,
            "spawner": chain.controller,
            "height": 100,
            "ext_index": 0,
            "proxy_type": "Any",
            "delay": delay,
            "index": 0,
        }
    )
    _protect(ctx)
    assert not _submitted(chain, "create_pure_proxy")
    assert len(_submitted(chain, "announce_coldkey_swap")) == 1
    assert _record()["delay_blocks"] == delay
    assert not pa.protected_setup_pending(_record())


def test_setup_waits_for_multisig_migration_without_duplicate_approval(setup_context):
    ctx, chain = setup_context
    chain.swap_approval_only = True
    _protect(ctx)
    assert pa.protected_setup_pending(_record())
    assert _record()["coldkey_swap"]["status"] == "awaiting_approval"
    _protect(ctx)
    assert len(_submitted(chain, "announce_coldkey_swap")) == 1
    assert pa.protected_setup_pending(_record())
    chain.announce_swap()
    _protect(ctx)
    assert len(_submitted(chain, "announce_coldkey_swap")) == 1
    assert not pa.protected_setup_pending(_record())


def test_cancelled_multisig_migration_can_be_retried(setup_context):
    ctx, chain = setup_context
    chain.swap_approval_only = True
    _protect(ctx)
    assert _record()["coldkey_swap"]["multisig_announcement"] == {
        "account_ss58": chain.controller,
        "call_hash": "0x" + "e3" * 32,
    }
    chain.pending_multisig = False
    chain.swap_approval_only = False
    _protect(ctx)
    assert len(_submitted(chain, "create_pure_proxy")) == 1
    assert len(_submitted(chain, "announce_coldkey_swap")) == 2
    assert not pa.protected_setup_pending(_record())


def test_unreadable_multisig_migration_stays_pending_without_resubmission(setup_context):
    ctx, chain = setup_context
    chain.swap_approval_only = True
    _protect(ctx)
    chain.fail_multisig_read = True
    _protect(ctx)
    assert len(_submitted(chain, "announce_coldkey_swap")) == 1
    assert pa.protected_setup_pending(_record())
    assert "approval state is unknown" in ctx.output.messages[-1]


def test_setup_recognizes_allow_executed_before_retry(setup_context):
    ctx, chain = setup_context
    chain.fail_allow = "Staking"
    with pytest.raises(typer.Exit):
        _protect(ctx, allow=["lock", "unstake"])
    chain.grants.append({"delegate": chain.controller, "proxy_type": "Lock", "delay": 0})
    chain.announcements.clear()
    _protect(ctx)
    assert len(_submitted(chain, "announce_proxy_call")) == 2
    assert _record()["allows"] == ["Lock"]
    assert _record()["pending_allows"] == ["Staking"]
    assert not pa.protected_setup_pending(_record())


@pytest.mark.parametrize("existing", [False, True])
def test_setup_dry_run_does_not_write_or_submit(setup_context, existing):
    ctx, chain = setup_context
    if existing:
        chain.fail_swap = True
        with pytest.raises(typer.Exit):
            _protect(ctx, allow=["lock"])
    before = (
        pa.load_protected(),
        cfg.load_proxies(),
        ann.load_announcements(),
        list(chain.submitted),
    )
    ctx.dry_run = True
    _protect(ctx, allow=["unstake"])
    assert (
        pa.load_protected(),
        cfg.load_proxies(),
        ann.load_announcements(),
        chain.submitted,
    ) == before
    assert "Dry run:" in ctx.output.messages[-1]
