"""The ``finalized_block`` read across every facade (issue #3068).

Mirrors the ``block`` surface: an async method on ``Client``, a blocking
property on ``SyncClient`` (inherited by ``Subtensor``), both reading the
chain fresh on every access.
"""

from __future__ import annotations

from bittensor._subtensor import Subtensor
from bittensor.client import Client
from bittensor.sync import SyncClient
from tests.harness.fake_substrate import FakeSubstrate


async def test_client_finalized_block_reads_finalized_head() -> None:
    fake = FakeSubstrate()
    client = Client("local", substrate=fake)
    assert await client.finalized_block() == 98
    assert await client.finalized_block() <= await client.block()


def test_sync_client_finalized_block_property_reads_the_chain_every_access() -> None:
    fake = FakeSubstrate()
    client = SyncClient("local", substrate=fake)
    try:
        assert client.finalized_block <= client.block
        # No caching: a moved finalized head must show up on the next access.
        fake.finalized_block = 105
        assert client.finalized_block == 105
    finally:
        client.close()


def test_subtensor_finalized_block_property() -> None:
    st = Subtensor("local", substrate=FakeSubstrate())
    try:
        assert st.finalized_block == 98
    finally:
        st.close()
