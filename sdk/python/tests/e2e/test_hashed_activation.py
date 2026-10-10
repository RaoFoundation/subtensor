"""Root can toggle hashed/MS support without replacing runtime metadata."""

import os

import pytest

import bittensor as bt
from bittensor.hashed import hashed_accounts_enabled
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_ADMIN_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires a fresh isolated Alice-root chain"),
]


async def test_sudo_activation_is_live_and_root_only():
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        alice = dev_wallet()
        assert await client.query(bt.storage.Sudo.Key) == alice.coldkey.ss58_address
        assert await hashed_accounts_enabled(client._substrate) is False
        initial_version = await client._substrate.spec_version()
        call = bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=True)
        denied = await client.submit_call(call, alice)
        assert not denied.success, denied.to_dict()
        assert await hashed_accounts_enabled(client._substrate) is False
        for enabled in (True, False, True, False):
            inner = bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=enabled)
            result = await client.submit_call(
                bt.calls.Sudo.sudo(call=await client.compose(inner)), alice
            )
            assert result.success, result.to_dict()
            assert await client.query(bt.storage.AdminUtils.HashedAccountsEnabled) is enabled
            assert await hashed_accounts_enabled(client._substrate) is enabled
            assert await client._substrate.spec_version() == initial_version
