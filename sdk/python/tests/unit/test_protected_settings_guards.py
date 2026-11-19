"""Settings changes preserve authority tracking and previews leave no durable state."""

from __future__ import annotations

from copy import deepcopy

import pytest
import typer

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import protected
from bittensor.intents import AddProxy, Batch
from tests.unit.test_protected_revocation_ordering import revocation_setup as revocation_setup
from tests.unit.test_protected_timing_regressions import GENESIS


def _update(ctx, record, *, allow=(), disallow=(), key=None):
    return protected._protect_update(
        ctx,
        record,
        delay_blocks=36,
        allow_types=list(allow),
        disallow_types=list(disallow),
        allow_key_ss58=key,
    )


@pytest.mark.parametrize("nested", [False, True])
def test_retarget_rejects_grants_queued_by_ordinary_proxy_commands(revocation_setup, nested):
    ctx, chain, record, new, old_key = revocation_setup
    record = {**record, "allow_key_ss58": old_key}
    pa.upsert_protected(record)
    intent = AddProxy(delegate_ss58=old_key, proxy_type="Lock", delay=0)
    if nested:
        intent = Batch(intents=[intent.to_dict()])
    protected._submit_protected_announcement(ctx, intent, record)
    queued = ann.load_announcements()
    assert queued[0]["category"] not in {"allow:Lock", "allow:Staking"}
    # Older clients did not classify ordinary proxy commands as pending allows.
    stored = pa.get_by_controller("controller", genesis_hash=GENESIS)
    pa.upsert_protected({**stored, "pending_allows": []})

    with pytest.raises(typer.Exit) as error:
        _update(ctx, record, key=new)

    assert error.value.exit_code == 2
    assert pa.get_by_controller("controller", genesis_hash=GENESIS)["allow_key_ss58"] == old_key
    assert chain.announcements
    # The original target remains tracked so revocation cancels its commitment.
    _update(ctx, pa.get_by_controller("controller", genesis_hash=GENESIS), disallow=["Lock"])
    assert chain.announcements == []
    _update(ctx, pa.get_by_controller("controller", genesis_hash=GENESIS), key=new)
    assert pa.get_by_controller("controller", genesis_hash=GENESIS)["allow_key_ss58"] == new


def test_dry_run_retarget_does_not_affect_next_real_grant(revocation_setup):
    ctx, _chain, record, new, old_key = revocation_setup
    record = {**record, "allow_key_ss58": old_key, "pending_allows": []}
    pa.upsert_protected(record)
    before = pa.protected_path().read_bytes()
    ctx.dry_run = True

    _update(ctx, record, key=new)

    assert pa.protected_path().read_bytes() == before
    assert not ann.announcements_path().exists()
    ctx.dry_run = False
    _update(ctx, pa.get_by_controller("controller", genesis_hash=GENESIS), allow=["Lock"])
    queued = ann.list_for_wallet("controller", genesis_hash=GENESIS)
    assert queued[0]["inner_args"]["delegate_ss58"] == old_key


@pytest.mark.parametrize("change", ["allow", "disallow", "delay"])
def test_settings_preview_does_not_update_bindings_or_announcement_store(
    revocation_setup, monkeypatch, change
):
    ctx, chain, record, _new, _separate = revocation_setup
    chain.grants.append({"delegate": chain.controller, "proxy_type": "Lock", "delay": 0})
    protected._submit_protected_announcement(
        ctx, AddProxy(delegate_ss58=chain.controller, proxy_type="Lock"), record
    )
    before_record = pa.protected_path().read_bytes()
    before_queue = ann.announcements_path().read_bytes()
    before_chain = deepcopy(chain.announcements)
    previews = []
    ctx.dry_run = True
    monkeypatch.setattr(ctx, "submit", lambda intent, **_kw: previews.append(intent))

    protected._protect_update(
        ctx,
        record,
        delay_blocks=72 if change == "delay" else 36,
        allow_types=["Staking"] if change == "allow" else [],
        disallow_types=["Lock"] if change == "disallow" else [],
        allow_key_ss58=None,
    )

    assert previews
    assert pa.protected_path().read_bytes() == before_record
    assert ann.announcements_path().read_bytes() == before_queue
    assert chain.announcements == before_chain


def test_dry_run_live_permission_refresh_is_read_only(revocation_setup):
    ctx, chain, record, _new, _separate = revocation_setup
    record = {**record, "allows": ["Staking"]}
    pa.upsert_protected(record)
    before = pa.protected_path().read_bytes()
    ctx.dry_run = True
    assert not any(grant["proxy_type"] == "Staking" for grant in chain.grants)

    refreshed = protected._refresh_live_allows(ctx, record)

    assert refreshed["allows"] == []
    assert pa.protected_path().read_bytes() == before


def test_dry_run_unknown_announcement_does_not_create_local_queue(revocation_setup):
    ctx, chain, record, new, _separate = revocation_setup
    chain.announcements.append({"real": chain.pure, "call_hash": "0x" + "77" * 32, "height": 100})
    before = pa.protected_path().read_bytes()
    ctx.dry_run = True

    with pytest.raises(typer.Exit) as error:
        _update(ctx, record, key=new)

    assert error.value.exit_code == 2
    assert pa.protected_path().read_bytes() == before
    assert not ann.announcements_path().exists()
