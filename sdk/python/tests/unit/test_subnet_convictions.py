"""``subnet_convictions`` reads the lock aggregate layout of the queried block's runtime."""

from bittensor import Client
from tests.harness.fake_substrate import FakeSubstrate

_NETUID = 1
_OWNER = "owner-hotkey"


def _lock(locked_mass: int, block: int) -> dict:
    return {"locked_mass": locked_mass, "conviction": {"bits": 0}, "last_update": block}


async def _owner_entry(chain: FakeSubstrate) -> dict:
    chain.seed("SubtensorModule", "SubnetOwnerHotkey", [_NETUID], _OWNER)
    result = await Client("local", substrate=chain).read("subnet_convictions", netuid=_NETUID)
    (entry,) = result["hotkeys"]
    assert entry["hotkey"] == _OWNER and entry["is_owner"]
    return entry


async def test_owner_hotkey_aggregate_has_owner_conviction() -> None:
    chain = FakeSubstrate()
    chain.seed_map("SubtensorModule", "HotkeyLock", [(_OWNER, _lock(1_000, chain.block))])

    entry = await _owner_entry(chain)

    assert entry["locked_alpha"].rao == 1_000
    assert entry["conviction_alpha"].rao == 1_000


async def test_runtime_with_owner_lock_storage_adds_the_owner_aggregates() -> None:
    chain = FakeSubstrate()
    chain.seed("SubtensorModule", "OwnerLock", [_NETUID], _lock(1_000, chain.block))
    chain.seed("SubtensorModule", "DecayingOwnerLock", [_NETUID], _lock(200, chain.block))
    # Under that layout a hotkey-keyed aggregate matures like any other hotkey's.
    chain.seed_map("SubtensorModule", "HotkeyLock", [(_OWNER, _lock(500, chain.block))])

    entry = await _owner_entry(chain)

    assert entry["locked_alpha"].rao == 1_700
    assert entry["conviction_alpha"].rao == 1_200


async def test_runtime_with_only_the_perpetual_owner_lock_storage_reads_it() -> None:
    chain = FakeSubstrate()
    chain.seed("SubtensorModule", "OwnerLock", [_NETUID], _lock(1_000, chain.block))
    chain.seed_map("SubtensorModule", "HotkeyLock", [(_OWNER, _lock(500, chain.block))])

    entry = await _owner_entry(chain)

    assert entry["locked_alpha"].rao == 1_500
    assert entry["conviction_alpha"].rao == 1_000
