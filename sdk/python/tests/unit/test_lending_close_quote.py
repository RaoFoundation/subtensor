"""lending_close_quote must pass optional hotkey into quote_close."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.intents import CloseLoan
from bittensor.reads import lending as lending_reads
from bittensor.result import BittensorError


class _RecordingView:
    def __init__(self):
        self.runtime_calls: list[tuple] = []
        self.block = 100

    async def at(self):
        return self

    async def runtime(self, method, params):
        self.runtime_calls.append((method, list(params)))
        return {
            "Ok": {
                "payment": 10**9,
                "refund": 10**9,
                "escrow_return": 0,
            }
        }

    async def query(self, item, params):
        name = getattr(item, "name", None) or getattr(item, "storage", None) or str(item)
        if "Dissolution" in str(item) or "Dissolutions" in str(name):
            return None
        return {
            "side": "Short",
            "principal": 10**9,
            "collateral": 10**10,
            "proceeds": 10**9,
            "escrow_alpha": 0,
            "lift_tao": 10**9,
            "lift_alpha": 0,
            "annual_interest": 0,
            "interest_remainder": 0,
            "last_accrued": self.block,
            "due": self.block + 100,
            "expires_at": self.block + 1000,
            "expiry_due": self.block + 1000,
        }

    async def constant(self, item):
        return 50_400

    def balance(self, rao, netuid=0):
        return Balance.from_rao(rao, netuid)


@pytest.mark.asyncio
async def test_lending_close_quote_passes_hotkey_as_optional_param():
    view = _RecordingView()
    owner = "5Owner"
    hotkey = "5Hotkey"
    result = await lending_reads.lending_close_quote(
        view, owner, 1, hotkey, repay_from_wallet=False
    )
    assert result["payment"].rao == 10**9
    assert len(view.runtime_calls) == 1
    method, params = view.runtime_calls[0]
    assert method == lending_reads.QUOTE_CLOSE
    assert params == [owner, 1, hotkey, False]


@pytest.mark.asyncio
async def test_lending_close_quote_allows_omitted_hotkey():
    view = _RecordingView()
    owner = "5Owner"
    result = await lending_reads.lending_close_quote(
        view, owner, 1, repay_from_wallet=False
    )
    assert result["payment"].rao == 10**9
    method, params = view.runtime_calls[0]
    assert method == lending_reads.QUOTE_CLOSE
    assert params == [owner, 1, None, False]


class _Wallet:
    coldkeypub = type("K", (), {"ss58_address": "5Owner"})()
    hotkey = type("K", (), {"ss58_address": "5WalletHot"})()


class _CloseSubstrate:
    def __init__(self, side: str):
        self.side = side
        self.composed = None

    async def query(self, *args):
        return {
            "side": self.side,
            "principal": 10**9,
            "collateral": 10**10,
            "proceeds": 10**9,
            "escrow_alpha": 10**9 if self.side == "Long" else 0,
        }

    async def compose(self, call):
        self.composed = call
        return call


@pytest.mark.asyncio
async def test_close_loan_requires_explicit_hotkey_when_alpha_moves():
    substrate = _CloseSubstrate("Long")
    intent = CloseLoan(netuid=1, max_payment="1", min_refund="0", repay_from_wallet=False)
    with pytest.raises(BittensorError, match="hotkey_ss58"):
        await intent.build(substrate, _Wallet())


@pytest.mark.asyncio
async def test_close_loan_short_escrow_allows_omitted_hotkey():
    substrate = _CloseSubstrate("Short")
    intent = CloseLoan(netuid=1, max_payment="1", min_refund="0", repay_from_wallet=False)
    await intent.build(substrate, _Wallet())
    assert substrate.composed.params["hotkey"] == "5Owner"
