"""Four schemes across three validators, using production slot timing."""

from __future__ import annotations

import asyncio
import os
import secrets
from types import SimpleNamespace

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from bittensor.sp_core import HASHED_CRYPTO_TYPES, Keypair
from tests.harness.samples import dev_wallet

PEERS = os.getenv("E2E_WALLET_PEERS", "").split(",")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(len(PEERS) != 3, reason="requires three activated local validators"),
]


@pytest.mark.parametrize("code,sponsor", [(0, "Alice"), (1, "Bob"), (4, "Charlie"), (5, "Dave")])
async def test_cross_validator_shield_and_external_nonce_advancement(code, sponsor):
    clients = [bt.Client(url, fallback_endpoints=[], archive_endpoints=[]) for url in PEERS]
    async with clients[0] as a, clients[1] as b, clients[2] as c:
        assert len(set(PEERS)) == 3
        assert await a.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
        assert await a.query(("AdminUtils", "HashedAccountsEnabled")) is True
        assert await a.constant(("Timestamp", "MinimumPeriod")) == 6000
        rpc = a._substrate.raw._session.request
        genesis = await rpc("chain_getBlockHash", [0])
        for client in clients[1:]:
            assert (
                await client._substrate.raw._session.request("chain_getBlockHash", [0]) == genesis
            )
        key = Keypair.create_from_seed(secrets.token_bytes(32), code)
        wallet = SimpleNamespace(coldkey=key, coldkeypub=key, hotkey=key)
        address = receiving_address(key, genesis)
        result = await asyncio.wait_for(
            a.execute(bt.intents.Transfer(address, 50), dev_wallet(f"//{sponsor}")), 120
        )
        assert result.success, result.to_dict()
        initial = await a.query(bt.storage.System.Account, [key.ss58_address])
        for client in clients:
            result = await asyncio.wait_for(
                client.submit_call(bt.calls.System.remark(remark="0x0102"), wallet), 120
            )
            assert result.success, result.to_dict()
        destination = dev_wallet("//Ferdie").coldkey.ss58_address
        result = await asyncio.wait_for(
            a.submit_shielded(
                bt.intents.Transfer(destination, 1), wallet, wait_for_finalization=True
            ),
            180,
        )
        assert result.success, result.to_dict()
        result = await asyncio.wait_for(
            b.submit_call(
                bt.calls.Balances.transfer_keep_alive(dest=destination, value=10**18),
                wallet,
                shielded=True,
            ),
            180,
        )
        assert not result.success and result.block_hash and result.error, result.to_dict()
        # This client last signed before both Shield pairs advanced the account.
        result = await asyncio.wait_for(
            c.submit_call(bt.calls.System.remark(remark="0x03"), wallet), 120
        )
        assert result.success, result.to_dict()
        for client in clients:
            account = await client.query(bt.storage.System.Account, [key.ss58_address])
            assert account["nonce"] == initial["nonce"] + 8
            if code in HASHED_CRYPTO_TYPES:
                record = await client.query(("HashedAccounts", "Accounts"), [key.ss58_address])
                assert record["generation"] == 8
