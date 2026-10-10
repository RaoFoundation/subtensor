"""Composable scheme/mode lifecycle on an isolated, activated Alice-root development chain."""

from __future__ import annotations

import asyncio
import json
import os
import sys
import time

import pytest

import bittensor as bt
from bittensor._transport.errors import SubstrateRequestException
from bittensor.evm.addresses import h160_to_ss58
from bittensor.evm.keys import create_evm_key
from bittensor.hashed import descriptor_value
from bittensor.receiving import receiving_address
from bittensor.sp_core import HASHED_CRYPTO_TYPES
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


@pytest.mark.parametrize("scheme", ["ed", "sr", "ms"])
@pytest.mark.parametrize("mode", ["standard", "hashed"])
async def test_composable_wallet_lifecycle(tmp_path, scheme, mode):
    env = {**os.environ, "BTCLI_CONFIG": str(tmp_path / "config.json")}
    wallet_path = str(tmp_path / "wallets")

    async def cli(*args):
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-m",
            "bittensor.cli.main",
            "--yes",
            "--json",
            "--network",
            ENDPOINT,
            "--wallet",
            "four-types",
            "--wallet-path",
            wallet_path,
            *args,
            env=env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        out, err = await asyncio.wait_for(process.communicate(), 120)
        assert process.returncode == 0, out.decode() + err.decode()
        return json.loads(out)

    created = await cli(
        "wallet", "create", "--crypto-type", scheme, "--type", mode, "--no-password"
    )
    wallet = bt.Wallet("four-types", path=wallet_path)
    code = bt.wallets.parse_crypto_type(scheme, mode)
    assert wallet.coldkey.crypto_type == wallet.hotkey.crypto_type == code
    hashed = code in HASHED_CRYPTO_TYPES
    alice = dev_wallet()
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == alice.coldkey.ss58_address
        assert await client.query(("AdminUtils", "HashedAccountsEnabled")) is True
        genesis = await client._substrate.raw._session.request("chain_getBlockHash", [0])

        async def submit(call, signer=alice, *, role="coldkey", root=False, success=True):
            if root:
                call = bt.calls.Sudo.sudo(call=await client.compose(call))
            result = await client.submit_call(call, signer, signer=role)
            assert result.success == success, result.to_dict()
            return result

        async def free(key):
            account = await client.query(bt.storage.System.Account, [key.ss58_address])
            return int(account["data"]["free"])

        async def record(key):
            return await client.query(("HashedAccounts", "Accounts"), [key.ss58_address])

        async def fund(key, amount=10):
            address = receiving_address(key, genesis) if hashed else key.ss58_address
            result = await client.execute(bt.intents.Transfer(address, amount), alice)
            assert result.success, result.to_dict()

        # Storage registration has a sponsor; PoW admission itself remains fee-free.
        if hashed:
            for key in (wallet.coldkey, wallet.hotkey):
                await submit(
                    bt.calls.HashedAccounts.register(
                        descriptor=descriptor_value(bytes(key.hashed_descriptor))
                    )
                )
                assert (await record(key))["generation"] == 0
        assert await free(wallet.coldkey) == 0
        # Keep repeated test runs affordable using the normal root setting.
        await submit(bt.calls.AdminUtils.sudo_set_lock_reduction_interval(interval=1), root=True)
        await submit(
            bt.calls.Balances.force_set_balance(who=alice.coldkey.ss58_address, new_free=10**15),
            root=True,
        )
        await submit(bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0), root=True)
        await submit(bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0), root=True)
        await submit(bt.calls.SubtensorModule.register_network(hotkey=alice.hotkey.ss58_address))
        netuid = max(subnet.netuid for subnet in await client.subnets.all())
        for call in (
            bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0),
            bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
            bt.calls.AdminUtils.sudo_set_min_difficulty(netuid=netuid, min_difficulty=1),
            bt.calls.AdminUtils.sudo_set_difficulty(netuid=netuid, difficulty=1),
            bt.calls.AdminUtils.sudo_set_network_pow_registration_allowed(
                netuid=netuid, registration_allowed=True
            ),
        ):
            await submit(call, root=True)
        await cli(
            "subnets",
            "register",
            "--netuid",
            str(netuid),
            "--pow",
            "--pow-backend",
            "cpu",
            "--pow-workers",
            "1",
            "--pow-timeout",
            "30",
        )
        assert await free(wallet.coldkey) == 0
        assert (
            await client.query(bt.storage.SubtensorModule.Owner, [wallet.hotkey.ss58_address])
            == wallet.coldkey.ss58_address
        )
        assert (
            await client.query(
                bt.storage.SubtensorModule.Uids, [netuid, wallet.hotkey.ss58_address]
            )
            is not None
        )
        registrations = await cli("wallet", "registrations", "--netuid", str(netuid))
        assert len(registrations[0]["hotkeys"]) == 1, registrations
        assert len(registrations[0]["hotkeys"][0]["registrations"]) == 1, registrations
        if hashed:
            assert (await record(wallet.coldkey))["generation"] == 1
        await fund(wallet.coldkey, 100)
        await fund(wallet.hotkey)

        # EVM-side commands consume the same named native wallets. Their
        # receiving descriptors must survive until finalized setup is checked.
        password = tmp_path / "evm-password"
        password.write_text("local-wallet-test")
        evm = create_evm_key("default", "four-types", wallet_path, password=password.read_text())
        evm_coldkey = h160_to_ss58(evm.address)
        await submit(
            bt.calls.Balances.force_set_balance(who=evm_coldkey, new_free=50 * 10**9),
            root=True,
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=netuid, subtoken_enabled=True),
            root=True,
        )

        async def evm_cli(*args):
            result = await cli(*args, "--wallet-password-file", str(password))
            assert result["success"], result
            return result

        before = await free(wallet.coldkey)
        await evm_cli("evm", "send-to-ss58", "--amount-tao", "1")
        assert await free(wallet.coldkey) == before + 10**9
        address = receiving_address(wallet.coldkey, genesis)
        await evm_cli("evm", "call", "balance-transfer", "transfer", address, "--value-tao", "1")
        assert await free(wallet.coldkey) == before + 2 * 10**9
        await evm_cli("evm", "stake", "add", "--netuid", str(netuid), "--amount-tao", "5")
        position = await client.read(
            "stake", coldkey_ss58=evm_coldkey, hotkey_ss58=wallet.hotkey.ss58_address, netuid=netuid
        )
        assert position.rao > 10**9
        await evm_cli("evm", "stake", "remove", "--netuid", str(netuid), "--amount-alpha", "1")
        remaining = await client.read(
            "stake", coldkey_ss58=evm_coldkey, hotkey_ss58=wallet.hotkey.ss58_address, netuid=netuid
        )
        assert remaining.rao == position.rao - 10**9
        cold_before = await record(wallet.coldkey) if hashed else None
        for _ in range(2):
            result = await client.execute(
                bt.intents.Transfer(alice.coldkey.ss58_address, 1), wallet
            )
            assert result.success, result.to_dict()
        await submit(
            bt.calls.Balances.transfer_keep_alive(dest=alice.coldkey.ss58_address, value=10**18),
            wallet,
            success=False,
        )
        await submit(bt.calls.System.remark(remark="0x010203"), wallet, role="hotkey")
        if hashed:
            assert (await record(wallet.coldkey))["generation"] == cold_before["generation"] + 3
            assert (await record(wallet.hotkey))["generation"] == 1
        # Production rules apply the association cooldown from genesis even
        # for a new UID. Let this manual-seal test chain reach that height.
        # EVM_KEY_ASSOCIATE_RATELIMIT is not exported in pallet metadata.
        association_delay = int(os.getenv("E2E_EVM_ASSOCIATION_DELAY", "7200"))
        deadline = time.monotonic() + 900
        while await client.block() < association_delay:
            assert time.monotonic() < deadline, "chain did not reach the EVM association window"
            await asyncio.sleep(0.5)
        await evm_cli("evm", "associate", "--netuid", str(netuid))
        uid = await client.query(
            bt.storage.SubtensorModule.Uids, [netuid, wallet.hotkey.ss58_address]
        )
        associated = await client.query(
            bt.storage.SubtensorModule.AssociatedEvmAddress, [netuid, uid]
        )
        assert associated[0].lower() == evm.address.lower()
        recovered = bt.Wallet("restored", path=str(tmp_path / "recovered"))
        recovered.regenerate_coldkey(
            mnemonic=created["coldkey_mnemonic"],
            crypto_type=code,
            use_password=False,
            suppress=True,
        )
        recovered.regenerate_hotkey(
            mnemonic=created["hotkey_mnemonic"], crypto_type=code, use_password=False, suppress=True
        )
        assert recovered.coldkey.ss58_address == wallet.coldkey.ss58_address
        assert recovered.hotkey.ss58_address == wallet.hotkey.ss58_address
        await submit(bt.calls.System.remark(remark="0x04"), recovered)
        await submit(bt.calls.System.remark(remark="0x05"), recovered, role="hotkey")
        for role in ("coldkey", "hotkey"):
            key = getattr(recovered, role)
            generation = (await record(key))["generation"] if hashed else None
            signature = bt.wallets.sign_message_key(
                "four wallet types", key, hashed_generation=generation
            )["signature"]
            address = receiving_address(key, genesis) if hashed else key.ss58_address
            assert bt.wallets.verify_message(
                "four wallet types", signature, address, crypto_type=code
            )
            assert not bt.wallets.verify_message("tampered", signature, address, crypto_type=code)
        hot = recovered.hotkey
        if hashed:
            hot = hot.at_generation((await record(hot))["generation"])
        headers = bt.http_auth.sign(
            hot,
            method="POST",
            path="/wallet-test",
            body=b"request",
            receiver_ss58=alice.hotkey.ss58_address,
        )
        caller = bt.http_auth.verify(
            headers,
            b"request",
            method="POST",
            path="/wallet-test",
            self_hotkey_ss58=alice.hotkey.ss58_address,
            expected_crypto_type=code,
        )
        assert caller.hotkey_ss58 == hot.ss58_address
        if hashed:
            raw = client._substrate.raw
            call = await client.compose(bt.calls.System.remark(remark="0x06"))
            xt = await raw.create_signed_extrinsic(call, recovered.coldkey, era={"period": 64})
            report = await raw.submit_extrinsic(
                xt, wait_for_inclusion=True, wait_for_finalization=True
            )
            assert report.is_success
            before = await record(recovered.coldkey)
            with pytest.raises(
                SubstrateRequestException,
                match=r"(?i)invalid transaction|outdated|stale|temporarily banned",
            ):
                await raw._session.request("author_submitExtrinsic", ["0x" + bytes(xt.data).hex()])
            assert await record(recovered.coldkey) == before

        # Root staking exercises hotkey ownership independently of PoW admission.
        await submit(
            bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                netuid=0, registration_allowed=True
            ),
            root=True,
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=0, subtoken_enabled=True),
            root=True,
        )
        for intent in (
            bt.intents.RootRegister(hotkey_ss58=wallet.hotkey.ss58_address),
            bt.intents.AddStake(wallet.hotkey.ss58_address, 0, 5),
            bt.intents.RemoveStake(wallet.hotkey.ss58_address, 0, 1),
        ):
            result = await client.execute(intent, recovered)
            assert result.success, result.to_dict()
        await cli("wallet", "balance", "--all")
        await cli("wallet", "overview")
        await cli("wallet", "inspect")
        await cli("stake", "list")

        delegate = bt.wallets.create(
            name="delegate",
            path=wallet_path,
            coldkey_crypto_type=code,
            use_password=False,
            on_mnemonic=lambda *_: None,
        )
        await fund(delegate.coldkey)
        await submit(
            bt.calls.Proxy.add_proxy(
                delegate=delegate.coldkey.ss58_address, proxy_type="Any", delay=0
            ),
            recovered,
        )
        transfer = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=alice.coldkey.ss58_address, value=10**9)
        )
        await submit(
            bt.calls.Proxy.proxy(
                real=recovered.coldkey.ss58_address, force_proxy_type="Any", call=transfer
            ),
            delegate,
        )
        if hashed:
            await submit(
                bt.calls.Proxy.add_proxy(
                    delegate=alice.coldkey.ss58_address, proxy_type="Any", delay=0
                ),
                recovered,
                success=False,
            )
            alias = "0x" + bytes(recovered.coldkey.public_key)[:20].hex()
            assert (
                await client.query(("HashedAccounts", "EvmAliases"), [alias])
                == recovered.coldkey.ss58_address
            )
            destination = "0x" + (bytes([160 + code]) * 20).hex()
            rpc = client._substrate.raw._session.request
            before = int(await rpc("eth_getBalance", [destination, "latest"]), 16)
            await submit(
                bt.calls.EVM.call(
                    source=alias,
                    target=destination,
                    input="0x",
                    value=[10**18, 0, 0, 0],
                    gas_limit=100000,
                    max_fee_per_gas=[10**10, 0, 0, 0],
                    max_priority_fee_per_gas=None,
                    nonce=None,
                    access_list=[],
                    authorization_list=[],
                ),
                recovered,
            )
            assert int(await rpc("eth_getBalance", [destination, "latest"]), 16) > before

        # The same public recovery scheme must survive the full ownership migration.
        replacement = bt.wallets.create(
            name="replacement",
            path=wallet_path,
            coldkey_crypto_type=code,
            use_password=False,
            on_mnemonic=lambda *_: None,
        )
        destination = (
            receiving_address(replacement.coldkey, genesis)
            if hashed
            else replacement.coldkey.ss58_address
        )
        stake_before = await client.read(
            "stake",
            coldkey_ss58=recovered.coldkey.ss58_address,
            hotkey_ss58=recovered.hotkey.ss58_address,
            netuid=0,
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_coldkey_swap_announcement_delay(duration=2), root=True
        )
        await submit(
            bt.calls.AdminUtils.sudo_set_coldkey_swap_reannouncement_delay(duration=0), root=True
        )
        # Re-announcing under the active swap lock must remain a direct call;
        # the first announcement may still sponsor a new destination atomically.
        for _ in range(2):
            result = await client.execute(bt.intents.AnnounceColdkeySwap(destination), recovered)
            assert result.success, result.to_dict()
            start = await client._substrate.raw._session.request("chain_getHeader", [])
            deadline = time.monotonic() + 90
            while True:
                head = await client._substrate.raw._session.request("chain_getHeader", [])
                if int(head["number"], 16) >= int(start["number"], 16) + 3:
                    break
                assert time.monotonic() < deadline, "chain stopped during coldkey swap delay"
                await asyncio.sleep(0.2)
        result = await client.execute(bt.intents.SwapColdkeyAnnounced(destination), recovered)
        assert result.success, result.to_dict()
        assert (
            await client.query(bt.storage.SubtensorModule.Owner, [recovered.hotkey.ss58_address])
            == replacement.coldkey.ss58_address
        )
        stake_after = await client.read(
            "stake",
            coldkey_ss58=replacement.coldkey.ss58_address,
            hotkey_ss58=recovered.hotkey.ss58_address,
            netuid=0,
        )
        assert stake_after.rao == stake_before.rao
        await submit(bt.calls.System.remark(remark="0x07"), replacement)
