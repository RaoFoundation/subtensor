"""Spend caps cover runtime-priced burns separately from transaction fees."""

import pytest

from bittensor.balance import Balance
from bittensor.intents._money import UNBOUNDED
from bittensor.intents.batch import Batch
from bittensor.intents.coldkey import AnnounceColdkeySwap, SwapColdkeyAnnounced
from bittensor.intents.plan import Policy
from bittensor.intents.registration import RootRegister, SwapHotkey


# Burns and full coldkey migration cannot bypass the caller's TAO spend cap.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize(
    "intent",
    [
        RootRegister(hotkey_ss58="hotkey"),
        SwapHotkey(hotkey_ss58="hotkey", new_hotkey_ss58="other"),
        SwapHotkey(hotkey_ss58="hotkey", new_hotkey_ss58="other", netuid=1),
        AnnounceColdkeySwap(new_coldkey_ss58="other"),
        SwapColdkeyAnnounced(new_coldkey_ss58="other"),
    ],
)
def test_runtime_priced_account_operations_enforce_spend_caps(intent):
    assert intent.spend() is UNBOUNDED
    zero_fee = Balance.from_rao(0)
    for cap in [Balance.from_rao(0), Balance.from_rao((1 << 64) - 1)]:
        policy = Policy(max_spend_tao=cap, max_fee_tao=zero_fee)
        assert any("cannot be bounded" in reason for reason in policy.check(intent, zero_fee))
        batch = Batch([intent])
        assert batch.spend() is UNBOUNDED
        assert any("cannot be bounded" in reason for reason in policy.check(batch, zero_fee))
    assert Policy(max_fee_tao=zero_fee).check(intent, zero_fee) == []
