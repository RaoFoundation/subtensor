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


# The SDK must not reject a valid capped Null row locally when the stored minimum
# exceeds the cap, completely blocking validator weight submission.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.asyncio
async def test_null_effective_minimum_allows_capped_row_and_rejects_shorter_row():
    from bittensor.intents import SetWeights
    from bittensor.result import ChainError
    from tests.harness.fake_substrate import FakeSubstrate
    from tests.harness.samples import dev_wallet

    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "SubnetEpochConsensus", "Null")
    substrate.seed_default("SubtensorModule", "ValidatorPermit", [True])
    substrate.seed_default("SubtensorModule", "MinAllowedWeights", 1_024)
    substrate.seed_default("SubtensorModule", "MechanismCountCurrent", 16)
    substrate.seed_default("SubtensorModule", "SubnetworkN", 256)

    built = await SetWeights(netuid=1, uids=list(range(156)), weights=[1] * 156).build(
        substrate, dev_wallet()
    )
    assert built.function == "set_mechanism_weights"
    assert built.params["dests"] == list(range(156))

    with pytest.raises(ChainError, match="at least 156 weights"):
        await SetWeights(netuid=1, uids=list(range(155)), weights=[1] * 155).build(
            substrate, dev_wallet()
        )


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("mode", "mechanisms", "subnet_n", "expected_minimum"),
    [
        ("Null", 1, 100, 100),
        ("Null", 16, 256, 156),
        ("Yuma", 16, 100, 1_024),
    ],
)
async def test_weight_preflight_effective_minimum_matches_runtime(
    mode, mechanisms, subnet_n, expected_minimum
):
    from bittensor.intents.weights import _preflight
    from tests.harness.fake_substrate import FakeSubstrate
    from tests.harness.samples import dev_wallet

    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "SubnetEpochConsensus", mode)
    substrate.seed_default("SubtensorModule", "ValidatorPermit", [True])
    substrate.seed_default("SubtensorModule", "MinAllowedWeights", 1_024)
    substrate.seed_default("SubtensorModule", "MechanismCountCurrent", mechanisms)
    substrate.seed_default("SubtensorModule", "SubnetworkN", subnet_n)

    preflight = await _preflight(substrate, dev_wallet().hotkey.ss58_address, 1, 0)
    assert preflight.min_allowed_weights == expected_minimum


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("mode", "mechanisms", "expected_cap"),
    [
        ("Null", 1, 2_500),
        ("Null", 2, 1_250),
        ("Null", 3, 833),
        ("Null", 16, 156),
        ("Yuma", 1, None),
        ("Yuma", 16, None),
    ],
)
async def test_weight_preflight_row_cap_matches_runtime(mode, mechanisms, expected_cap):
    from bittensor.intents.weights import _preflight
    from tests.harness.fake_substrate import FakeSubstrate
    from tests.harness.samples import dev_wallet

    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "SubnetEpochConsensus", mode)
    substrate.seed_default("SubtensorModule", "ValidatorPermit", [True])
    substrate.seed_default("SubtensorModule", "MechanismCountCurrent", mechanisms)

    preflight = await _preflight(substrate, dev_wallet().hotkey.ss58_address, 1, 0)
    assert preflight.max_row_len == expected_cap


def test_conform_enforces_the_null_row_cap():
    from bittensor.result import ChainError, ErrorCode

    capped = _Preflight(0, False, 0, 65_535, max_row_len=156)
    uids = list(range(1, 158))
    # A row at the cap passes, in the relative and in the raw form.
    assert len(_conform(uids[:156], [1.0] * 156, capped, 1)[0]) == 156
    assert len(_conform(uids[:156], [7] * 156, capped, 1, True)[0]) == 156
    # One entry more is refused with the code of the chain's error.
    for raw_u16, weights in ((False, [1.0] * 157), (True, [7] * 157)):
        with pytest.raises(ChainError, match="at most 156 weights") as error:
            _conform(uids, weights, capped, 1, raw_u16)
        assert error.value.code == ErrorCode.LIMIT_EXCEEDED
        assert error.value.name == "TooManyWeightsForNullMechanism"
    # Zero weights are not submitted, so they do not count.
    assert len(_conform(uids, [1.0] * 156 + [0.0], capped, 1)[0]) == 156
    # Without a cap (Yuma) the same row passes.
    assert len(_conform(uids, [1.0] * 157, _Preflight(0, False, 0, 65_535), 1)[0]) == 157


@pytest.mark.asyncio
@pytest.mark.parametrize("commit_reveal", [False, True])
async def test_null_row_above_the_cap_is_rejected_before_signing_or_encrypting(
    monkeypatch, commit_reveal
):
    from bittensor.intents import CommitWeights, SetWeights
    from bittensor.result import ChainError, ErrorCode
    from tests.harness.fake_substrate import FakeSubstrate
    from tests.harness.samples import dev_wallet

    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "SubnetEpochConsensus", "Null")
    substrate.seed_default("SubtensorModule", "ValidatorPermit", [True])
    substrate.seed_default("SubtensorModule", "CommitRevealWeightsEnabled", commit_reveal)
    substrate.seed_default("SubtensorModule", "MechanismCountCurrent", 16)
    substrate.seed_default("SubtensorModule", "SubnetworkN", 2_500)

    def unexpected_encrypt(**kwargs):
        raise AssertionError("A row above the cap must be rejected before encryption")

    monkeypatch.setattr(
        "bittensor.intents.weights._core.get_encrypted_commit_v2", unexpected_encrypt
    )
    row = {uid: 1 for uid in range(157)}
    for intent in (
        SetWeights(netuid=1, mechid=15, weights=row, raw_u16=True),
        CommitWeights(netuid=1, mechid=15, weights=row),
    ):
        with pytest.raises(ChainError, match="at most 156 weights") as error:
            await intent.build(substrate, dev_wallet())
        assert error.value.code == ErrorCode.LIMIT_EXCEEDED
