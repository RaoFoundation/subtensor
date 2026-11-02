"""CLI raise default --valid-until must use Client.block, not client.substrate.

open/close leave the intent default (u32::MAX) and never hit this path.
"""

from __future__ import annotations

import pytest

from bittensor.cli.commands.lending import (
    DEFAULT_RAISE_VALIDITY_BLOCKS,
    _resolve_valid_until,
)
from bittensor.intents.lending import CloseLoan, OpenLoan, RaiseLoan, RepayLoan


class _FakeClient:
    def __init__(self, head: int):
        self._head = head
        self.block_calls = 0

    async def block(self) -> int:
        self.block_calls += 1
        return self._head

    @property
    def substrate(self):
        raise AssertionError("Client has no public .substrate; use Client.block()")


@pytest.mark.asyncio
async def test_resolve_valid_until_uses_explicit_deadline_without_reading_head():
    client = _FakeClient(1_000)
    assert await _resolve_valid_until(client, 1_042) == 1_042
    assert client.block_calls == 0


@pytest.mark.asyncio
async def test_resolve_valid_until_defaults_to_head_plus_window():
    client = _FakeClient(9_000)
    assert await _resolve_valid_until(client, None) == 9_000 + DEFAULT_RAISE_VALIDITY_BLOCKS
    assert client.block_calls == 1


@pytest.mark.asyncio
async def test_resolve_valid_until_does_not_touch_missing_substrate_attr():
    client = _FakeClient(50)
    # Regression for AttributeError: 'Client' object has no attribute 'substrate'.
    assert await _resolve_valid_until(client, None) == 50 + DEFAULT_RAISE_VALIDITY_BLOCKS


def test_raise_sell_alpha_help_describes_runtime_default_sale():
    help_text = RaiseLoan.field_help("sell_alpha")
    assert "omit to sell none" not in help_text.lower()
    assert "escrowed alpha" in help_text.lower()


def test_open_and_close_default_valid_until_is_far_future():
    # Confirm open/close/repay never needed the broken head-relative Client.substrate path.
    assert OpenLoan.__dataclass_fields__["valid_until"].default == 2**32 - 1
    assert CloseLoan.__dataclass_fields__["valid_until"].default == 2**32 - 1
    assert RepayLoan.__dataclass_fields__["valid_until"].default == 2**32 - 1


def test_raise_loan_default_valid_until_is_none():
    # Raise resolves head + 50 in RaiseLoan.build (or via CLI _resolve_valid_until).
    assert RaiseLoan.__dataclass_fields__["valid_until"].default is None
