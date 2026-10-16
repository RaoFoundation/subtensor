"""Weight clipping must not turn invalid validator scores into valid votes."""

from unittest.mock import AsyncMock, Mock

import pytest

from bittensor.intents.weights import CommitWeights, SetWeights, clip_to_max_weight
from bittensor.result import BittensorError
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import dev_wallet


@pytest.mark.parametrize("bad", [-1.0, float("nan"), float("inf"), float("-inf")])
@pytest.mark.parametrize("limit", [0.5, 0.75])
def test_clipping_rejects_invalid_scores(bad, limit):
    with pytest.raises(BittensorError, match="finite and non-negative"):
        clip_to_max_weight([bad, 2.0], limit)


@pytest.mark.parametrize(
    "weights, limit, expected",
    [
        ([1.0, 3.0], 0.5, [0.5, 0.5]),
        ([1.0, 2.0, 3.0], 0.5, [1 / 6, 2 / 6, 3 / 6]),
        ([0.0, 1.0, 3.0], 0.6, [0.0, 0.4, 0.6]),
    ],
)
def test_clipping_preserves_valid_scores(weights, limit, expected):
    assert clip_to_max_weight(weights, limit) == pytest.approx(expected)


@pytest.mark.parametrize("bad", [-1.0, float("nan"), float("inf"), float("-inf")])
@pytest.mark.parametrize(
    "intent_type, commit_reveal",
    [(SetWeights, False), (SetWeights, True), (CommitWeights, True)],
)
async def test_invalid_scores_rejected_before_composing_or_encrypting(
    monkeypatch, bad, intent_type, commit_reveal
):
    substrate = FakeSubstrate()
    # With two scores this limit triggers the uniform-distribution shortcut.
    substrate.seed_default("SubtensorModule", "MaxWeightsLimit", 32_767)
    substrate.seed_default("SubtensorModule", "CommitRevealWeightsEnabled", commit_reveal)
    compose = AsyncMock(wraps=substrate.compose)
    encrypt = Mock(return_value=(b"commit", 1_000))
    monkeypatch.setattr(substrate, "compose", compose)
    monkeypatch.setattr("bittensor.intents.weights._core.get_encrypted_commit_v2", encrypt)

    intent = intent_type(netuid=1, weights={1: bad, 2: 2.0})
    with pytest.raises(BittensorError, match="finite and non-negative"):
        await intent.build(substrate, dev_wallet())
    compose.assert_not_called()
    encrypt.assert_not_called()
