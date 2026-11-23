"""Channel commitments, burn pricing, and mechanism-scoped weight delegates."""

import pytest
from typer.testing import CliRunner

from bittensor import wallets
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.intents import (
    CommitWeightHash,
    CommitWeights,
    RevealWeights,
    SetChannelCommitment,
    SetCommitBurn,
    SetMechWeightDelegate,
    SetWeights,
)
from bittensor.intents.base import BuiltCall
from bittensor.reads.identity import channel_commitment, commit_burn
from bittensor.result import ChainError
from tests.conftest import golden_codec
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE_HOT, BOB, BOB_HOT, dev_wallet


@pytest.mark.asyncio
async def test_delegate_set_and_revoke():
    substrate = FakeSubstrate()
    wallet = dev_wallet()
    call = await SetMechWeightDelegate(netuid=3, mechid=2, delegate=BOB).build(substrate, wallet)
    assert call.function == "set_mech_weight_delegate"
    assert call.params == {"netuid": 3, "mecid": 2, "delegate": BOB}
    call = await SetMechWeightDelegate(netuid=3).build(substrate, wallet)
    assert call.params["delegate"] is None


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("intent", "method", "expected"),
    [
        (
            SetWeights(netuid=1, mechid=2, weights={0: 1}, for_hotkey=BOB_HOT),
            "set_mechanism_weights_for",
            {
                "netuid": 1,
                "mecid": 2,
                "hotkey": BOB_HOT,
                "dests": [0],
                "weights": [65535],
                "version_key": 0,
            },
        ),
        (
            RevealWeights(netuid=1, mechid=2, uids=[0], weights=[1], salt=[1], for_hotkey=BOB_HOT),
            "reveal_mechanism_weights_for",
            {
                "netuid": 1,
                "mecid": 2,
                "hotkey": BOB_HOT,
                "uids": [0],
                "values": [65535],
                "salt": [1],
                "version_key": 0,
            },
        ),
        (
            CommitWeightHash(netuid=1, mechid=2, commit_hash="0x" + "ab" * 32, for_hotkey=BOB_HOT),
            "commit_mechanism_weights_for",
            {"netuid": 1, "mecid": 2, "hotkey": BOB_HOT, "commit_hash": "0x" + "ab" * 32},
        ),
    ],
)
async def test_all_legacy_delegate_calls_use_target_hotkey(intent, method, expected):
    substrate = FakeSubstrate()
    substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_mech_weight_delegate", ALICE_HOT)
    call = await intent.build(substrate, dev_wallet())
    assert call.function == method
    assert call.params == expected


@pytest.mark.asyncio
async def test_legacy_reveal_keeps_original_main_mechanism_call():
    call = await RevealWeights(netuid=1, uids=[0], weights=[1], salt=[1]).build(
        FakeSubstrate(), dev_wallet()
    )
    assert call.function == "reveal_weights"
    assert "mecid" not in call.params
    assert "hotkey" not in call.params


@pytest.mark.asyncio
async def test_delegate_timelock_encrypts_for_validator_not_delegate(monkeypatch):
    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "CommitRevealWeightsEnabled", True)
    substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_mech_weight_delegate", ALICE_HOT)
    encrypted = []

    def encrypt(**kwargs):
        encrypted.append(kwargs)
        return b"sealed", 12345

    monkeypatch.setattr("bittensor.intents.weights._core.get_encrypted_commit_v2", encrypt)
    intent = CommitWeights(netuid=1, mechid=2, weights={0: 1}, for_hotkey=BOB_HOT)
    built = await intent.build(substrate, dev_wallet())
    assert isinstance(built, BuiltCall)
    assert encrypted[0]["hotkey"] == bytes(dev_wallet(hot="//Bob//hot").hotkey.public_key)
    assert built.call.function == "commit_timelocked_mechanism_weights_for"
    assert built.call.params["hotkey"] == BOB_HOT
    assert built.extras == {"reveal_round": 12345}


@pytest.mark.asyncio
async def test_delegate_missing_authorization_is_rejected():
    substrate = FakeSubstrate()
    substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_mech_weight_delegate", None)
    with pytest.raises(ChainError, match="no weight delegate"):
        await SetWeights(netuid=1, weights={0: 1}, for_hotkey=BOB_HOT).build(
            substrate, dev_wallet()
        )


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("name", "value", "method", "param", "expected"),
    [
        ("enabled", "true", "sudo_set_commit_burn_enabled", "enabled", True),
        ("min_burn", "0.5", "sudo_set_commit_burn_min", "value", 500_000_000),
        ("max_burn", 2, "sudo_set_commit_burn_max", "value", 2_000_000_000),
        ("half_life", "360", "sudo_set_commit_burn_half_life", "value", 360),
        (
            "increase_mult",
            "1.25",
            "sudo_set_commit_burn_increase_mult",
            "value",
            {"bits": 5 * 2**62},
        ),
        (
            "increase_mult",
            "1.26",
            "sudo_set_commit_burn_increase_mult",
            "value",
            {"bits": (63 * 2**64) // 50},
        ),
    ],
)
async def test_commit_burn_setters(name, value, method, param, expected):
    intent = SetCommitBurn(netuid=1, mechid=2, channel=7, name=name, value=value)
    call = await intent.build(FakeSubstrate(), dev_wallet())
    assert call.function == method
    assert call.params == {"netuid": 1, "mecid": 2, "channel": 7, param: expected}


@pytest.mark.asyncio
async def test_commitment_payload_uses_exact_runtime_shape():
    substrate = FakeSubstrate()
    intent = SetChannelCommitment(netuid=1, mechid=2, channel=7, data="hello", max_burn_tao="1.25")
    built = await intent.build(substrate, dev_wallet())
    assert isinstance(built, BuiltCall)
    assert built.call.module == "Commitments"
    assert built.call.function == "set_commitment_to"
    assert built.call.params == {
        "netuid": 1,
        "mecid": 2,
        "channel": 7,
        "hotkey": ALICE_HOT,
        "info": {"fields": [{"Raw5": b"hello"}]},
        "max_burn": 1_250_000_000,
    }
    assert intent.spend().rao == 1_250_000_000


@pytest.mark.asyncio
async def test_timelocked_commitment_uses_inner_ciphertext(monkeypatch):
    monkeypatch.setattr(
        "bittensor.intents.commitments._core.encrypt_at_round",
        lambda payload, round_number: (b"inner-tle", round_number),
    )
    built = await SetChannelCommitment(
        netuid=1, data="hello", reveal_round=12_345, max_burn_tao=0
    ).build(FakeSubstrate(), dev_wallet())
    assert built.call.params["info"] == {
        "fields": [{"TimelockEncrypted": {"encrypted": b"inner-tle", "reveal_round": 12_345}}]
    }
    assert built.extras == {"reveal_round": 12_345}
    empty = await SetChannelCommitment(netuid=1, data="").build(FakeSubstrate(), dev_wallet())
    assert empty.call.params["info"] == {"fields": []}


@pytest.mark.asyncio
async def test_live_and_stored_burn_reads_are_distinguished():
    live = {
        "enabled": True,
        "burn": 2_000_000_000,
        "min_burn": 500_000_000,
        "max_burn": 10_000_000_000,
        "half_life": 360,
        "increase_mult": {"bits": 5 * 2**62},
        "last_update_block": 100,
    }
    client = Client("local", substrate=FakeSubstrate())
    client._substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_commit_burn", live)
    result = await commit_burn(client, netuid=1, mechid=2, channel=7)
    assert result["burn"].rao == 2_000_000_000
    assert result["increase_mult"] == "1.25"
    assert result["price_is_decayed"] is True
    client._substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_commit_burn", None)
    client._substrate.seed("Commitments", "CommitBurnConfig", [(2 * 4096 + 1, 7)], live)
    result = await commit_burn(client, netuid=1, mechid=2, channel=7)
    assert result["price_is_decayed"] is False


@pytest.mark.asyncio
async def test_stored_burn_uses_tuple_key():
    class View:
        async def at(self):
            return self

        async def runtime(self, item, params):
            assert item.name == "get_commit_burn"
            return None

        async def query(self, item, params):
            assert item.name == "CommitBurnConfig"
            assert params == [(4097, 7)]
            return None

    assert await commit_burn(View(), netuid=1, mechid=1, channel=7) is None


@pytest.mark.asyncio
@pytest.mark.parametrize("encoding", ["bytes", "hex", "utf-8", "utf-8 with 0x prefix"])
async def test_channel_commitment_scale_bytes_decode_to_fields(monkeypatch, encoding):
    substrate = FakeSubstrate()
    deposit = int.from_bytes(b"0x", "little") if encoding == "utf-8 with 0x prefix" else 0
    record = {"deposit": deposit, "block": 100, "info": {"fields": [{"Raw5": "0x636166c3a9"}]}}
    encoded = golden_codec().encode("scale_info::513", record)
    if encoding == "hex":
        encoded = "0x" + encoded.hex()
    elif encoding.startswith("utf-8"):
        encoded = encoded.decode("utf-8")
    substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_channel_commitment", encoded)

    async def decode(data, block_hash=None):
        entry = golden_codec().storage_entry("Commitments", "CommitmentOf")
        return golden_codec().decode(entry.value_type, data)

    monkeypatch.setattr(substrate, "decode_commitment_registration", decode, raising=False)
    result = await channel_commitment(
        Client("local", substrate=substrate), netuid=1, hotkey_ss58=ALICE_HOT
    )
    assert result.data == "café"
    assert result.block == 100
    assert result.deposit.rao == deposit


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("data", "expected"),
    [(b"revealed", "revealed"), ("0xabc", "0xabc"), ("0x6869", "0x6869"), (b"\xff", "0xff")],
)
async def test_revealed_channel_remains_readable_without_active_row(data, expected):
    substrate = FakeSubstrate()
    substrate.seed_runtime("SubnetRegistrationRuntimeApi", "get_channel_commitment", None)
    substrate.seed("Commitments", "ChannelRevealedCommitments", [1, ALICE_HOT, 0], [(data, 101)])
    result = await channel_commitment(
        Client("local", substrate=substrate), netuid=1, hotkey_ss58=ALICE_HOT
    )
    assert result is not None
    assert result.fields == []
    assert result.revealed == [{"data": expected, "block": 101}]


@pytest.mark.asyncio
async def test_transport_resolves_channel_registration_type_from_metadata():
    from bittensor._substrate import RpcSubstrate

    substrate = RpcSubstrate("ws://unused")
    record = {"deposit": 0, "block": 100, "info": {"fields": [{"Raw5": "0x68656c6c6f"}]}}
    encoded = golden_codec().encode("scale_info::513", record)

    class Raw:
        async def decode_commitment_registration(self, data, block_hash=None):
            entry = golden_codec().storage_entry("Commitments", "CommitmentOf")
            assert entry.value_type == "scale_info::513"
            return golden_codec().decode(entry.value_type, data)

    substrate._substrate = Raw()
    assert await substrate.decode_commitment_registration(encoded) == record
    entry = golden_codec().storage_entry("Commitments", "CommitmentOf")
    assert golden_codec().decode(entry.value_type, encoded) == record


def test_new_cli_commands_are_wired():
    result = CliRunner().invoke(app, ["weights", "delegate", "--help"])
    assert result.exit_code == 0, result.output
    assert all(word in result.output for word in ("set", "revoke", "show"))
    result = CliRunner().invoke(app, ["commitments", "commit", "--help"])
    assert result.exit_code == 0, result.output
    for flag in ("--mechid", "--channel", "--max-burn", "--data"):
        assert flag in result.output
    result = CliRunner().invoke(app, ["weights", "reveal", "--help"])
    assert "--for-hotkey" in result.output and "--mechid" in result.output


@pytest.fixture()
def cli_wallet(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "btcli.json"))
    monkeypatch.setenv("BTCLI_ADDRESSES_PATH", str(tmp_path / "addresses.json"))
    monkeypatch.setenv("BTCLI_PROXIES_PATH", str(tmp_path / "proxies.json"))
    monkeypatch.setenv("BTCLI_MULTISIGS_PATH", str(tmp_path / "multisigs.json"))
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path / "wallets"))
    wallets.create(
        name="validator", hotkey="default", path=tmp_path / "wallets", use_password=False
    )
    return str(tmp_path / "wallets")


def test_commitment_cli_dry_run(cli_wallet, monkeypatch):
    substrate = FakeSubstrate()
    substrate.seed_default(
        "System", "Account", {"data": {"free": 10**12, "reserved": 0, "frozen": 0}}
    )
    monkeypatch.setattr(
        "bittensor.cli.context.Client",
        lambda network, **kwargs: Client(network, substrate=substrate),
    )
    result = CliRunner().invoke(
        app,
        [
            "--json",
            "--dry-run",
            "commitments",
            "commit",
            "--netuid",
            "1",
            "--mechid",
            "2",
            "--channel",
            "7",
            "--data",
            "hello",
            "--max-burn",
            "1.25",
            "-w",
            "validator",
        ],
    )
    assert result.exit_code == 0, result.output
    assert substrate.submissions == []
    assert '"max_burn_tao": "1.25"' in result.output


def test_delegate_cli_dry_run(cli_wallet, monkeypatch):
    substrate = FakeSubstrate()
    monkeypatch.setattr(
        "bittensor.cli.context.Client",
        lambda network, **kwargs: Client(network, substrate=substrate),
    )
    result = CliRunner().invoke(
        app,
        [
            "--json",
            "--dry-run",
            "weights",
            "delegate",
            "set",
            "--netuid",
            "1",
            "--mechid",
            "2",
            "--delegate",
            BOB,
            "-w",
            "validator",
        ],
    )
    assert result.exit_code == 0, result.output
    assert substrate.submissions == []
    assert '"set_mech_weight_delegate"' in result.output
