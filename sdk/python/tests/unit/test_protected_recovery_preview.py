"""Announcement recovery must not persist a dry-run preview."""

from copy import deepcopy
from hashlib import blake2b

import pytest

from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import protected
from bittensor.intents import AddProxy, AnnounceProxyCall, Batch, RemoveProxy
from tests.unit.test_protected_revocation_ordering import revocation_setup as revocation_setup
from tests.unit.test_protected_timing_regressions import GENESIS, _payload


def _intent(chain, entrypoint):
    if entrypoint == "settings":
        return Batch(
            intents=[
                RemoveProxy(delegate_ss58=chain.controller, proxy_type="Any", delay=36).to_dict(),
                AddProxy(delegate_ss58=chain.controller, proxy_type="Any", delay=72).to_dict(),
            ]
        )
    return AddProxy(delegate_ss58=chain.controller, proxy_type="Lock", delay=0)


def _invoke(ctx, record, intent, entrypoint):
    if entrypoint == "settings":
        return protected._protect_update(
            ctx,
            record,
            delay_blocks=72,
            allow_types=[],
            disallow_types=[],
            allow_key_ss58=None,
        )
    return protected._submit_protected_announcement(ctx, intent, record)


def _seed_live_commitment(ctx, chain, intent, *, stored):
    call_hash = "0x" + blake2b(_payload(intent), digest_size=32).hexdigest()
    chain.announcements.append(
        {"real": chain.pure, "call_hash": call_hash, "height": chain.current_block}
    )
    if stored:
        ann.upsert_on_chain_announcement(
            controller_wallet=ctx.wallet_name,
            genesis_hash=GENESIS,
            delegate_ss58=chain.controller,
            real_ss58=chain.pure,
            call_hash=call_hash,
            delay_blocks=36,
            announced_at_block=chain.current_block,
        )
    return call_hash


def _files(path):
    return {
        str(file.relative_to(path)): (file.read_bytes(), file.stat().st_mtime_ns)
        for file in path.rglob("*")
        if file.is_file()
    }


@pytest.mark.parametrize("entrypoint", ["settings", "announcement"])
@pytest.mark.parametrize("stored", [False, True])
def test_identical_live_commitment_dry_run_keeps_journal_and_settings_read_only(
    revocation_setup, monkeypatch, tmp_path, entrypoint, stored
):
    ctx, chain, record, _new, _separate = revocation_setup
    intent = _intent(chain, entrypoint)
    call_hash = _seed_live_commitment(ctx, chain, intent, stored=stored)
    before_files = _files(tmp_path)
    before_chain = deepcopy(chain.announcements)
    before_grants = deepcopy(chain.grants)
    previews = []
    ctx.dry_run = True
    monkeypatch.setattr(ctx, "submit", lambda value, **_kwargs: previews.append(value))

    _invoke(ctx, record, intent, entrypoint)

    assert _files(tmp_path) == before_files
    assert chain.announcements == before_chain
    assert chain.grants == before_grants
    assert len(previews) == 1
    assert isinstance(previews[0], AnnounceProxyCall)
    assert previews[0].call_hash == call_hash
    assert previews[0].real_ss58 == chain.pure


@pytest.mark.parametrize("entrypoint", ["settings", "announcement"])
def test_identical_live_commitment_real_retry_recovers_plaintext_without_resubmitting(
    revocation_setup, monkeypatch, entrypoint
):
    ctx, chain, record, _new, _separate = revocation_setup
    intent = _intent(chain, entrypoint)
    call_hash = _seed_live_commitment(ctx, chain, intent, stored=False)
    before_chain = deepcopy(chain.announcements)
    submissions = []
    monkeypatch.setattr(ctx, "submit", lambda value, **_kwargs: submissions.append(value))

    _invoke(ctx, record, intent, entrypoint)

    rows = ann.list_for_wallet(ctx.wallet_name, genesis_hash=GENESIS)
    assert len(rows) == 1
    assert rows[0]["call_hash"] == call_hash
    assert rows[0]["call_data"] == "0x" + _payload(intent).hex()
    assert rows[0]["inner_op"] == intent.op
    assert rows[0]["status"] == "pending"
    assert not rows[0].get("plaintext_missing", False)
    assert chain.announcements == before_chain
    assert submissions == []
    if entrypoint == "settings":
        saved = pa.get_by_controller(ctx.wallet_name, genesis_hash=GENESIS)
        assert saved["pending_delay_blocks"] == 72
        assert saved["pending_delay_index"] == rows[0]["index"]
