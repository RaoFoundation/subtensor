"""Required-collateral search must stay inside unlocked long stake."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.reads import lending as lending_reads
from bittensor.result import BittensorError, ChainError


class _StakeAwareView:
    """StakeUnavailable above available; InsufficientCollateral below needed."""

    def __init__(self, *, available: int, needed: int):
        self.available = available
        self.needed = needed
        self.block = 100
        self.probes: list[int] = []

    async def at(self):
        return self

    async def runtime(self, method, params):
        name = getattr(method, "method", None) or str(method)
        if "get_stake_info_for_hotkey_coldkey_netuid" in name:
            return {"stake": self.available}
        if "get_stake_availability_for_coldkeys" in name:
            return {
                "5Owner": {1: {"available": self.available, "total": self.available, "locked": 0}}
            }
        raise AssertionError(f"unexpected runtime call {name}")

    async def query(self, item, params):
        name = str(item)
        if "MinerCollateral" in name:
            return {"locked": 0, "min_locked": 0, "earned": 0, "drain_ratio": {"bits": 0}}
        if "Dissolution" in name:
            return None
        return None

    async def constant(self, item):
        return 50_400

    def balance(self, rao, netuid=0):
        return Balance.from_rao(rao, netuid)


def _stake_error(name: str) -> ChainError:
    return ChainError(f"{name} for test", name=name)


@pytest.mark.asyncio
async def test_required_collateral_initial_probe_stays_within_available(monkeypatch):
    """0.5α available, 0.1α needed: first probe must not abort on StakeUnavailable."""
    available = 500_000_000
    needed = 100_000_000
    view = _StakeAwareView(available=available, needed=needed)

    async def fake_open_quote(view, netuid, side, collateral, coldkey_ss58, hotkey_ss58, amount):
        rao = collateral.rao if hasattr(collateral, "rao") else int(collateral)
        view.probes.append(rao)
        if rao > available:
            raise _stake_error("StakeUnavailable")
        if rao < needed:
            raise _stake_error("InsufficientCollateral")
        return {
            "principal": Balance.from_rao(10**9, 0),
            "annual_interest": Balance.from_rao(0, netuid),
            "opening_value": Balance.from_rao(10**8),
        }

    async def no_position(*_a, **_k):
        return None

    monkeypatch.setattr(lending_reads, "lending_open_quote", fake_open_quote)
    monkeypatch.setattr(lending_reads, "lending_position", no_position)

    result = await lending_reads.lending_required_collateral(
        view, 1, "long", "1", "5Owner", "5Hotkey"
    )
    assert result["collateral"].rao == needed
    assert all(p <= available for p in view.probes), view.probes
    assert max(view.probes) <= available


@pytest.mark.asyncio
async def test_required_collateral_doubling_does_not_overshoot_available(monkeypatch):
    """Needed sits between 1α and available; doubling must land on available, not past it."""
    available = 1_500_000_000
    needed = 1_200_000_000
    view = _StakeAwareView(available=available, needed=needed)

    async def fake_open_quote(view, netuid, side, collateral, coldkey_ss58, hotkey_ss58, amount):
        rao = collateral.rao if hasattr(collateral, "rao") else int(collateral)
        view.probes.append(rao)
        if rao > available:
            raise _stake_error("StakeUnavailable")
        if rao < needed:
            raise _stake_error("InsufficientCollateral")
        return {
            "principal": Balance.from_rao(10**9, 0),
            "annual_interest": Balance.from_rao(0, netuid),
            "opening_value": Balance.from_rao(10**9),
        }

    async def no_position(*_a, **_k):
        return None

    monkeypatch.setattr(lending_reads, "lending_open_quote", fake_open_quote)
    monkeypatch.setattr(lending_reads, "lending_position", no_position)

    result = await lending_reads.lending_required_collateral(
        view, 1, "long", "1", "5Owner", "5Hotkey"
    )
    assert result["collateral"].rao == needed
    assert 1_000_000_000 in view.probes  # geometric start
    assert available in view.probes  # doubled/capped to available
    assert all(p <= available for p in view.probes), view.probes


@pytest.mark.asyncio
async def test_required_collateral_reports_when_needed_exceeds_available(monkeypatch):
    available = 500_000_000
    needed = 800_000_000
    view = _StakeAwareView(available=available, needed=needed)

    async def fake_open_quote(view, netuid, side, collateral, coldkey_ss58, hotkey_ss58, amount):
        rao = collateral.rao if hasattr(collateral, "rao") else int(collateral)
        if rao > available:
            raise _stake_error("StakeUnavailable")
        raise _stake_error("InsufficientCollateral")

    async def no_position(*_a, **_k):
        return None

    monkeypatch.setattr(lending_reads, "lending_open_quote", fake_open_quote)
    monkeypatch.setattr(lending_reads, "lending_position", no_position)

    with pytest.raises(BittensorError, match="unlocked alpha"):
        await lending_reads.lending_required_collateral(view, 1, "long", "1", "5Owner", "5Hotkey")
