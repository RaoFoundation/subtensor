"""Mixed signature schemes and Vault transport on an activated local chain.

Vault's QR transport is simulated with a test key; metadata proofs, signatures,
extrinsic encoding and chain execution are real. This is not a phone firmware test.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
from itertools import permutations

import pytest

import bittensor as bt
from bittensor.hashed import descriptor_value
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from bittensor.vault.signer import VaultSigner
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


async def fund(client, key, amount=100):
    result = await client.execute(bt.intents.Transfer(receiving_address(key), amount), dev_wallet())
    assert result.success, result.to_dict()


async def account(client, address):
    return await client.query(bt.storage.System.Account, [address])


@pytest.mark.parametrize(
    "modes", [("hashed", "standard", "standard"), ("standard", "hashed", "hashed")]
)
async def test_cli_mixed_two_of_three_all_pairs_and_orders(tmp_path, modes):
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
    path = str(tmp_path / "wallets")

    async def cli(wallet, *args, success=True):
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
            "--wallet-path",
            path,
            "--wallet",
            wallet,
            *args,
            env=env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
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

    keys = {}
    for scheme, mode in zip(("ms", "sr", "ed"), modes):
        await cli(
            scheme, "wallet", "create", "--crypto-type", scheme, "--type", mode, "--no-password"
        )
        keys[scheme] = bt.Wallet(scheme, path=path).coldkey
    saved = await cli(
        "ms", "multisig", "add", "team", "--threshold", "2", "--signatories", "ms,sr,ed"
    )
    address = saved["multisig_address"]
    recipient = Keypair.create_from_seed(os.urandom(32), 5)
    destination = recipient.ss58_address
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.AdminUtils.HashedAccountsEnabled) is True
        for key in keys.values():
            await fund(client, key)
        result = await client.execute(bt.intents.Transfer(address, 20), dev_wallet())
        assert result.success, result.to_dict()
        # Both spellings and all member permutations derive the same account.
        for names in permutations(keys):
            multi = await client.multisig([receiving_address(keys[name]) for name in names], 2)
            assert multi.address == address
        # Registration is deliberately direct-only in this runtime. The CLI
        # must refuse an unregistered destination before taking a deposit.
        args = ("wallet", "transfer", "--dest", receiving_address(recipient), "--amount", "1")
        rejected = await cli("team", *args, "--signatory", "ms", success=False)
        assert "direct sponsor wallet" in rejected, rejected
        result = await client.submit_call(
            bt.calls.HashedAccounts.register(
                descriptor=descriptor_value(bytes(recipient.hashed_descriptor)),
            ),
            dev_wallet(),
            wait_for_finalization=True,
        )
        assert result.success, result.to_dict()
        rejected = await cli("team", *args, "--signatory", "ms=vault", success=False)
        assert "standard sr25519/ed25519 accounts only" in rejected, rejected
        for first, second in permutations(keys, 2):
            before = int((await account(client, destination))["data"]["free"])
            reserved = int((await account(client, keys[first].ss58_address))["data"]["reserved"])
            args = ("wallet", "transfer", "--dest", receiving_address(recipient), "--amount", "1")
            await cli("team", *args, "--signatory", first)
            assert int((await account(client, destination))["data"]["free"]) == before
            assert (
                int((await account(client, keys[first].ss58_address))["data"]["reserved"])
                > reserved
            )
            rejected = await cli("team", *args, "--signatory", first, success=False)
            assert "already approved" in rejected, rejected
            await cli("team", *args, "--signatory", second)
            assert int((await account(client, destination))["data"]["free"]) == before + 10**9
            assert (
                int((await account(client, keys[first].ss58_address))["data"]["reserved"])
                == reserved
            )
        # Every member has signed repeatedly; rotating identities stay stable.
        again = await client.multisig([receiving_address(key) for key in keys.values()], 2)
        assert again.address == address
        # A single invocation can collect approvals from different local modes.
        await cli("team", *args, "--signatory", "ms=wallet", "--signatory", "sr=wallet")
        assert int((await account(client, destination))["data"]["free"]) == 7 * 10**9

        # Cancellation is authorized by the opener, and restores its deposit.
        call = bt.calls.System.remark(remark="0x1234")
        composed = await client.compose(call)
        call_hash = "0x" + bytes(composed.call_hash).hex()
        opener = keys["ms"]
        reserved = int((await account(client, opener.ss58_address))["data"]["reserved"])
        result = await again.approve(call, opener, wait_for_finalization=True)
        assert result.success, result.to_dict()
        pending = await client.query(bt.storage.Multisig.Multisigs, [address, call_hash])
        assert pending
        others = [member for member in again.signatories if member != opener.ss58_address]
        result = await client.submit_call(
            bt.calls.Multisig.cancel_as_multi(
                threshold=2,
                other_signatories=others,
                timepoint=pending["when"],
                call_hash=call_hash,
            ),
            opener,
        )
        assert result.success, result.to_dict()
        assert await client.query(bt.storage.Multisig.Multisigs, [address, call_hash]) is None
        assert int((await account(client, opener.ss58_address))["data"]["reserved"]) == reserved

        # Failed inner dispatch still consumes the included signing generation;
        # the next mixed approval must resynchronize and work normally.
        failing = bt.calls.Balances.transfer_keep_alive(dest=destination, value=10**18)
        before = int((await account(client, destination))["data"]["free"])
        result = await again.approve(failing, opener, wait_for_finalization=True)
        assert result.success, result.to_dict()
        result = await again.approve(failing, keys["sr"], wait_for_finalization=True)
        assert not result.success, result.to_dict()
        assert int((await account(client, destination))["data"]["free"]) == before
        await cli("team", *args, "--signatory", "ms=wallet", "--signatory", "sr=wallet")
        assert int((await account(client, destination))["data"]["free"]) == before + 10**9
        # Being a member does not make the composite account an MS authority.
        # The existing no-downgrade proxy policy must still reject it.
        result = await client.submit_call(
            bt.calls.Proxy.add_proxy(
                delegate=address,
                proxy_type="Any",
                delay=0,
            ),
            opener,
        )
        assert not result.success, result.to_dict()


class SimulatedVault(VaultSigner):
    """Replace only the phone/browser boundary, keeping actual proof/QR generation."""

    def __init__(self, key):
        super().__init__(key.ss58_address, crypto_type=key.crypto_type, open_browser=False)
        self.key = key
        self.rounds = 0
        self.http_url = "http://simulated-vault.invalid"

    async def _ensure_server(self):
        return self

    async def sign_unsigned_extrinsic(self, unsigned):
        self.unsigned = unsigned
        return await super().sign_unsigned_extrinsic(unsigned)

    async def request_signature(self, *, frames, frames_hex, summary, **kwargs):
        assert self._context is not None  # real metadata digest/proof path
        assert len(frames) == len(frames_hex) > 0
        chunks = [bytes.fromhex(frame) for frame in frames_hex]
        for index, chunk in enumerate(chunks):
            assert chunk[:5] == b"\x00" + len(chunks).to_bytes(2, "big") + index.to_bytes(2, "big")
        frame = b"".join(chunk[5:] for chunk in chunks)
        assert frame[:35] == bytes([0x53, self.crypto_type, 0x06]) + self.public_key
        assert frame[-32:] == bytes.fromhex(self.unsigned.genesis_hash.removeprefix("0x"))
        assert summary["address"] == self.ss58_address
        self.rounds += 1
        return (bytes([self.crypto_type]) + self.key.sign(self.unsigned.payload)).hex()


@pytest.mark.parametrize("vault_code", [0, 1])
async def test_vault_adapter_co_signs_hashed_ms_both_orders(vault_code):
    ms = Keypair.create_from_seed(os.urandom(32), 5)
    vault_key = Keypair.create_from_seed(os.urandom(32), vault_code)
    third = Keypair.create_from_seed(os.urandom(32), 1)
    vault = SimulatedVault(vault_key)
    destination = Keypair.create_from_seed(os.urandom(32), 1).ss58_address
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        for key in (ms, vault_key, third):
            await fund(client, key)
        multi = await client.multisig(
            [receiving_address(ms), vault.ss58_address, third.ss58_address], 2
        )
        result = await client.execute(bt.intents.Transfer(multi.address, 10), dev_wallet())
        assert result.success, result.to_dict()
        call = bt.calls.Balances.transfer_keep_alive(dest=destination, value=10**9)
        for first, second in ((ms, vault), (vault, ms)):
            before = int((await account(client, destination))["data"]["free"])
            result = await multi.approve(call, first, wait_for_finalization=True)
            assert result.success, result.to_dict()
            assert int((await account(client, destination))["data"]["free"]) == before
            result = await multi.approve(call, second, wait_for_finalization=True)
            assert result.success, result.to_dict()
            assert int((await account(client, destination))["data"]["free"]) == before + 10**9
        assert vault.rounds == 2


@pytest.mark.parametrize("recipient_code", [4, 5, 6, 7])
@pytest.mark.parametrize("threshold", [1, 2])
async def test_direct_sdk_multisig_receiving_transfer(recipient_code, threshold):
    members = [Keypair.create_from_seed(os.urandom(32), code) for code in (5, 1, 0)]
    recipient = Keypair.create_from_seed(os.urandom(32), recipient_code)
    spec = bt.intents.Transfer(receiving_address(recipient), 1).to_dict()

    def intent_for(index, timepoint=None):
        kwargs = {
            "other_signatories": [key.ss58_address for i, key in enumerate(members) if i != index],
            "call": spec,
        }
        if threshold == 1:
            return bt.intents.MultisigThreshold1(**kwargs)
        return bt.intents.MultisigExecute(threshold=threshold, timepoint=timepoint, **kwargs)

    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        reserved_before = {}
        for key in members:
            await fund(client, key)
            reserved_before[key.ss58_address] = int(
                (await account(client, key.ss58_address))["data"]["reserved"]
            )
        multi = await client.multisig([key.ss58_address for key in members], threshold)
        result = await client.execute(bt.intents.Transfer(multi.address, 10), dev_wallet())
        assert result.success, result.to_dict()
        with pytest.raises(ValueError, match="direct sponsor"):
            await client.plan(intent_for(0), members[0])
        assert int((await account(client, multi.address))["data"]["free"]) == 10**10

        registered = await client.submit_call(
            bt.calls.HashedAccounts.register(
                descriptor=descriptor_value(bytes(recipient.hashed_descriptor))
            ),
            dev_wallet(),
            wait_for_finalization=True,
        )
        assert registered.success, registered.to_dict()
        first = await client.execute(intent_for(0), members[0])
        assert first.success, first.to_dict()
        if threshold == 2:
            assert int((await account(client, recipient.ss58_address))["data"]["free"]) == 0
            pending = await client.query(
                bt.storage.Multisig.Multisigs,
                [multi.address, first.data["multisig_call_hash"]],
            )
            second = await client.execute(intent_for(1, pending["when"]), members[1])
            assert second.success, second.to_dict()
            assert second.data["multisig_call_data"] == first.data["multisig_call_data"]
            assert (
                await client.query(
                    bt.storage.Multisig.Multisigs,
                    [multi.address, first.data["multisig_call_hash"]],
                )
                is None
            )
        assert int((await account(client, recipient.ss58_address))["data"]["free"]) == 10**9
        assert int((await account(client, multi.address))["data"]["free"]) == 9 * 10**9
        for key in members:
            # No approval deposit remains; the MS registration reserve is separate.
            assert (
                int((await account(client, key.ss58_address))["data"]["reserved"])
                == (reserved_before[key.ss58_address])
            )
