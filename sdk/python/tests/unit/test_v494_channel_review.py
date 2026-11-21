"""Focused coverage for the v494 channel-read review repair."""

import asyncio
from types import SimpleNamespace

import pytest
from typer.testing import CliRunner

from bittensor import storage as st
from bittensor.balance import Balance
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.reads.identity import (
    ChannelCommitment,
    channel_commitment,
    revealed_channel_commitment,
)
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE_HOT


@pytest.mark.asyncio
async def test_explicit_revealed_read_distinguishes_history_from_active_absence():
    substrate = FakeSubstrate()
    substrate.seed(
        "Commitments",
        "ChannelRevealedCommitments",
        [2 * 4096 + 1, ALICE_HOT, 7],
        [(b"revealed plaintext", 321)],
    )
    client = Client("local", substrate=substrate)
    params = {"netuid": 1, "mechid": 2, "channel": 7, "hotkey_ss58": ALICE_HOT}
    combined = await channel_commitment(client, **params)
    assert combined is not None
    assert combined.has_active is False
    assert combined.block == 0
    assert await revealed_channel_commitment(client, **params) == [
        {"data": "revealed plaintext", "block": 321}
    ]
    assert await revealed_channel_commitment(client, **{**params, "channel": 8}) is None


@pytest.mark.asyncio
async def test_channel_read_marks_active_and_revealed_records_separately():
    substrate = FakeSubstrate()
    substrate.seed_runtime(
        "SubnetRegistrationRuntimeApi",
        "get_channel_commitment",
        {"deposit": 0, "block": 100, "info": {"fields": []}},
    )
    substrate.seed("Commitments", "ChannelRevealedCommitments", [4097, ALICE_HOT, 7], [])
    result = await channel_commitment(
        Client("local", substrate=substrate), netuid=1, channel=7, hotkey_ss58=ALICE_HOT
    )
    assert result is not None
    assert result.has_active is True
    assert result.block == 100
    assert result.revealed == []


def test_commitments_show_labels_active_and_revealed_records(monkeypatch):
    details = []
    ctx = SimpleNamespace(
        resolve_address=lambda *_: ALICE_HOT,
        run=lambda work: asyncio.run(
            work(SimpleNamespace(read=lambda name, **params: _channel_read(name, params)))
        ),
        output=SimpleNamespace(
            detail=lambda title, fields, json_fields=None: details.append(
                (title, fields, json_fields)
            )
        ),
    )
    monkeypatch.setattr("bittensor.cli.commands.commitments.ctx_of", lambda _: ctx)
    result = CliRunner().invoke(
        app,
        [
            "commitments",
            "show",
            "--netuid",
            "1",
            "--mechid",
            "2",
            "--channel",
            "7",
            "--hotkey",
            ALICE_HOT,
        ],
        obj=SimpleNamespace(),
    )
    assert result.exit_code == 0, result.output
    assert details == [
        (
            "active channel commitment",
            {"block": 100, "deposit": Balance.from_rao(0), "data": "active", "fields": []},
            None,
        ),
        (
            "revealed channel commitment",
            {"0": {"data": "revealed", "block": 321}},
            [{"data": "revealed", "block": 321}],
        ),
    ]


async def _channel_read(name, params):
    assert params == {"netuid": 1, "mechid": 2, "channel": 7, "hotkey_ss58": ALICE_HOT}
    if name == "channel_commitment":
        return ChannelCommitment(
            block=100,
            deposit=Balance.from_rao(0),
            data="active",
            fields=[],
            revealed=[{"data": "revealed", "block": 321}],
            has_active=True,
        )
    assert name == "revealed_channel_commitment"
    return [{"data": "revealed", "block": 321}]


def test_revealed_channel_storage_descriptor_covers_the_new_read():
    assert st.Commitments.ChannelRevealedCommitments.name == "ChannelRevealedCommitments"
