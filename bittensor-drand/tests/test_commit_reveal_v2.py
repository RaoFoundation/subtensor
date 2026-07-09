"""API contract tests for get_encrypted_commit_v2 (no epoch simulation in Python)."""

import pytest
import bittensor_drand as btcr

# cycle_reset row from src/epoch_schedule_vectors.rs (subtensor tempo_control.rs:195)
_VECTOR_KWARGS = dict(
    uids=[0, 1],
    weights=[100, 200],
    version_key=1,
    last_epoch_block=10,
    pending_epoch_at=0,
    subnet_epoch_index=0,
    tempo=50,
    blocks_since_last_step=0,
    current_block=10,
    subnet_reveal_period_epochs=1,
    block_time=12.0,
    hotkey=bytes([1, 2, 3]),
)


def test_get_encrypted_commit_v2_returns_bytes_and_positive_round():
    encrypted, reveal_round = btcr.get_encrypted_commit_v2(**_VECTOR_KWARGS)
    assert isinstance(encrypted, bytes)
    assert len(encrypted) > 0
    assert isinstance(reveal_round, int)
    assert reveal_round > 0


def test_get_encrypted_commit_v2_is_deterministic_for_fixed_inputs():
    first = btcr.get_encrypted_commit_v2(**_VECTOR_KWARGS)
    second = btcr.get_encrypted_commit_v2(**_VECTOR_KWARGS)
    assert first[1] == second[1]


def test_get_encrypted_commit_v2_tempo_zero_raises():
    kwargs = {**_VECTOR_KWARGS, "tempo": 0}
    with pytest.raises(ValueError):
        btcr.get_encrypted_commit_v2(**kwargs)
