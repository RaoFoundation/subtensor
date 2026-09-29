from unittest.mock import patch

import pytest

from bittensor._generated.errors import ERRORS
from bittensor.intents import EnableNullConsensus, PowRegister, SetWeightsV2
from bittensor.intents.null_consensus import pow_seal
from bittensor.result import BittensorError, ErrorCode, chain_error_from_dispatch
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import dev_wallet


@pytest.mark.parametrize(
    "name, remedy",
    [
        ("NullConsensusRequiresPowRegistration", "btcli pow register"),
        ("NullConsensusRequiresU32Weights", "set_weights_v2"),
        ("NullConsensusPowRegistrationDisabled", "NetworkPowRegistrationAllowed"),
        ("PowWorkAlreadyUsed", "new nonce or block"),
    ],
)
def test_dispatch_errors_preserve_the_specific_failure_and_remedy(name, remedy):
    pallet, index = next(key for key, info in ERRORS.items() if info.name == name)
    error = chain_error_from_dispatch({"Module": {"index": pallet, "error": [index, 0, 0, 0]}})
    assert error.name == name
    assert error.code is not ErrorCode.UNKNOWN
    assert remedy in error.message


@pytest.mark.asyncio
async def test_pow_register_solves_runtime_seal_and_preserves_coldkey_signing():
    fake = FakeSubstrate()
    for name in ("NullConsensus", "NetworkRegistrationAllowed", "NetworkPowRegistrationAllowed"):
        fake.seed("SubtensorModule", name, [1], True)
    fake.seed("SubtensorModule", "Difficulty", [1], 1)
    wallet = dev_wallet()
    call = await PowRegister(netuid=1).build(fake, wallet)
    module, name, args = call
    assert (module, name) == ("SubtensorModule", "register")
    assert args["hotkey"] == wallet.hotkey.ss58_address
    assert args["coldkey"] == wallet.coldkeypub.ss58_address
    block_hash = bytes.fromhex((await fake.block_hash(args["block_number"]))[2:])
    assert bytes(args["work"]) == pow_seal(block_hash, wallet.hotkey.public_key, args["nonce"])


@pytest.mark.asyncio
async def test_pow_refuses_legacy_subnet_before_composing_burn_alias():
    fake = FakeSubstrate()
    with pytest.raises(BittensorError, match="enabled null-consensus"):
        await PowRegister(netuid=1).build(fake, dev_wallet())


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "disabled_flag, message",
    [
        ("NetworkRegistrationAllowed", "Subnet registration is paused"),
        ("NetworkPowRegistrationAllowed", "PoW registration is paused"),
    ],
)
async def test_pow_identifies_the_paused_registration_flag(disabled_flag, message):
    fake = FakeSubstrate()
    for name in ("NullConsensus", "NetworkRegistrationAllowed", "NetworkPowRegistrationAllowed"):
        fake.seed("SubtensorModule", name, [1], name != disabled_flag)
    with pytest.raises(BittensorError, match=message):
        await PowRegister(netuid=1).build(fake, dev_wallet())


@pytest.mark.asyncio
async def test_pow_timeout_is_bounded():
    fake = FakeSubstrate()
    for name in ("NullConsensus", "NetworkRegistrationAllowed", "NetworkPowRegistrationAllowed"):
        fake.seed("SubtensorModule", name, [1], True)
    fake.seed("SubtensorModule", "Difficulty", [1], 1)
    with (
        patch("bittensor.intents.null_consensus.monotonic", side_effect=[0, 2]),
        pytest.raises(BittensorError, match="timed out"),
    ):
        await PowRegister(netuid=1, timeout_seconds=1).build(fake, dev_wallet())


@pytest.mark.asyncio
async def test_u32_scores_are_never_requantized():
    fake = FakeSubstrate()
    _, _, params = await SetWeightsV2(netuid=1, uids=[1, 2], weights=[0xFFFFFFFF, 1]).build(
        fake, dev_wallet()
    )
    assert params["weights"] == [0xFFFFFFFF, 1]
    assert params["dests"] == [1, 2]
    call = await EnableNullConsensus(netuid=1).build(fake, dev_wallet())
    _, name, _ = call
    assert name == "enable_null_consensus"


@pytest.mark.parametrize(
    "uids, weights",
    [
        ([1, 1], [1, 1]),
        ([1], [-1]),
        ([1], [1 << 32]),
        ([32768], [1]),
        ([1], [1.5]),
        ([1], [0]),
        ([1], []),
    ],
)
def test_invalid_u32_scores_fail_locally(uids, weights):
    with pytest.raises(BittensorError):
        SetWeightsV2(netuid=1, uids=uids, weights=weights)
