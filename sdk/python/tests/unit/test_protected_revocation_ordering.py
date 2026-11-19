"""Revocations must remain effective when other announced grants execute first."""

from __future__ import annotations

import asyncio
from copy import deepcopy
from hashlib import blake2b
from types import SimpleNamespace

import pytest

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import prompt
from bittensor.cli import protected as protected_cli
from bittensor.cli.context import AppContext
from bittensor.intents import AddProxy, AnnounceProxyCall, ExecuteProxyAnnounced, RemoveStake
from bittensor.intents.proxy import RemoveProxyAnnouncement
from bittensor.sp_core import Keypair
from tests.unit.test_protected_timing_regressions import GENESIS, _payload, _TimingChain


class _RevocationChain(_TimingChain):
    def _apply(self, encoded):
        if encoded["op"] == "remove_proxies":
            self.grants.clear()
            return
        snapshot = deepcopy(self.grants)
        try:
            super()._apply(encoded)
        except (AssertionError, ValueError):
            # Utility.batch_all rolls back every child if any regrant fails.
            self.grants = snapshot
            raise

    def submit(self, intent, **kwargs):
        if not isinstance(intent, RemoveProxyAnnouncement):
            return super().submit(intent, **kwargs)
        before = len(self.announcements)
        self.announcements = [
            row
            for row in self.announcements
            if row["real"] != intent.real_ss58 or row["call_hash"] != intent.call_hash
        ]
        assert len(self.announcements) < before
        return SimpleNamespace(
            success=True, events=[], extrinsic_id=f"{self.current_block}-0", data={}
        )


@pytest.fixture
def revocation_setup(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_PROXY_ANNOUNCEMENTS_PATH", str(tmp_path / "announcements.json"))
    controller = Keypair.create_from_seed(bytes([101]) * 32, 1).ss58_address
    pure = Keypair.create_from_seed(bytes([102]) * 32, 1).ss58_address
    new = Keypair.create_from_seed(bytes([103]) * 32, 1).ss58_address
    separate = Keypair.create_from_seed(bytes([104]) * 32, 1).ss58_address
    chain = _RevocationChain(controller, pure, delay=36)
    output = SimpleNamespace(
        json_mode=True,
        value=lambda *_a: None,
        message=lambda *_a: None,
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
    monkeypatch.setattr(
        ctx, "resolve_address", lambda _param, value: new if value == "new" else value
    )
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
    return ctx, chain, record, new, separate


def _record():
    return pa.get_by_controller("controller", genesis_hash=GENESIS)


def _update(ctx, *, allow=(), disallow=()):
    protected_cli._protect_update(
        ctx,
        _record(),
        delay_blocks=36,
        allow_types=list(allow),
        disallow_types=list(disallow),
        allow_key_ss58=None,
    )


# PR #3305: rotation must revoke old-controller authority granted after its announcement snapshot.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("separate_allow", [False, True])
def test_rotation_revokes_intervening_old_grant_and_preserves_separate_allow(
    revocation_setup, separate_allow
):
    ctx, chain, record, new, separate = revocation_setup
    separate_grant = {"delegate": separate, "proxy_type": "Staking", "delay": 0}
    if separate_allow:
        chain.grants.append(separate_grant)
        record = {**record, "allows": ["Staking"], "allow_key_ss58": separate}
        pa.upsert_protected(record)

    protected_cli.run_rotate(ctx, to_wallet="new")
    rotation = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    chain.current_block = 101
    if separate_allow:
        # A second client grants the old controller authority while the
        # configured separate allow key remains authorized independently.
        protected_cli._announce_as_protected(
            ctx,
            _record(),
            AddProxy(delegate_ss58=chain.controller, proxy_type="Lock", delay=0),
            category="allow:Lock",
        )
    else:
        _update(ctx, allow=["Lock"])
    later_allow = ann.list_for_wallet("controller", genesis_hash=GENESIS)[-1]
    chain.current_block = 137
    protected_cli.run_execute_protected(ctx, index=later_allow["index"])
    assert {"delegate": chain.controller, "proxy_type": "Lock", "delay": 0} in chain.grants

    protected_cli.run_execute_protected(ctx, index=rotation["index"])
    assert not any(grant["delegate"] == chain.controller for grant in chain.grants)
    assert {"delegate": new, "proxy_type": "Any", "delay": 36} in chain.grants
    assert (separate_grant in chain.grants) is separate_allow
    assert pa.get_by_controller("controller", genesis_hash=GENESIS) is None
    moved = pa.get_by_controller("new", genesis_hash=GENESIS)
    assert moved["controller_ss58"] == new
    assert moved["allows"] == (["Staking"] if separate_allow else [])
    assert moved["pending_allows"] == []


# PR #3305: disallow must consume younger duplicate grants before reporting authority revoked.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_disallow_cancels_younger_duplicate_after_older_allow_executes(revocation_setup):
    ctx, chain, _initial, _new, _separate = revocation_setup
    _update(ctx, allow=["Lock"])
    allow = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    chain.current_block = 110
    chain.submit(AnnounceProxyCall(real_ss58=chain.pure, call_hash=allow["call_hash"]))
    chain.current_block = 136
    protected_cli.run_execute_protected(ctx, index=allow["index"])
    lock = {"delegate": chain.controller, "proxy_type": "Lock", "delay": 0}
    assert lock in chain.grants
    assert [row["height"] for row in chain.announcements] == [110]
    # A previous client already cleared this flag after the older execution.
    # Disallow must inspect commitments independently of this local cache.
    pa.upsert_protected({**_record(), "pending_allows": []})
    _update(ctx, disallow=["Lock"])
    assert not any(row["call_hash"] == allow["call_hash"] for row in chain.announcements)
    removals = [
        row
        for row in ann.list_for_wallet("controller", genesis_hash=GENESIS)
        if row["category"] == "disallow:Lock"
    ]
    assert len(removals) == 1
    chain.current_block = 172
    protected_cli.run_execute_protected(ctx, index=removals[0]["index"])
    assert lock not in chain.grants
    chain.current_block = 200
    with pytest.raises(AssertionError, match="at least one matured commitment"):
        chain.submit(
            ExecuteProxyAnnounced(
                delegate_ss58=chain.controller,
                real_ss58=chain.pure,
                inner_op=allow["inner_op"],
                inner_args=allow["inner_args"],
                announced_call_data=allow["call_data"],
            )
        )
    assert lock not in chain.grants


# PR #3305: revoking a separate allow key must cancel a queued rotation that would regrant it.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_disallow_cancels_rotation_that_would_restore_separate_allow(revocation_setup):
    ctx, chain, record, new, separate = revocation_setup
    lock = {"delegate": separate, "proxy_type": "Lock", "delay": 0}
    chain.grants.append(lock)
    pa.upsert_protected(
        {**record, "allow_key_ss58": separate, "allows": ["Lock"], "pending_allows": []}
    )
    protected_cli.run_rotate(ctx, to_wallet="new")
    rotation = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]

    _update(ctx, disallow=["Lock"])
    assert not any(row["call_hash"] == rotation["call_hash"] for row in chain.announcements)
    assert (
        ann.get_by_index("controller", rotation["index"], genesis_hash=GENESIS)["status"]
        == "cancelled"
    )
    removal = next(
        row
        for row in ann.list_for_wallet("controller", genesis_hash=GENESIS)
        if row["category"] == "disallow:Lock"
    )
    chain.current_block = 136
    protected_cli.run_execute_protected(ctx, index=removal["index"])
    assert lock not in chain.grants
    assert {"delegate": chain.controller, "proxy_type": "Any", "delay": 36} in chain.grants
    assert not any(grant["delegate"] == new for grant in chain.grants)
    with pytest.raises(AssertionError, match="at least one matured commitment"):
        chain.submit(
            ExecuteProxyAnnounced(
                delegate_ss58=chain.controller,
                real_ss58=chain.pure,
                inner_op=rotation["inner_op"],
                inner_args=rotation["inner_args"],
                announced_call_data=rotation["call_data"],
            )
        )
    assert lock not in chain.grants


# PR #3305: preserving separate Validate/Weights grants must not block controller rotation.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_rotation_preserves_separate_hotkey_scopes_with_one_pure_origin(revocation_setup):
    ctx, chain, record, new, separate = revocation_setup
    scopes = ["Staking", "Validate", "Weights"]
    grants = [{"delegate": separate, "proxy_type": scope, "delay": 0} for scope in scopes]
    chain.grants.extend(grants)
    pa.upsert_protected({**record, "allows": scopes, "allow_key_ss58": separate})

    protected_cli.run_rotate(ctx, to_wallet="new")
    rotation = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    chain.current_block = 136
    protected_cli.run_execute_protected(ctx, index=rotation["index"])

    assert not any(grant["delegate"] == chain.controller for grant in chain.grants)
    assert {"delegate": new, "proxy_type": "Any", "delay": 36} in chain.grants
    assert all(grant in chain.grants for grant in grants)
    moved = pa.get_by_controller("new", genesis_hash=GENESIS)
    assert sorted(moved["allows"]) == sorted(scopes)
    assert moved["allow_key_ss58"] == separate


# PR #3305: failed rotation regrants must atomically restore control of the funded pure account.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_rotation_rolls_back_every_grant_if_replacement_fails(revocation_setup, monkeypatch):
    ctx, chain, record, new, separate = revocation_setup
    staking = {"delegate": separate, "proxy_type": "Staking", "delay": 0}
    chain.grants.append(staking)
    initial_grants = deepcopy(chain.grants)
    pa.upsert_protected({**record, "allows": ["Staking"], "allow_key_ss58": separate})
    protected_cli.run_rotate(ctx, to_wallet="new")
    rotation = ann.list_for_wallet("controller", genesis_hash=GENESIS)[0]
    original_apply = chain._apply
    state_at_failure = []

    def fail_separate_regrant(encoded):
        if encoded["op"] == "add_proxy" and encoded["delegate_ss58"] == separate:
            state_at_failure.extend(deepcopy(chain.grants))
            raise ValueError("replacement proxy deposit could not be reserved")
        original_apply(encoded)

    monkeypatch.setattr(chain, "_apply", fail_separate_regrant)
    chain.current_block = 136
    with pytest.raises(ValueError, match="deposit could not be reserved"):
        protected_cli.run_execute_protected(ctx, index=rotation["index"])

    assert state_at_failure == [{"delegate": new, "proxy_type": "Any", "delay": 36}]
    assert chain.grants == initial_grants
    assert _record()["controller_ss58"] == chain.controller
    assert pa.get_by_controller("new", genesis_hash=GENESIS) is None


# PR #3305: stale Staking cache must not block withdrawals by selecting an unusable instant grant.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("live_grant", ["removed", "separate", "delayed", "controller"])
def test_staking_routes_from_current_controller_authority(revocation_setup, live_grant):
    ctx, chain, record, _new, separate = revocation_setup
    record = {**record, "allows": ["Staking"]}
    if live_grant != "removed":
        delegate = separate if live_grant == "separate" else chain.controller
        chain.grants.append(
            {
                "delegate": delegate,
                "proxy_type": "Staking",
                "delay": 5 if live_grant == "delayed" else 0,
            }
        )
    if live_grant == "separate":
        record["allow_key_ss58"] = separate
    pa.upsert_protected(record)
    withdrawal = RemoveStake(hotkey_ss58=separate, netuid=1, amount_alpha=1)

    route = protected_cli.maybe_route_protected(
        ctx, withdrawal, proxy_for=None, user_set_proxy=False
    )
    if live_grant == "controller":
        assert route == (chain.pure, "Staking", False)
        assert chain.announcements == []
    else:
        assert route == (None, None, True)
        pending = ann.list_for_wallet("controller", genesis_hash=GENESIS)
        assert len(pending) == 1
        assert pending[0]["inner_op"] == "remove_stake"
        assert chain.announcements[0]["call_hash"] == pending[0]["call_hash"]
    assert _record()["allows"] == (["Staking"] if live_grant in {"separate", "controller"} else [])
