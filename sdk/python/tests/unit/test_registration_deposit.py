"""Registration prices follow mutable storage and fail closed on read errors."""

from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor._substrate import RpcSubstrate
from bittensor._transport.errors import StorageFunctionNotFound
from bittensor.error_descriptions import DESCRIPTIONS
from bittensor.error_map import NAME_TO_CODE, ErrorCode
from bittensor.hashed import hashed_registration_deposit, quote_hashed_registration_deposit
from tests.harness.fake_substrate import FakeSubstrate

HASH = "0x" + "12" * 32
LEGACY_CONSTANTS = (
    "RegistrationDeposit",
    "MlDsaRegistrationDeposit",
    "DerivedRegistrationDeposit",
)


@pytest.mark.parametrize("legacy_constant", LEGACY_CONSTANTS)
async def test_live_deposit_overrides_constants_and_is_not_cached(legacy_constant):
    chain = FakeSubstrate()
    chain.seed_constant("HashedAccounts", legacy_constant, 1)
    chain.seed_default("HashedAccounts", "HashedAccountRegistrationDeposit", 200_000_000)
    assert await hashed_registration_deposit(chain, legacy_constant) == 200_000_000
    for amount in (350_000_000, 0):
        chain.seed("HashedAccounts", "HashedAccountRegistrationDeposit", [], amount)
        assert await hashed_registration_deposit(chain, legacy_constant) == amount


@pytest.mark.parametrize("legacy_constant", LEGACY_CONSTANTS)
async def test_legacy_metadata_uses_corresponding_constant_at_same_block(legacy_constant):
    chain = SimpleNamespace(
        query=AsyncMock(side_effect=StorageFunctionNotFound("absent in metadata")),
        constant=AsyncMock(return_value=123),
        block_hash=AsyncMock(return_value=HASH),
    )
    assert await hashed_registration_deposit(chain, legacy_constant) == 123
    chain.query.assert_awaited_once_with(
        "HashedAccounts", "HashedAccountRegistrationDeposit", block_hash=HASH
    )
    chain.constant.assert_awaited_once_with("HashedAccounts", legacy_constant, block_hash=HASH)


async def test_explicit_block_is_preserved_without_resolving_a_new_head():
    chain = SimpleNamespace(query=AsyncMock(return_value=0), block_hash=AsyncMock())
    assert await hashed_registration_deposit(chain, "RegistrationDeposit", block_hash=HASH) == 0
    chain.block_hash.assert_not_awaited()
    chain.query.assert_awaited_once_with(
        "HashedAccounts", "HashedAccountRegistrationDeposit", block_hash=HASH
    )


@pytest.mark.parametrize("configurable", [False, True])
async def test_quote_reports_guard_support_from_the_same_single_storage_read(configurable):
    chain = SimpleNamespace(
        query=AsyncMock(
            return_value=0,
            side_effect=None if configurable else StorageFunctionNotFound("legacy metadata"),
        ),
        constant=AsyncMock(return_value=123),
    )
    quote = await quote_hashed_registration_deposit(chain, "RegistrationDeposit", block_hash=HASH)
    assert quote.amount_rao == (0 if configurable else 123)
    assert quote.configurable is configurable
    chain.query.assert_awaited_once()
    assert chain.constant.await_count == int(not configurable)


def test_registration_price_limit_has_actionable_error_mapping():
    assert NAME_TO_CODE["RegistrationDepositTooHigh"] is ErrorCode.LIMIT_EXCEEDED
    description = DESCRIPTIONS["RegistrationDepositTooHigh"]
    assert "HashedAccounts.HashedAccountRegistrationDeposit" in description


@pytest.mark.parametrize("error", [ConnectionError("offline"), ValueError("bad SCALE")])
async def test_storage_read_errors_never_fall_back_to_metadata_constants(error):
    chain = SimpleNamespace(query=AsyncMock(side_effect=error), constant=AsyncMock())
    with pytest.raises(type(error), match=str(error)):
        await hashed_registration_deposit(chain, "RegistrationDeposit", block_hash=HASH)
    chain.constant.assert_not_awaited()


async def test_invalid_live_value_never_falls_back_to_metadata_constants():
    chain = SimpleNamespace(query=AsyncMock(return_value=None), constant=AsyncMock())
    with pytest.raises(ValueError, match="could not quote"):
        await hashed_registration_deposit(chain, "RegistrationDeposit", block_hash=HASH)
    chain.constant.assert_not_awaited()


async def test_constant_rpc_preserves_pinned_block():
    chain = RpcSubstrate("ws://unused:1")
    chain._substrate = SimpleNamespace(get_constant=AsyncMock(return_value=123))
    assert await chain.constant("HashedAccounts", "RegistrationDeposit", block_hash=HASH) == 123
    chain._substrate.get_constant.assert_awaited_once_with(
        "HashedAccounts", "RegistrationDeposit", HASH
    )
