from bittensor.metagraph import _commitments
from bittensor.reads.locks import _eligible_alpha


def test_eligible_alpha_excludes_protocol_and_burned_balances() -> None:
    assert _eligible_alpha(8_000_000, 1_000_000, 1_000_000) == 6_000_000
    assert _eligible_alpha(100, 80, 30) == 0


def test_failed_timelock_commitment_is_terminal_and_not_revealed() -> None:
    commitments = _commitments(
        netuid=42,
        committed=[
            (
                "5Fhotkey",
                {
                    "block": 100,
                    "deposit": 0,
                    "info": {
                        "fields": [
                            {
                                "TimelockRevealFailed": {
                                    "encrypted": "0x0102",
                                    "reveal_round": 123,
                                }
                            }
                        ]
                    },
                },
            )
        ],
        revealed=[],
        queried_block=110,
    )

    commitment = commitments["5Fhotkey"]
    assert commitment.status == "failed"
    assert commitment.is_revealed is False
    assert commitment.value is None
    assert commitment.reveals_at is None
