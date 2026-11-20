"""Encrypted CLI unlock must preserve the recipient's account mode.

Only disposable wallets under pytest's temporary directory are used. The
destination is deliberately bare SS58: rotating recipients need their local
descriptor throughout submission; fixed post-quantum recipients need no registration.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

import pytest

import bittensor as bt
from bittensor.sp_core import Keypair, requires_registration
from tests.e2e.test_mldsa_extended import Evidence
from tests.e2e.test_mldsa_release_costs import attributes, money, observe, setup, snapshot
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


@pytest.mark.parametrize(
    "code",
    [7, 8, 9, 10, 11, 12],
    ids=["hashed-ms", "ms", "falcon512", "hashed-falcon512", "falcon1024", "hashed-falcon1024"],
)
@pytest.mark.parametrize("shielded", [False, True])
async def test_encrypted_cli_sender_retains_local_ms_registration(tmp_path, code, shielded):
    report = Evidence(f"release-cli-unlock-registration-{code}-{int(shielded)}")
    wallet_path = tmp_path / "wallets"
    password_file = tmp_path / "password"
    password_file.write_text("disposable-localnet-registration-test")
    password_file.chmod(0o600)
    sender = bt.Wallet("encrypted-sender", path=str(wallet_path))
    sender.regenerate_coldkey(
        seed=os.urandom(32),
        coldkey_password=password_file.read_text(),
        suppress=True,
        save_coldkey_to_env=False,
    )
    assert sender.coldkey_file.is_encrypted()
    recipient_seed = os.urandom(32)
    recipient_wallet = bt.Wallet("local-ms-recipient", path=str(wallet_path))
    recipient_wallet.regenerate_coldkey(
        seed=recipient_seed, crypto_type=code, use_password=False, suppress=True
    )
    recipient = recipient_wallet.coldkeypub
    rotating = requires_registration(recipient)
    sponsor_uri = os.getenv("E2E_CLI_SPONSOR_URI", "//Alice")
    sponsor = dev_wallet(sponsor_uri, sponsor_uri + "//hot")
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

    async def cli(*extra):
        process = await asyncio.create_subprocess_exec(
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
            "--wallet",
            sender.name,
            "--wallet-path",
            str(wallet_path),
            "wallet",
            "transfer",
            "--dest",
            recipient.ss58_address,
            "--amount",
            "2",
            "--wallet-password-file",
            str(password_file),
            "--mev-shield" if shielded else "--no-mev-shield",
            *extra,
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
        text = out.decode() + err.decode()
        return process.returncode, out.decode(), text

    async with (
        asyncio.timeout(900),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        registration_deposit = await setup(client)
        deposit = registration_deposit if rotating else 0
        async with alice_submission():
            funded = await client.execute(
                bt.intents.Transfer(sender.coldkeypub.ss58_address, 50),
                sponsor,
                shielded=False,
                wait_for_finalization=True,
            )
        await observe(client, report, "fund encrypted classical sender", funded)

        async def sender_state():
            return await client.query(bt.storage.System.Account, [sender.coldkeypub.ss58_address])

        before_sender = await sender_state()
        before_recipient = await snapshot(client, recipient)
        assert before_recipient["authority"] is None
        assert before_recipient["account"]["data"]["free"] == 0
        if rotating:
            denied_code, _, denied_text = await cli()
            assert denied_code != 0, denied_text
            assert "explicit registration sponsorship consent" in denied_text, denied_text
            assert await sender_state() == before_sender
            assert await snapshot(client, recipient) == before_recipient
            report.add("no consent refuses before any transaction", output=denied_text)

        status, output, text = await cli(
            *(["--max-registration-deposit", "0.2"] if rotating else [])
        )
        assert status == 0, text
        result = json.loads(output)
        assert result["success"] and result["block_hash"] and result["extrinsic_id"], result
        assert bool(result.get("data", {}).get("shielded")) is shielded
        height, index = map(int, result["extrinsic_id"].split("-"))
        async with asyncio.timeout(180):
            while await client._substrate.finalized_block_number() < height:
                await asyncio.sleep(1)
        assert await client._substrate.block_hash(height) == result["block_hash"]
        events = [
            entry
            for entry in await client._substrate.raw.get_events(result["block_hash"])
            if entry.get("extrinsic_idx") == index
        ]
        registrations = attributes(events, "HashedAccounts", "RegisteredMlDsa")
        if rotating:
            assert len(registrations) == 1
            assert registrations[0]["account"] == recipient.ss58_address
            assert registrations[0]["sponsor"] == sender.coldkeypub.ss58_address
            assert int(registrations[0]["deposit"]) == deposit
        else:
            assert registrations == []
        transfers = attributes(events, "Balances", "Transfer")
        assert any(
            event["from"] == sender.coldkeypub.ss58_address
            and event["to"] == recipient.ss58_address
            and int(event["amount"]) == 2 * 10**9
            for event in transfers
        )
        assert bool(attributes(events, "Utility", "BatchCompleted")) == rotating
        after_sender, after_recipient = await sender_state(), await snapshot(client, recipient)
        assert (after_recipient["authority"] is not None) == rotating
        assert after_recipient["account"]["data"]["free"] == 2 * 10**9
        assert after_sender["data"]["reserved"] == before_sender["data"]["reserved"] + deposit
        assert after_sender["nonce"] == before_sender["nonce"] + (2 if shielded else 1)
        total_fee = (
            before_sender["data"]["free"] - after_sender["data"]["free"] - deposit - 2 * 10**9
        )
        paid = attributes(events, "TransactionPayment", "TransactionFeePaid")
        assert len(paid) == 1
        inner_fee = int(paid[0]["actual_fee"])
        assert total_fee > inner_fee if shielded else total_fee == inner_fee
        report.add(
            "encrypted unlock preserves atomic recipient registration and payment",
            result=result,
            canonical_events=events,
            sender_before=before_sender,
            sender_after=after_sender,
            recipient_after=after_recipient,
            total_fee_rao=total_fee,
        )

        # Restore from the seed, rather than reusing a key object advanced by
        # this client. The recipient must actually control the funded account.
        restored = Keypair.create_from_seed(recipient_seed, code)
        assert restored.ss58_address == recipient.ss58_address
        returned = await client.execute(
            bt.intents.Transfer(sender.coldkeypub.ss58_address, money(10**9)),
            restored,
            shielded=False,
            wait_for_finalization=True,
        )
        await observe(client, report, "recovered seed spends the received funds", returned)
        assert (await sender_state())["data"]["free"] == after_sender["data"]["free"] + 10**9
        authority = (await snapshot(client, recipient))["authority"]
        assert authority["generation"] == 1 if rotating else authority is None
