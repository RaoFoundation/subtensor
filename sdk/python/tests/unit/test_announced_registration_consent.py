"""Saved proxy bytes must obey the same registration rule as fresh wrappers."""

from unittest.mock import AsyncMock, Mock

import pytest

from bittensor import Balance
from bittensor.executor import Executor
from bittensor.hashed import registration_descriptor_value
from bittensor.intents.proxy import ExecuteProxyAnnounced
from bittensor.sp_core import Keypair
from tests.harness.fake_substrate import ComposedCall, FakeSubstrate
from tests.harness.samples import ALICE, BOB, dev_wallet


def replay_fixture(register=True):
    chain = FakeSubstrate()
    chain.seed_default("System", "Account", {"data": {"free": 100 * 10**9, "frozen": 0}})
    recipient = Keypair.create_from_seed(bytes([77]) * 32, 4)
    descriptor = registration_descriptor_value(bytes(recipient.hashed_descriptor))
    guarded = ComposedCall(
        "HashedAccounts", "register" if register else "check_registered", {"descriptor": descriptor}
    )
    transfer = ComposedCall(
        "Balances", "transfer_keep_alive", {"dest": recipient.ss58_address, "value": 10**9}
    )
    inner = ComposedCall("Utility", "batch_all", {"calls": [guarded, transfer]})

    def decoded(call):
        return {
            "call_module": call.module,
            "call_function": call.function,
            "call_args": [
                {
                    "name": name,
                    "value": [decoded(child) for child in value] if name == "calls" else value,
                }
                for name, value in call.params.items()
            ],
        }

    async def decode_saved(kind, data):
        assert kind == "Call"
        assert bytes(data) == bytes(inner.data)
        return decoded(inner)

    chain.decode_scale = AsyncMock(side_effect=decode_saved)
    intent = ExecuteProxyAnnounced(
        delegate_ss58=ALICE,
        real_ss58=BOB,
        inner_op="transfer",
        inner_args={"dest_ss58": recipient.ss58_address, "amount_tao": 1},
        announced_call_data="0x" + bytes(inner.data).hex(),
        expected_call_hash="0x" + bytes(inner.call_hash).hex(),
    )
    return chain, Executor(chain), intent, inner


# Saved announced calls cannot bypass the direct-sponsor requirement and lock
# funds without approval by omitting registration metadata.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.asyncio
async def test_saved_registration_is_rejected_before_private_key_access_or_submission(monkeypatch):
    chain, executor, intent, _ = replay_fixture()
    unlock = Mock(
        side_effect=AssertionError("private key accessed before registration authorization")
    )
    monkeypatch.setattr("bittensor.executor.resolve_signer", unlock)
    consent = Mock(return_value=False)
    with pytest.raises(ValueError, match="register the hashed recipient first"):
        await executor.execute(
            intent,
            dev_wallet(),
            max_registration_deposit=Balance.from_rao(0),
            on_registration_sponsorship=consent,
        )
    unlock.assert_not_called()
    assert not chain.submissions


@pytest.mark.asyncio
async def test_saved_check_only_registration_guard_preserves_bytes_and_needs_no_consent():
    chain, executor, intent, inner = replay_fixture(register=False)
    consent = Mock(side_effect=AssertionError("registered recipient needs no new reserve"))
    result = await executor.execute(
        intent,
        dev_wallet(),
        max_registration_deposit=Balance.from_rao(0),
        on_registration_sponsorship=consent,
    )
    assert result.success
    assert bytes(chain.last_call.params["call"].data) == bytes(inner.data)
    consent.assert_not_called()
