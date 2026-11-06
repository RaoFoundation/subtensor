"""Live coverage for subnet emission weights: the SDK read and intent, root's
baseline share paid off the top of the validator slot, and ``btcli subnets
emission``."""

from __future__ import annotations

import asyncio
import json
import os

import pytest
from typer.testing import CliRunner

import bittensor as bt
from bittensor import emission_weights as ew
from bittensor.cli.main import app
from bittensor.intents import SetSubnetEmissionWeights
from bittensor.wallet import Wallet
from tests.harness.samples import dev_wallet

E2E_ENDPOINT = os.getenv("E2E_ENDPOINT")

pytestmark = [
    pytest.mark.skipif(not E2E_ENDPOINT, reason="requires E2E_ENDPOINT with a writable localnet"),
]

ROOT_STAKE = 100_000 * 10**9
runner = CliRunner()


async def _prepare_subnet(client: bt.Client, alice) -> int:
    """Zero the admin limits, register a subnet for Alice and make sure root
    holds enough TAO for a validator floor to exist. Returns the new netuid."""
    root = await client.query(bt.storage.Sudo.Key)
    assert root == alice.coldkey.ss58_address, "requires the Alice development root"

    async def submit(call, wallet=alice, *, root=False):
        if root:
            call = bt.calls.Sudo.sudo(call=await client.compose(call))
        result = await client.submit_call(call, wallet)
        assert result.success, result.message

    st = bt.storage.SubtensorModule
    if await client.query(st.NetworkRateLimit, []) != 0:
        await submit(bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0), root=True)
        await submit(bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0), root=True)
        await submit(bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0), root=True)
    await submit(bt.calls.SubtensorModule.register_network(hotkey=alice.hotkey.ss58_address))
    netuid = max(subnet.netuid for subnet in await client.subnets.all())
    if not await client.query(st.SubtokenEnabled, [0]):
        await submit(
            bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=0, subtoken_enabled=True),
            root=True,
        )
    if int(await client.query(st.SubnetTAO, [0]) or 0) < ROOT_STAKE:
        await submit(
            bt.calls.SubtensorModule.add_stake(
                hotkey=alice.hotkey.ss58_address, netuid=0, amount_staked=ROOT_STAKE
            )
        )
    return netuid


@pytest.mark.asyncio
async def test_root_is_paid_its_baseline_off_the_top_via_python_sdk() -> None:
    alice = dev_wallet()
    async with bt.Client(E2E_ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        netuid = await _prepare_subnet(client, alice)
        fresh = await client.read("subnet_emission_weights", netuid=netuid)
        assert fresh.weights == ew.default_weights()
        assert fresh.baseline_root_parts > 0, "root stake gives a positive baseline"
        assert fresh.min_validator == fresh.baseline_root_parts + 1
        # On the default vector root earns exactly the baseline and is not capped.
        assert fresh.root_share == pytest.approx(fresh.baseline_root_share, abs=1e-9)
        assert fresh.root_capped is False
        assert fresh.validator_share_after_root == pytest.approx(
            fresh.shares["validator"] - fresh.baseline_root_share, abs=1e-9
        )
        assert fresh.rate_limit()["owner_can_change_now"]

        # Below the floor: the SDK names the chain rule before anything is signed.
        plan = await client.plan(
            SetSubnetEmissionWeights(
                netuid=netuid,
                slots=["validator"],
                values=[max(0, fresh.min_validator - 10**15)],
                renormalize=True,
            ),
            alice,
        )
        assert not plan.ok
        assert "ValidatorEmissionTooLowToPreserveRoot" in plan.violations[0]
        assert plan.preview["root"]["share_after"] is None

        # Raise the validator slot: root keeps its baseline, validators get the rest.
        raise_intent = SetSubnetEmissionWeights(
            netuid=netuid, slots=["validator"], values=["45%"], renormalize=True
        )
        plan = await client.plan(raise_intent, alice)
        assert plan.ok, plan.violations
        assert plan.preview["root"]["share_after"] == pytest.approx(
            fresh.baseline_root_share, abs=1e-9
        )
        assert plan.preview["root"]["validators_after"] == pytest.approx(
            0.45 - fresh.baseline_root_share, abs=1e-9
        )
        result = await client.execute(raise_intent, alice)
        assert result.success, result.message

        after = await client.read("subnet_emission_weights", netuid=netuid)
        assert after.shares["validator"] == pytest.approx(0.45, abs=1e-9)
        # The baseline only moves with root TAO, alpha issuance and the global
        # TaoWeight (a few blocks of emission at most), never with the vector.
        assert after.baseline_root_share == pytest.approx(fresh.baseline_root_share, rel=1e-3)
        assert after.root_share == pytest.approx(after.baseline_root_share, abs=1e-9)
        assert after.root_capped is False
        assert after.validator_share_after_root == pytest.approx(
            0.45 - after.baseline_root_share, abs=1e-9
        )
        assert after.last_change_block > 0

        # Back to the default validator share: root's cut is unchanged either way.
        rate = after.rate_limit()
        defaults = ew.default_weights()
        restore = SetSubnetEmissionWeights(
            netuid=netuid,
            slots=["owner", "validator", "miner"],
            values=[defaults[2], defaults[3], defaults[4]],
        )
        plan = await client.plan(restore, alice)
        if not rate["owner_can_change_now"]:
            assert not plan.ok and "root is exempt" in plan.violations[-1]
            return
        assert plan.ok, plan.violations
        assert plan.preview["root"]["share_after"] == pytest.approx(
            after.baseline_root_share, abs=1e-9
        )
        result = await client.execute(restore, alice)
        assert result.success, result.message
        restored = await client.read("subnet_emission_weights", netuid=netuid)
        assert restored.weights == defaults
        assert restored.root_share == pytest.approx(restored.baseline_root_share, abs=1e-9)


def _disk_wallet(path: str, keys) -> None:
    wallet = Wallet(name="alice", hotkey="default", path=path)
    wallet.coldkey_file.set_keypair(keys.coldkey, encrypt=False, overwrite=True)
    wallet.coldkeypub_file.set_keypair(keys.coldkey, encrypt=False, overwrite=True)
    wallet.hotkey_file.set_keypair(keys.hotkey, encrypt=False, overwrite=True)


def _stderr_json(result) -> dict:
    text = result.stderr if hasattr(result, "stderr") and result.stderr else result.output
    return json.loads(text[text.index("{") :])


def test_btcli_subnets_emission_against_localnet(tmp_path, monkeypatch) -> None:
    alice = dev_wallet()

    async def prepare() -> int:
        async with bt.Client(E2E_ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
            return await _prepare_subnet(client, alice)

    netuid = asyncio.run(prepare())
    _disk_wallet(str(tmp_path), alice)
    for var, value in {
        "BT_NETWORK": E2E_ENDPOINT,
        "BT_FALLBACK_ENDPOINTS": "none",
        "BT_ARCHIVE_ENDPOINTS": "none",
        "BT_WALLET": "alice",
        "BT_WALLET_HOTKEY": "default",
        "BT_WALLET_PATH": str(tmp_path),
        "BTCLI_CONFIG": str(tmp_path / "btcli.json"),
        "BTCLI_PROXIES_PATH": str(tmp_path / "proxies.json"),
        "BTCLI_ADDRESSES_PATH": str(tmp_path / "addresses.json"),
        "BTCLI_MULTISIGS_PATH": str(tmp_path / "multisigs.json"),
        "BTCLI_MULTISIG_CACHE": str(tmp_path / "multisig_cache.json"),
        "BTCLI_SUBNET_NAMES_CACHE": str(tmp_path / "subnet_names.json"),
        "BTCLI_TOKEN_SYMBOLS_CACHE": str(tmp_path / "token_symbols.json"),
    }.items():
        monkeypatch.setenv(var, value)

    def cli(*args: str):
        return runner.invoke(app, ["--json", *args])

    result = cli("subnets", "emission", "show", "--netuid", str(netuid))
    assert result.exit_code == 0, result.output
    shown = json.loads(result.output)
    assert shown["netuid"] == netuid
    assert shown["weights"] == ew.default_weights()
    assert shown["root_share"] == pytest.approx(shown["baseline_root_share"], abs=1e-9)
    assert shown["root_capped"] is False
    assert shown["min_validator"] > 0
    assert shown["rate_limit"]["owner_can_change_now"] is True

    # A vector the chain would reject exits 2 and names the rule and the range.
    result = cli(
        "subnets", "emission", "set-value", "--netuid", str(netuid), "--slot", "validator",
        "--value", "1%", "--dry-run",
    )  # fmt: skip
    assert result.exit_code == 2, result.output
    rejected = _stderr_json(result)
    assert "ValidatorEmissionTooLowToPreserveRoot" in rejected["error"]
    assert f"allowed on subnet {netuid} right now: validator" in rejected["note"]

    # A valid dry run exits 0 with a structured preview.
    result = cli(
        "subnets", "emission", "set-value", "--netuid", str(netuid), "--slot", "validator",
        "--value", "45%", "--dry-run",
    )  # fmt: skip
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert plan["ok"] is True and plan["violations"] == []
    assert plan["preview"]["after"]["shares"]["validator"] == pytest.approx(0.45, abs=1e-9)
    baseline = plan["preview"]["root"]["baseline_share"]
    assert plan["preview"]["root"]["share_after"] == pytest.approx(baseline, abs=1e-9)
    assert plan["preview"]["root"]["validators_after"] == pytest.approx(0.45 - baseline, abs=1e-9)
    assert plan["preview"]["root"]["capped_after"] is False

    # The real submit: root still earns the baseline, validators the rest.
    result = cli(
        "subnets", "emission", "set-value", "--netuid", str(netuid), "--slot", "validator",
        "--value", "45%", "--yes",
    )  # fmt: skip
    assert result.exit_code == 0, result.output
    submitted = json.loads(result.output)
    assert submitted["success"] is True and submitted["extrinsic_id"]

    result = cli("subnets", "emission", "show", "--netuid", str(netuid))
    assert result.exit_code == 0, result.output
    shown = json.loads(result.output)
    assert shown["shares"]["validator"] == pytest.approx(0.45, abs=1e-9)
    assert shown["baseline_root_share"] == pytest.approx(baseline, rel=1e-3)
    assert shown["root_share"] == pytest.approx(shown["baseline_root_share"], abs=1e-9)
    assert shown["validator_share_after_root"] == pytest.approx(
        0.45 - shown["baseline_root_share"], abs=1e-9
    )
    assert shown["rate_limit"]["last_change_block"] > 0

    # Inside the owner window a second change is blocked, not rejected (exit 1).
    if not shown["rate_limit"]["owner_can_change_now"]:
        result = cli(
            "subnets", "emission", "increase", "--netuid", str(netuid), "--slot", "miner",
            "--by", "1%", "--dry-run",
        )  # fmt: skip
        assert result.exit_code == 1, result.output
        blocked = json.loads(result.output)
        assert blocked["ok"] is False and "root is exempt" in blocked["violations"][0]
