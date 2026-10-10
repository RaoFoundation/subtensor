"""Real CLI compatibility matrix on an isolated, activated Alice-root chain."""

from __future__ import annotations

import asyncio
import json
import os
import sys

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from bittensor.sp_core import HASHED_CRYPTO_TYPES, Keypair, serialized_keypair_to_keyfile_data
from tests.harness.receiving import legacy_receiving_address
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
SCHEMES = ("sr", "ed", "hashed", "ms")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


@pytest.mark.parametrize("scheme", SCHEMES)
async def test_cli_wallet_parity(tmp_path, scheme):
    env = dict(os.environ)
    for setting in (
        "CONFIG",
        "ADDRESSES_PATH",
        "PROXIES_PATH",
        "MULTISIGS_PATH",
        "MULTISIG_CACHE",
        "SUBNET_NAMES_CACHE",
        "TOKEN_SYMBOLS_CACHE",
    ):
        env[f"BTCLI_{setting}"] = str(tmp_path / f"{setting.lower()}.json")
    wallet_path = str(tmp_path / "wallets")

    async def cli(name, *args, success=True):
        command = [
            sys.executable,
            "-m",
            "bittensor.cli.main",
            "--yes",
            "--json",
            "--network",
            ENDPOINT,
            "--fallback-endpoints",
            "none",
            "--archive-endpoints",
            "none",
            "--wallet-path",
            wallet_path,
            "--wallet",
            name,
            "--wallet-hotkey",
            "default",
            *args,
        ]
        process = await asyncio.create_subprocess_exec(
            *command, env=env, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )
        try:
            out, err = await asyncio.wait_for(process.communicate(), 120)
        except BaseException:
            process.kill()
            await process.wait()
            raise
        if not success:
            assert process.returncode != 0, out.decode()
            return out.decode() + err.decode()
        assert process.returncode == 0, out.decode() + err.decode()
        return json.loads(out)

    created = await cli("original", "wallet", "create", "--crypto-type", scheme, "--no-password")
    original = bt.Wallet("original", path=wallet_path)
    code = bt.wallets.parse_crypto_type(scheme)
    protected = code in HASHED_CRYPTO_TYPES
    alice = dev_wallet()
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == alice.coldkey.ss58_address
        assert await client.constant(("HashedAccounts", "Enabled")) is True

        async def fund(key, amount=100):
            result = await client.execute(
                bt.intents.Transfer(receiving_address(key), amount), alice
            )
            assert result.success, result.to_dict()

        async def balance(key):
            value = await client.query(bt.storage.System.Account, [key.ss58_address])
            return int(value["data"]["free"])

        async def nonce(key):
            value = await client.query(bt.storage.System.Account, [key.ss58_address])
            return int(value["nonce"])

        # Funding via the ordinary receiving address must sponsor setup, not
        # require a separate manual registration or a different transfer command.
        for key in (original.coldkey, original.hotkey):
            await fund(key, 1000)
            assert await balance(key) == 1000 * 10**9
        await cli("original", "wallet", "balance")
        await cli("original", "wallet", "balance", "--all")
        await cli("original", "wallet", "inspect")
        await cli("original", "wallet", "show")
        for role in ("coldkey", "hotkey"):
            await cli(
                "original", "call", "System.remark", "--args", '{"remark":"0x01"}', "--signer", role
            )

        # All 16 sender/recipient combinations use the normal transfer command.
        for recipient_scheme in SCHEMES:
            name = f"recipient-{recipient_scheme}"
            await cli(name, "wallet", "create", "--crypto-type", recipient_scheme, "--no-password")
            recipient = bt.Wallet(name, path=wallet_path)
            before = await balance(recipient.coldkeypub)
            await cli("original", "wallet", "transfer", "--dest", name, "--amount", "1")
            assert await balance(recipient.coldkeypub) == before + 10**9
            address = receiving_address(recipient.coldkeypub)
            if recipient.coldkeypub.crypto_type in HASHED_CRYPTO_TYPES:
                address = legacy_receiving_address(recipient.coldkeypub)
            await cli("original", "wallet", "transfer", "--dest", address, "--amount", "1")
            assert await balance(recipient.coldkeypub) == before + 2 * 10**9

        # Exercise CLI imports, not just Keypair.create_from_mnemonic. Restored
        # keys must sign successfully after the chain has advanced their state.
        for form in ("mnemonic", "private-key"):
            name = f"restored-{form}"
            for role in ("coldkey", "hotkey"):
                key = getattr(original, role)
                value = (
                    created[f"{role}_mnemonic"]
                    if form == "mnemonic"
                    else json.loads(serialized_keypair_to_keyfile_data(key))["privateKey"]
                )
                flags = ["--no-password"] if role == "coldkey" else []
                await cli(
                    name,
                    "wallet",
                    f"regen-{role}",
                    "--crypto-type",
                    scheme,
                    f"--{form}",
                    value,
                    *flags,
                )
                restored = getattr(bt.Wallet(name, path=wallet_path), role)
                assert restored.ss58_address == key.ss58_address
                assert restored.crypto_type == key.crypto_type
                await cli(
                    name, "call", "System.remark", "--args", '{"remark":"0x02"}', "--signer", role
                )

        # Seed imports also retain both roles and the selected scheme.
        for role, byte in (("coldkey", 67), ("hotkey", 68)):
            seed = bytes([byte]) * 32
            expected = Keypair.create_from_seed(seed, code)
            flags = ["--no-password"] if role == "coldkey" else []
            await cli(
                "seed-backup",
                "wallet",
                f"regen-{role}",
                "--crypto-type",
                scheme,
                "--seed",
                seed.hex(),
                *flags,
            )
            restored = getattr(bt.Wallet("seed-backup", path=wallet_path), role)
            assert restored.ss58_address == expected.ss58_address
            await fund(restored)
            await cli(
                "seed-backup",
                "call",
                "System.remark",
                "--args",
                '{"remark":"0x03"}',
                "--signer",
                role,
            )

        # Read-only backups and message verification retain the descriptor scheme.
        for role in ("coldkey", "hotkey"):
            key = getattr(original, role)
            if protected:
                await cli(
                    "watch-only",
                    "wallet",
                    f"regen-{role}pub",
                    "--address",
                    legacy_receiving_address(key),
                )
                public = getattr(bt.Wallet("watch-only", path=wallet_path), role + "pub")
                assert public.ss58_address == key.ss58_address
                assert public.crypto_type == code
            signed = await cli(
                "original",
                "wallet",
                "sign",
                "--message",
                "parity test",
                *(["--use-hotkey"] if role == "hotkey" else []),
            )
            verified = await cli(
                "original",
                "wallet",
                "verify",
                "--message",
                "parity test",
                "--signature",
                signed["signed_message"],
                "--ss58",
                receiving_address(key),
                "--crypto-type",
                scheme,
            )
            assert verified["valid"], verified

        # Two actual approvals through a named preset, funded through the CLI.
        await cli("peer", "wallet", "create", "--crypto-type", scheme, "--no-password")
        peer = bt.Wallet("peer", path=wallet_path)
        await fund(peer.coldkey)
        saved = await cli(
            "original",
            "multisig",
            "add",
            "team",
            "--threshold",
            "2",
            "--signatories",
            "original,peer",
        )
        multi = saved["multisig_address"]
        await cli("original", "wallet", "transfer", "--dest", multi, "--amount", "20")
        dest = bt.Wallet("recipient-ms", path=wallet_path).coldkeypub
        before = await balance(dest)
        for signer in ("original", "peer"):
            await cli(
                "team",
                "wallet",
                "transfer",
                "--dest",
                "recipient-ms",
                "--amount",
                "1",
                "--signatory",
                signer,
            )
        assert await balance(dest) == before + 10**9

        # The same ordinary command can run through a compatible proxy delegate.
        await cli("original", "proxy", "add", "--delegate", "peer", "--proxy-type", "Any")
        before = await balance(dest)
        await cli(
            "peer",
            "wallet",
            "transfer",
            "--dest",
            "recipient-ms",
            "--amount",
            "1",
            "--proxy-for",
            "original",
        )
        assert await balance(dest) == before + 10**9

        # Each scheme can create a subnet and exercise its owner permissions.
        for call in (
            bt.calls.AdminUtils.sudo_set_lock_reduction_interval(interval=1),
            bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0),
            bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0),
            bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
            bt.calls.Balances.force_set_balance(who=original.coldkey.ss58_address, new_free=10**15),
        ):
            result = await client.submit_call(
                bt.calls.Sudo.sudo(call=await client.compose(call)), alice
            )
            assert result.success, result.to_dict()
        await cli("original", "subnets", "create")
        netuid = max(subnet.netuid for subnet in await client.subnets.all())
        assert await client.query(bt.storage.SubtensorModule.SubnetOwner, [netuid]) == (
            original.coldkey.ss58_address
        )
        assert await client.query(bt.storage.SubtensorModule.SubnetOwnerHotkey, [netuid]) == (
            original.hotkey.ss58_address
        )
        # Runtime policy requires at least one registration route to stay open.
        await cli(
            "original",
            "call",
            "AdminUtils.sudo_set_network_pow_registration_allowed",
            "--args",
            json.dumps({"netuid": netuid, "registration_allowed": True}),
        )
        await cli(
            "original",
            "call",
            "AdminUtils.sudo_set_network_registration_allowed",
            "--args",
            json.dumps({"netuid": netuid, "registration_allowed": False}),
        )
        assert (
            await client.query(bt.storage.SubtensorModule.NetworkRegistrationAllowed, [netuid])
            is False
        )

        # Password handling must work for each scheme; failure cannot advance state.
        password = tmp_path / "password"
        password.write_text("isolated-wallet-test")
        original.coldkey_file.set_keypair(
            original.coldkey, encrypt=True, overwrite=True, password=password.read_text()
        )
        before = await nonce(original.coldkey)
        wrong = tmp_path / "wrong-password"
        wrong.write_text("wrong")
        await cli(
            "original",
            "call",
            "System.remark",
            "--args",
            '{"remark":"0x04"}',
            "--wallet-password-file",
            str(wrong),
            success=False,
        )
        assert await nonce(original.coldkey) == before
        await cli(
            "original",
            "call",
            "System.remark",
            "--args",
            '{"remark":"0x04"}',
            "--wallet-password-file",
            str(password),
        )
        assert await nonce(original.coldkey) == before + 1
