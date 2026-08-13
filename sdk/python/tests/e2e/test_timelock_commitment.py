"""Live coverage for the Python timelocked-commitment submission path."""

from __future__ import annotations

import asyncio
import os
import time

import pytest

import bittensor as bt
from tests.harness.samples import dev_wallet

E2E_ENDPOINT = os.getenv("E2E_ENDPOINT")

pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not E2E_ENDPOINT, reason="requires E2E_ENDPOINT with a writable localnet"),
]


def _chain_bytes(value: bytes | str | list[int]) -> bytes:
    if isinstance(value, str):
        return bytes.fromhex(value[2:]) if value.startswith("0x") else value.encode()
    return bytes(value)


async def test_timelocked_commitment_reveals_via_python_sdk() -> None:
    alice = dev_wallet()
    hotkey = alice.hotkey.ss58_address
    plaintext = b"python-sdk-timelock-e2e"

    async with bt.Client(E2E_ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        registered = await client.execute_tool("register_subnet", {}, alice)
        assert registered.success, registered.message
        netuid = max(subnet.netuid for subnet in await client.subnets.all())

        deadline = time.monotonic() + 30
        while True:
            stored_round = int(await client.query(bt.storage.Drand.LastStoredRound) or 0)
            if stored_round > 0:
                break
            assert time.monotonic() < deadline, "drand bridge did not initialize"
            await asyncio.sleep(0.1)

        reveal_round = max(stored_round, bt.timelock.current_round()) + 2
        sealed = bt.timelock.encrypt(plaintext, reveal_round=reveal_round)
        encrypted = sealed.encrypted
        assert encrypted != bytes(sealed), "test must submit the inner ciphertext, not the envelope"

        call = bt.calls.Commitments.set_commitment(
            netuid=netuid,
            info={
                "fields": [
                    [
                        {
                            "TimelockEncrypted": {
                                "encrypted": encrypted,
                                "reveal_round": sealed.reveal_round,
                            }
                        }
                    ]
                ]
            },
        )
        submitted = await client.submit_call(call, alice, signer="hotkey")
        assert submitted.success, submitted.message

        deadline = time.monotonic() + 45
        while True:
            revealed = await client.query(
                bt.storage.Commitments.RevealedCommitments, [netuid, hotkey]
            )
            if any(
                _chain_bytes(data) == plaintext and int(reveal_block) > 0
                for data, reveal_block in revealed or []
            ):
                break
            assert time.monotonic() < deadline, (
                f"commitment did not reveal at drand round {sealed.reveal_round}; "
                f"RevealedCommitments={revealed!r}"
            )
            await asyncio.sleep(0.1)

        pending = await client.query(bt.storage.Commitments.CommitmentOf, [netuid, hotkey])
        assert pending is None
