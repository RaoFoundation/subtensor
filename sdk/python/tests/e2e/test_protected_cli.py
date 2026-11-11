"""Protected-wallet green paths using real btcli subprocesses and a disposable chain.

Run against an isolated Alice-root node with E2E_WALLET_ENDPOINT. The fixture
shortens migration delays only on that disposable chain. No mocks, mainnet
keys, external faucet or scheduler are used.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

import pytest

import bittensor as bt
from bittensor.sp_core import Keypair
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(
        not ENDPOINT,
        reason="requires a disposable Alice-root chain",
    ),
]


class ProtectedCLI:
    def __init__(self, directory):
        self.directory = directory
        self.path = str(directory / "wallets")
        self.env = dict(os.environ)
        for setting in (
            "CONFIG",
            "ADDRESSES_PATH",
            "PROXIES_PATH",
            "MULTISIGS_PATH",
            "MULTISIG_CACHE",
            "SUBNET_NAMES_CACHE",
            "TOKEN_SYMBOLS_CACHE",
            "PROTECTED_PATH",
            "PROXY_ANNOUNCEMENTS_PATH",
        ):
            self.env[f"BTCLI_{setting}"] = str(directory / f"{setting.lower()}.json")
        self.evidence = []

    async def cli(self, wallet, *args):
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-m",
            "bittensor.cli.main",
            "--yes",
            "--json",
            "--no-mev-shield",
            "--network",
            ENDPOINT,
            "--fallback-endpoints",
            "none",
            "--archive-endpoints",
            "none",
            "--wallet-path",
            self.path,
            "--wallet",
            wallet,
            *args,
            env=self.env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            out, err = await asyncio.wait_for(process.communicate(), 150)
        except BaseException:
            process.kill()
            await process.wait()
            raise
        self.evidence.append(
            {
                "wallet": wallet,
                "args": args,
                "code": process.returncode,
                "stdout": out.decode(),
                "stderr": err.decode(),
            }
        )
        if args[:2] == ("wallet", "create"):
            # These keys are disposable, but evidence artifacts should contain
            # only public wallet metadata and transaction receipts.
            public = json.loads(out)
            public.pop("coldkey_mnemonic", None)
            public.pop("hotkey_mnemonic", None)
            self.evidence[-1]["stdout"] = json.dumps(public)
        if location := os.getenv("E2E_EVIDENCE_DIR"):
            from pathlib import Path

            target = Path(location)
            target.mkdir(parents=True, exist_ok=True)
            (target / f"protected-{self.directory.name}.json").write_text(
                json.dumps(self.evidence, indent=2)
            )
        assert process.returncode == 0, out.decode() + err.decode()
        decoder, results, remaining = json.JSONDecoder(), [], out.decode().strip()
        while remaining:
            value, end = decoder.raw_decode(remaining)
            results.append(value)
            remaining = remaining[end:].strip()
        return results[-1] if results else None

    async def create(self, name):
        await self.cli(name, "wallet", "create", "--crypto-type", "sr", "--no-password")
        return bt.Wallet(name, path=self.path).coldkey

    async def wait(self, client, block):
        for _ in range(120):
            if await client.block() >= block:
                return
            await asyncio.sleep(0.5)
        pytest.fail(f"chain did not reach block {block}")


@pytest.mark.parametrize("multisig", [False, True], ids=["coldkey", "multisig"])
async def test_cli_protected_migration_cancel_execute_and_rotate(tmp_path, multisig):
    run = ProtectedCLI(tmp_path)
    owner, reserve, successor = [await run.create(name) for name in ("owner", "reserve", "next")]
    controller, controller_address = "owner", owner.ss58_address
    signatories = []
    if multisig:
        result = await run.cli(
            "owner",
            "multisig",
            "add",
            "team",
            "--threshold",
            "2",
            "--signatories",
            "owner,reserve,next",
        )
        controller, controller_address = "team", result["multisig_address"]
        signatories = ["--signatory", "owner", "--signatory", "reserve"]

    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
        for call in (
            bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
            bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0),
            bt.calls.AdminUtils.sudo_set_coldkey_swap_announcement_delay(duration=2),
            bt.calls.AdminUtils.sudo_set_coldkey_swap_reannouncement_delay(duration=0),
            bt.calls.AdminUtils.sudo_set_nominator_min_required_stake(min_stake=1),
            bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                netuid=0, registration_allowed=True
            ),
            bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=0, subtoken_enabled=True),
        ):
            result = await client.submit_call(
                bt.calls.Sudo.sudo(call=await client.compose(call)),
                dev_wallet(),
                shielded=False,
            )
            assert result.success, result.to_dict()
        for address in {
            owner.ss58_address,
            reserve.ss58_address,
            successor.ss58_address,
            controller_address,
        }:
            result = await client.execute(
                bt.intents.Transfer(address, 50),
                dev_wallet(),
                shielded=False,
            )
            assert result.success, result.to_dict()

        hot = Keypair.create_from_seed(os.urandom(32), 1).ss58_address
        result = await client.submit_call(
            bt.calls.SubtensorModule.root_register(hotkey=hot), dev_wallet(), shielded=False
        )
        assert result.success, result.to_dict()
        await run.cli(
            controller,
            "stake",
            "add",
            "--hotkey",
            hot,
            "--netuid",
            "0",
            "--amount",
            "2",
            *signatories,
        )
        migrated_stake = (
            await client.read("stake", coldkey_ss58=controller_address, hotkey_ss58=hot, netuid=0)
        ).rao
        assert migrated_stake > 0
        await run.cli(controller, "proxy", "protect", "--delay", "4b", *signatories)
        shown = await run.cli(controller, "proxy", "show")
        pure = shown["protected"]["protected_ss58"]
        swap = await client.read("coldkey_swap_announcement", coldkey_ss58=controller_address)
        assert swap is not None
        await run.wait(client, swap["execute_block"])
        await run.cli(controller, "wallet", "swap-coldkey", "--new-coldkey", pure, *signatories)
        assert (
            await client.read("coldkey_swap_announcement", coldkey_ss58=controller_address) is None
        )
        assert (
            await client.read("stake", coldkey_ss58=pure, hotkey_ss58=hot, netuid=0)
        ).rao == migrated_stake
        assert (
            await client.read("stake", coldkey_ss58=controller_address, hotkey_ss58=hot, netuid=0)
        ).rao == 0
        before_funding = await run.cli(controller, "proxy", "show")
        assert before_funding["controller_funding"]["low"]
        await run.cli(controller, "proxy", "fund", "--from-wallet", "reserve")
        shown = await run.cli(controller, "proxy", "show")
        assert shown["controller_funding"]["can_announce"]
        assert not shown["controller_funding"]["low"]

        await run.cli(controller, "proxy", "protect", "--allow", "staking", *signatories)
        allow_row = (await run.cli(controller, "proxy", "show"))["pending"][0]
        await run.wait(client, allow_row["executable_at_block"])
        execute_signer = ["--signatory", "owner"] if multisig else []
        await run.cli(
            controller, "proxy", "execute", "--index", str(allow_row["index"]), *execute_signer
        )
        assert any(
            grant["delegate"] == controller_address
            and grant["proxy_type"] == "Staking"
            and grant["delay"] == 0
            for grant in (await client.read("proxies", coldkey_ss58=pure))["proxies"]
        )

        async def queue():
            await run.cli(
                controller,
                "wallet",
                "transfer",
                "--dest",
                successor.ss58_address,
                "--amount",
                "1",
                *signatories,
            )
            state = await run.cli(controller, "proxy", "show")
            assert len(state["pending"]) == 1
            return state["pending"][0]

        row = await queue()
        before = (await client.read("balance", coldkey_ss58=successor.ss58_address)).rao
        await run.cli(controller, "proxy", "cancel", "--index", str(row["index"]), *signatories)
        assert not (await run.cli(controller, "proxy", "show"))["pending"]
        assert (await client.read("balance", coldkey_ss58=successor.ss58_address)).rao == before
        row = await queue()
        await run.wait(client, row["executable_at_block"])
        # Execution needs one fee payer, including for a multisig controller.
        await run.cli(controller, "proxy", "execute", "--index", str(row["index"]), *execute_signer)
        assert (
            await client.read("balance", coldkey_ss58=successor.ss58_address)
        ).rao == before + 10**9

        for operation in ("transfer", "stake"):
            # Use disposable-chain root authority only to reproduce a low
            # controller balance. Refill and user actions use normal btcli.
            result = await client.submit_call(
                bt.calls.Sudo.sudo(
                    call=await client.compose(
                        bt.calls.Balances.force_set_balance(
                            who=controller_address, new_free=200_000_000
                        )
                    )
                ),
                dev_wallet(),
                shielded=False,
            )
            assert result.success, result.to_dict()
            args = (
                ["wallet", "transfer", "--dest", successor.ss58_address, "--amount", "1"]
                if operation == "transfer"
                else ["stake", "add", "--hotkey", hot, "--netuid", "0", "--amount", "1"]
            )
            before_balance = (await client.read("balance", coldkey_ss58=successor.ss58_address)).rao
            before_stake = (
                await client.read("stake", coldkey_ss58=pure, hotkey_ss58=hot, netuid=0)
            ).rao
            await run.cli(controller, *args, *signatories)
            state = await run.cli(controller, "proxy", "show")
            assert len(state["pending"]) == 1
            refill_row = state["pending"][0]
            assert refill_row["inner_op"] == "batch"
            children = refill_row["inner_args"]["intents"]
            assert children[0]["op"] == "transfer"
            assert children[0]["dest_ss58"] == controller_address
            assert len(children) == 2
            await run.wait(client, refill_row["executable_at_block"])
            # Exactly one execution submission; the refill is already committed.
            await run.cli(
                controller, "proxy", "execute", "--index", str(refill_row["index"]), *execute_signer
            )
            after = await run.cli(controller, "proxy", "show")
            assert not after["pending"]
            assert not after["controller_funding"]["low"]
            if operation == "transfer":
                assert (
                    await client.read("balance", coldkey_ss58=successor.ss58_address)
                ).rao == before_balance + 10**9
            else:
                assert (
                    await client.read("stake", coldkey_ss58=pure, hotkey_ss58=hot, netuid=0)
                ).rao > before_stake
        await run.cli(controller, "proxy", "rotate", "--to", "next", *signatories)
        state = await run.cli(controller, "proxy", "show")
        row = next(item for item in state["pending"] if item["category"] == "rotate-controller")
        await run.wait(client, row["executable_at_block"])
        await run.cli(controller, "proxy", "execute", "--index", str(row["index"]), *execute_signer)
        grants = (await client.read("proxies", coldkey_ss58=pure))["proxies"]
        assert any(
            g["delegate"] == successor.ss58_address and g["proxy_type"] == "Any" for g in grants
        )
        assert not any(g["delegate"] == controller_address for g in grants)
        assert not (await run.cli("next", "proxy", "show"))["pending"]
