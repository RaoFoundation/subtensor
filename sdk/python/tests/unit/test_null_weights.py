"""Exact integer ratios across large Null rows and CLI file input."""

import json
from unittest.mock import AsyncMock

import pytest

from bittensor.cli.commands.weights import _weight_input
from bittensor.intents.weights import _conform, _ensure_raw_null, _Preflight, normalize
from bittensor.result import BittensorError


def test_raw_full_row_preserves_small_weights():
    uids = list(range(2_500))
    values = [65_534] + [1] * 2_499
    assert _conform(uids, values, _Preflight(0, False, 1, 65_535), 1, True) == (uids, values)


@pytest.mark.parametrize("bad", [-1, 65_536, 1.5, float("inf"), float("nan")])
def test_raw_rejects_invalid_values(bad):
    with pytest.raises(BittensorError):
        _conform([1], [bad], _Preflight(0, False, 1, 65_535), 1, True)


def test_raw_checks_limit_without_clipping():
    with pytest.raises(BittensorError, match="max_weight_limit"):
        _conform([1, 2], [100, 1], _Preflight(0, False, 1, 32_767), 1, True)
    assert normalize([1, 2], [3, 1]) == ([1, 2], [65_535, 21_845])


@pytest.mark.asyncio
async def test_raw_requires_null_consensus():
    substrate = AsyncMock()
    substrate.query.return_value = "Yuma"
    with pytest.raises(BittensorError, match="Null"):
        await _ensure_raw_null(substrate, 1)
    substrate.query.return_value = "Null"
    await _ensure_raw_null(substrate, 1)


def test_cli_full_row_file(tmp_path):
    data = {str(uid): 1 for uid in range(2_500)}
    data["0"] = 65_534
    path = tmp_path / "weights.json"
    path.write_text(json.dumps(data))
    assert _weight_input(None, None, path, True) == (None, data)


@pytest.mark.asyncio
async def test_owner_consensus_intent_uses_enum():
    from bittensor.intents import SetHyperparameter

    substrate = AsyncMock()
    await SetHyperparameter(netuid=1, name="epoch_consensus", value="Null").build(substrate, None)
    call = substrate.compose.call_args.args[0]
    assert call.module == "AdminUtils"
    assert call.function == "sudo_set_epoch_consensus"
    assert call.params == {"netuid": 1, "mode": "Null"}
    with pytest.raises(ValueError, match="Yuma or Null"):
        SetHyperparameter(netuid=1, name="epoch_consensus", value=2)


@pytest.mark.asyncio
@pytest.mark.parametrize("commit_reveal", [False, True])
async def test_full_row_intent_composes_exact_values(monkeypatch, commit_reveal):
    from bittensor.intents import SetWeights
    from tests.harness.fake_substrate import FakeSubstrate
    from tests.harness.samples import dev_wallet

    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "SubnetEpochConsensus", "Null")
    substrate.seed_default("SubtensorModule", "ValidatorPermit", [True])
    substrate.seed_default("SubtensorModule", "CommitRevealWeightsEnabled", commit_reveal)
    values = [65_534] + [1] * 2_499
    encrypted = []

    def encrypt(**kwargs):
        encrypted.append(kwargs)
        return bytes(17_000), 1_000

    monkeypatch.setattr("bittensor.intents.weights._core.get_encrypted_commit_v2", encrypt)
    built = await SetWeights(netuid=1, uids=list(range(2_500)), weights=values, raw_u16=True).build(
        substrate, dev_wallet()
    )
    if commit_reveal:
        assert encrypted[0]["weights"] == values
        assert encrypted[0]["uids"] == list(range(2_500))
        assert built.call.function == "commit_timelocked_mechanism_weights"
        assert len(built.call.params["commit"]) == 17_000
    else:
        assert built.function == "set_mechanism_weights"
        assert built.params["weights"] == values


@pytest.mark.asyncio
async def test_null_nonpermit_rejected_before_composing_or_encrypting(monkeypatch):
    from bittensor.intents import SetWeights
    from bittensor.result import ChainError, ErrorCode
    from tests.harness.fake_substrate import FakeSubstrate
    from tests.harness.samples import dev_wallet

    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "SubnetEpochConsensus", "Null")
    substrate.seed_default("SubtensorModule", "ValidatorPermit", [False, True])
    substrate.seed_default("SubtensorModule", "CommitRevealWeightsEnabled", True)

    def unexpected_encrypt(**kwargs):
        raise AssertionError("A nonpermit holder must be rejected before encryption")

    monkeypatch.setattr(
        "bittensor.intents.weights._core.get_encrypted_commit_v2", unexpected_encrypt
    )
    with pytest.raises(ChainError) as error:
        await SetWeights(netuid=1, weights={0: 1}, raw_u16=True).build(substrate, dev_wallet())
    assert error.value.code == ErrorCode.NOT_AUTHORIZED
