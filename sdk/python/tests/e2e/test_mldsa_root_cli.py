"""Root CLI wallet selection with real, competing stake positions.

Requires an isolated activated chain with root registration/staking configured
by the release harness. The test changes only its fresh wallets' positions.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from tests.e2e.test_mldsa_extended import Evidence
from tests.e2e.test_mldsa_release_costs import attributes, money, observe, setup, snapshot
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(
        not ENDPOINT, reason="requires an activated chain with root staking enabled"
    ),
]


@pytest.mark.parametrize("code,selector", [(7, "local"), (8, "receiving")])
async def test_root_cli_moves_selected_ms_wallet_without_touching_configured_wallet(
    tmp_path, code, selector
):
    report = Evidence(f"release-root-cli-wallet-selection-{code}")
    wallet_path = tmp_path / "wallets"
    configured = bt.Wallet("configured-A", path=str(wallet_path))
    configured.regenerate_coldkey(seed=os.urandom(32), use_password=False, suppress=True)
    selected = bt.Wallet("selected-B", path=str(wallet_path))
    selected.regenerate_coldkey(
        seed=os.urandom(32), crypto_type=code, use_password=False, suppress=True
    )
    source, destination = [Keypair.create_from_seed(os.urandom(32), 1) for _ in range(2)]
    selected_ref = selected.name if selector == "local" else receiving_address(selected.coldkeypub)
    env = dict(os.environ)
    for variable, filename in (
        ("BTCLI_CONFIG", "config.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_MULTISIG_CACHE", "multisig-cache.json"),
        ("BTCLI_SUBNET_NAMES_CACHE", "subnets.json"),
        ("BTCLI_TOKEN_SYMBOLS_CACHE", "symbols.json"),
    ):
        env[variable] = str(tmp_path / filename)

    async def cli(operation, *options):
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-m",
            "bittensor.cli.main",
            "--yes",
            "--json",
            "--network",
            ENDPOINT,
            "--wallet",
            configured.name,
            "--wallet-path",
            str(wallet_path),
            "root",
            operation,
            "--coldkey",
            selected_ref,
            "--no-mev-shield",
            *options,
            env=env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            out, err = await asyncio.wait_for(process.communicate(), 240)
        except BaseException:
            if process.returncode is None:
                process.kill()
                await process.communicate()
            raise
        return process.returncode, out.decode(), out.decode() + err.decode()

    async with (
        asyncio.timeout(1200),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        deposit = await setup(client)
        for wallet in (configured, selected):
            async with alice_submission():
                result = await client.execute(
                    bt.intents.Transfer(receiving_address(wallet.coldkeypub), 100),
                    dev_wallet(),
                    shielded=False,
                    max_registration_deposit=money(deposit),
                    wait_for_finalization=True,
                )
            await observe(client, report, "fund isolated local wallet", result)
        for hotkey in (source, destination):
            result = await client.execute(
                bt.intents.RootRegister(hotkey_ss58=hotkey.ss58_address),
                configured,
                shielded=False,
                wait_for_finalization=True,
            )
            await observe(client, report, "register independent root validator", result)
        for wallet, amount in ((configured, 5), (selected, 7)):
            result = await client.execute(
                bt.intents.AddStake(source.ss58_address, 0, amount),
                wallet,
                shielded=False,
                wait_for_finalization=True,
            )
            await observe(client, report, "both wallets stake on the same validator", result)

        async def positions(wallet):
            rows = await client.read(
                "stake_for_coldkey", coldkey_ss58=wallet.coldkeypub.ss58_address
            )
            return {row.hotkey: row.stake.rao for row in rows if row.netuid == 0 and row.stake.rao}

        async def state():
            return {
                "configured_account": await client.query(
                    bt.storage.System.Account, [configured.coldkeypub.ss58_address]
                ),
                "configured_stake": await positions(configured),
                "selected": await snapshot(client, selected.coldkeypub),
                "selected_stake": await positions(selected),
            }

        before = await state()
        assert before["configured_stake"] == {source.ss58_address: 5 * 10**9}
        assert before["selected_stake"] == {source.ss58_address: 7 * 10**9}
        code_claim, _, claim_text = await cli("claim", "--hotkey", source.ss58_address)
        assert code_claim == 1 and "no accrued yield to claim" in claim_text, claim_text
        assert await state() == before
        report.add("empty-yield claim refuses without signing either wallet", output=claim_text)

        status, output, text = await cli(
            "move", "--from", source.ss58_address, "--to", destination.ss58_address
        )
        assert status == 0, text
        result = json.loads(output)
        assert result["success"] and result["block_hash"] and result["extrinsic_id"], result
        height, index = map(int, result["extrinsic_id"].split("-"))
        # Rotating signers wait for finalized authority; fixed MS follows the
        # ordinary wallet CLI behavior and returns once its call is included.
        if code == 8:
            async with asyncio.timeout(120):
                while await client._substrate.finalized_block_number() < height:
                    await asyncio.sleep(1)
        assert await client._substrate.finalized_block_number() >= height
        assert await client._substrate.block_hash(height) == result["block_hash"]
        events = [
            entry
            for entry in await client._substrate.raw.get_events(result["block_hash"])
            if entry.get("extrinsic_idx") == index
        ]
        assert attributes(events, "SubtensorModule", "StakeMoved")
        assert attributes(events, "Utility", "BatchCompleted")
        after = await state()
        assert after["configured_account"] == before["configured_account"]
        assert after["configured_stake"] == before["configured_stake"]
        assert after["selected_stake"] == {destination.ss58_address: 7 * 10**9}
        assert after["selected"]["account"]["nonce"] == before["selected"]["account"]["nonce"] + 1
        if code == 7:
            assert (
                after["selected"]["authority"]["generation"]
                == before["selected"]["authority"]["generation"] + 1
            )
        else:
            assert before["selected"]["authority"] is after["selected"]["authority"] is None
        report.add(
            "claim and move ALL use selected B and preserve configured A",
            selector=selector,
            result=result,
            canonical_events=events,
            before=before,
            after=after,
        )
