"""Fee-free registration through the production signed transaction pipeline."""

import asyncio
import os
import sys

import pytest

import bittensor as bt
from tests.harness.samples import dev_wallet

E2E_ENDPOINT = os.getenv("E2E_ENDPOINT")
POW_BACKEND = os.getenv("E2E_POW_BACKEND", "auto")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not E2E_ENDPOINT, reason="requires an Alice-root local development chain"),
]


async def test_pow_register_unfunded_coldkey_and_owner_toggle(tmp_path):
    alice = dev_wallet()
    newcomer = dev_wallet("//PowUnfunded", "//PowUnfunded//hot")
    st = bt.storage.SubtensorModule
    async with bt.Client(E2E_ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == alice.coldkey.ss58_address

        async def submit(call, *, root=False):
            if root:
                call = bt.calls.Sudo.sudo(call=await client.compose(call))
            result = await client.submit_call(call, alice)
            assert result.success, result.message

        await submit(bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0), root=True)
        await submit(bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0), root=True)
        await submit(bt.calls.SubtensorModule.register_network(hotkey=alice.hotkey.ss58_address))
        netuid = max(subnet.netuid for subnet in await client.subnets.all())
        await submit(bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0), root=True)
        await submit(bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0), root=True)
        await submit(
            bt.calls.AdminUtils.sudo_set_min_difficulty(netuid=netuid, min_difficulty=1), root=True
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_difficulty(netuid=netuid, difficulty=1), root=True
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_burn_increase_mult(
                netuid=netuid, burn_increase_mult={"bits": 2 << 64}
            )
        )
        # Pin burn to its floor so block decay cannot obscure cross-route bumps.
        burn_floor = await client.query(st.Burn, [netuid])
        await submit(
            bt.calls.AdminUtils.sudo_set_min_burn(netuid=netuid, min_burn=burn_floor), root=True
        )
        # Exercise the owner's signed administration path, not just sudo.
        await submit(
            bt.calls.AdminUtils.sudo_set_network_pow_registration_allowed(
                netuid=netuid, registration_allowed=True
            )
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                netuid=netuid, registration_allowed=False
            )
        )
        result = await client.execute_tool(
            "set_hyperparameter",
            {"netuid": netuid, "name": "epoch_consensus", "value": "Null"},
            alice,
        )
        assert result.success, result.message
        result = await client.execute_tool(
            "set_hyperparameter",
            {"netuid": netuid, "name": "max_allowed_uids", "value": 2500},
            alice,
        )
        assert result.success, result.message
        assert await client.query(st.MaxAllowedUids, [netuid]) == 2500
        coldkey = newcomer.coldkey.ss58_address
        hotkey = newcomer.hotkey.ss58_address
        account = await client.query(bt.storage.System.Account, [coldkey])
        assert int(account["data"]["free"]) == 0
        burn = await client.query(st.Burn, [netuid])
        population = await client.query(st.SubnetworkN, [netuid])
        intent = await client.mine_pow_registration(
            netuid, hotkey, coldkey, workers=1, max_seconds=30, backend=POW_BACKEND
        )
        result = await client.execute(intent, newcomer)
        assert result.success, result.message
        account = await client.query(bt.storage.System.Account, [coldkey])
        assert int(account["data"]["free"]) == 0
        assert await client.query(st.Owner, [hotkey]) == coldkey
        assert await client.query(st.Uids, [netuid, hotkey]) is not None
        assert await client.query(st.SubnetworkN, [netuid]) == population + 1
        # PoW-only admission must preserve burn price exactly.
        assert await client.query(st.Burn, [netuid]) == burn
        assert await client.query(st.MinerCollateral, [netuid, hotkey, coldkey]) is None
        assert await client.query(st.LastPowRegistrationBlock, [hotkey]) == intent.work_block
        if result.fee is not None:
            assert result.fee.rao == 0

        # Run the real CLI against the same node with a separate unfunded wallet.
        cli_wallet = bt.wallets.create(
            name="pow-miner",
            path=str(tmp_path / "wallets"),
            use_password=False,
            on_mnemonic=lambda _role, _mnemonic: None,
        )
        environment = {**os.environ, "BTCLI_CONFIG": str(tmp_path / "cli.json")}
        for name in [
            "PROXIES",
            "ADDRESSES",
            "MULTISIGS",
            "MULTISIG_CACHE",
            "SUBNET_NAMES_CACHE",
            "TOKEN_SYMBOLS_CACHE",
        ]:
            environment[
                f"BTCLI_{name}_PATH"
                if name in ["PROXIES", "ADDRESSES", "MULTISIGS"]
                else f"BTCLI_{name}"
            ] = str(tmp_path / f"{name}.json")
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-m",
            "bittensor.cli.main",
            "subnets",
            "register",
            "--netuid",
            str(netuid),
            "--pow",
            "--pow-backend",
            POW_BACKEND,
            "--pow-workers",
            "1",
            "--pow-timeout",
            "30",
            "--network",
            E2E_ENDPOINT,
            "--wallet",
            "pow-miner",
            "--wallet-path",
            str(tmp_path / "wallets"),
            "--yes",
            "--json",
            env=environment,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=90)
        assert process.returncode == 0, stdout.decode() + stderr.decode()
        cli_coldkey = cli_wallet.coldkeypub.ss58_address
        cli_hotkey = cli_wallet.hotkey.ss58_address
        assert await client.query(st.Uids, [netuid, cli_hotkey]) is not None
        account = await client.query(bt.storage.System.Account, [cli_coldkey])
        assert int(account["data"]["free"]) == 0
        assert await client.query(st.MinerCollateral, [netuid, cli_hotkey, cli_coldkey]) is None
        refused = await client.submit_call(
            bt.calls.AdminUtils.sudo_set_network_pow_registration_allowed(
                netuid=netuid, registration_allowed=False
            ),
            alice,
        )
        assert not refused.success
        assert await client.query(st.NetworkPowRegistrationAllowed, [netuid])
        await submit(
            bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                netuid=netuid, registration_allowed=True
            )
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_network_pow_registration_allowed(
                netuid=netuid, registration_allowed=False
            )
        )
        assert not await client.query(st.NetworkPowRegistrationAllowed, [netuid])
