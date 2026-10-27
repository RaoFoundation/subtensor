"""Real CLI compatibility matrix on an isolated, activated Alice-root chain.

Parallel workers must share E2E_WALLET_ALICE_LOCK for sponsor submissions.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
from pathlib import Path

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from bittensor.sp_core import (
    PQ_CRYPTO_TYPES,
    Keypair,
    is_stateless_pq,
    requires_registration,
    serialized_keypair_to_keyfile_data,
)
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.receiving import descriptor_receiving_address
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
WALLET_MODES = [
    (scheme, mode)
    for scheme in ("sr", "ed", "ms", "falcon512", "falcon1024")
    for mode in ("standard", "hashed")
]
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


@pytest.mark.parametrize("scheme,mode", WALLET_MODES)
async def test_cli_wallet_parity(tmp_path, scheme, mode):
    env = dict(os.environ)
    receipts = []
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
            "--max-registration-deposit",
            "2",
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
        result = json.loads(out)
        if isinstance(result, dict) and result.get("block_hash"):
            # Transaction receipts contain public data. Do not persist wallet
            # creation/import output, which can contain recovery secrets.
            receipts.append({"wallet": name, "operation": " ".join(args[:2]), **result})
        return result

    created = await cli(
        "original",
        "wallet",
        "create",
        "--crypto-type",
        scheme,
        *(["--type", "hashed"] if mode == "hashed" else []),
        "--no-password",
    )
    original = bt.Wallet("original", path=wallet_path)
    code = bt.wallets.parse_crypto_type(scheme, mode)
    protected = requires_registration(original.coldkeypub)
    stateless = is_stateless_pq(original.coldkeypub)
    alice = dev_wallet()
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == alice.coldkey.ss58_address
        assert await client.query(("AdminUtils", "HashedAccountsEnabled")) is True

        async def fund(key, amount=100):
            async with alice_submission():
                result = await client.execute(
                    bt.intents.Transfer(receiving_address(key), amount),
                    alice,
                    wait_for_finalization=True,
                    max_registration_deposit=bt.Balance.from_tao(2),
                )
            assert result.success, result.to_dict()
            receipts.append({"operation": "sponsor transfer", **result.to_dict()})
            if is_stateless_pq(key):
                assert (
                    await client.query(("HashedAccounts", "MlDsaAccounts"), [key.ss58_address])
                    is None
                )

        async def balance(key):
            value = await client.query(bt.storage.System.Account, [key.ss58_address])
            return int(value["data"]["free"])

        async def nonce(key):
            value = await client.query(bt.storage.System.Account, [key.ss58_address])
            return int(value["nonce"])

        # The ordinary receiving address sponsors setup only for rotating accounts;
        # Fixed post-quantum wallets use SS58 without creating a registration record.
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

        # Every sender/recipient combination uses the normal transfer command.
        for recipient_scheme, recipient_mode in WALLET_MODES:
            name = f"recipient-{recipient_scheme}-{recipient_mode}"
            await cli(
                name,
                "wallet",
                "create",
                "--crypto-type",
                recipient_scheme,
                "--type",
                recipient_mode,
                "--no-password",
            )
            recipient = bt.Wallet(name, path=wallet_path)
            before = await balance(recipient.coldkeypub)
            await cli("original", "wallet", "transfer", "--dest", name, "--amount", "1")
            assert await balance(recipient.coldkeypub) == before + 10**9
            address = receiving_address(recipient.coldkeypub)
            if requires_registration(recipient.coldkeypub):
                address = descriptor_receiving_address(recipient.coldkeypub)
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
                    "--type",
                    mode,
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
                "--type",
                mode,
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

        # Read-only backups retain the scheme for descriptors and fixed PQ SS58.
        for role in ("coldkey", "hotkey"):
            key = getattr(original, role)
            if protected or stateless:
                await cli(
                    "watch-only",
                    "wallet",
                    f"regen-{role}pub",
                    "--address",
                    descriptor_receiving_address(key) if protected else key.ss58_address,
                    *(["--crypto-type", scheme] if stateless else []),
                )
                public = getattr(bt.Wallet("watch-only", path=wallet_path), role + "pub")
                assert public.ss58_address == key.ss58_address
                assert public.crypto_type == code
            if code in PQ_CRYPTO_TYPES:
                from bittensor.mldsa import install_message_capability

                grant = install_message_capability(
                    key,
                    epoch=1,
                    expires_at=(await client.block()) + 100_000,
                    audience=b"bittensor/wallet-message",
                )
                result = await client.submit_call(grant, key)
                assert result.success, result.to_dict()
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
        await cli(
            "peer", "wallet", "create", "--crypto-type", scheme, "--type", mode, "--no-password"
        )
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
        assert saved["entry"]["account_mode"] == "safe"
        group = await client.multisig(
            [original.coldkey.ss58_address, peer.coldkey.ss58_address],
            2,
            account_mode=saved["entry"]["account_mode"],
        )
        assert group.address == multi
        status = await group.protection_status()
        assert status["safe_derivation"] and not status["eligible"] and not status["registered"]
        assert await client.query(("HashedAccounts", "DerivedAuthorities"), [multi]) is None
        await cli("original", "wallet", "transfer", "--dest", multi, "--amount", "20")
        dest = bt.Wallet("recipient-ed-hashed", path=wallet_path).coldkeypub
        before = await balance(dest)
        for signer in ("original", "peer"):
            await cli(
                "team",
                "wallet",
                "transfer",
                "--dest",
                "recipient-ed-hashed",
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
            "recipient-ed-hashed",
            "--amount",
            "1",
            "--proxy-for",
            "original",
        )
        assert await balance(dest) == before + 10**9

        # Each scheme can create a subnet and exercise its owner permissions.
        setup = (
            bt.calls.AdminUtils.sudo_set_lock_reduction_interval(interval=1),
            bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0),
            bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0),
            bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
            bt.calls.Balances.force_set_balance(who=original.coldkey.ss58_address, new_free=10**15),
        )
        async with alice_submission():
            result = await client.submit_call(
                bt.calls.Sudo.sudo(
                    call=await client.compose(
                        bt.calls.Utility.batch_all(
                            calls=[await client.compose(call) for call in setup]
                        )
                    )
                ),
                alice,
                wait_for_finalization=True,
            )
        assert result.success, result.to_dict()
        receipts.append({"operation": "root subnet setup", **result.to_dict()})
        created_subnet = await cli("original", "subnets", "create")
        netuid = int(created_subnet["data"]["netuid"])
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
        if stateless:
            for key in (original.coldkey, original.hotkey, peer.coldkey):
                assert (
                    await client.query(("HashedAccounts", "MlDsaAccounts"), [key.ss58_address])
                    is None
                )
        if directory := os.getenv("E2E_EVIDENCE_DIR"):
            evidence_path = Path(directory)
            evidence_path.mkdir(parents=True, exist_ok=True)
            (evidence_path / f"wallet-parity-{scheme}-{mode}.json").write_text(
                json.dumps(
                    {"scheme": scheme, "mode": mode, "netuid": netuid, "receipts": receipts},
                    indent=2,
                    default=str,
                )
                + "\n"
            )
