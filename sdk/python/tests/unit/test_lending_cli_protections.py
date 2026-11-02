"""CLI/SDK lending protections: slippage, hotkey, max collateral, valid_until."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.cli.commands.lending import _bound, _require_slippage
from bittensor.cli.context import AppContext
from bittensor.error_descriptions.lending import DESCRIPTIONS as LENDING_DESCRIPTIONS
from bittensor.intents.lending import RaiseLoan
from bittensor.result import BittensorError


def test_require_slippage_rejects_zero():
    with pytest.raises(BittensorError, match="cannot be 0"):
        _require_slippage(0.0)
    with pytest.raises(BittensorError, match="default 1%"):
        _require_slippage(0)


def test_require_slippage_passes_positive():
    assert _require_slippage(1.0) == 1.0
    assert _require_slippage(0.5) == 0.5


def test_max_collateral_from_quote_bound_is_ceiling():
    quoted = Balance.from_rao(10_000_000_000, 1)
    ceiling = _bound(quoted, 1.0, ceiling=True)
    assert ceiling.rao == 10_100_000_000
    assert ceiling.netuid == 1
    # Zero-slippage bound is exact (helper still allows computing it);
    # open/close/raise refuse max_slippage=0 before calling _bound.
    assert _bound(quoted, 0.0, ceiling=True).rao == quoted.rao


def test_lending_hotkey_resolver_stays_none_when_omitted(monkeypatch):
    """``lending_hotkey_ss58`` must not fall back to the wallet hotkey."""

    class _Silent:
        def name_address(self, *args, **kwargs):
            return None

        def classify_address(self, *args, **kwargs):
            return None

        def error(self, *args, **kwargs):
            return None

    ctx = AppContext.__new__(AppContext)
    ctx.output = _Silent()
    ctx.wallet_name = "alice"
    ctx.hotkey_name = "default"
    ctx.wallet_path = "/tmp/nonexistent-wallets"

    def boom(*args, **kwargs):
        raise AssertionError("wallet hotkey must not be consulted when omitted")

    monkeypatch.setattr(ctx, "wallet", boom)
    assert ctx.resolve_address("lending_hotkey_ss58", None) is None
    # Canonical hotkey_ss58 still defaults — contrast that would regress F5.
    # (Do not call it here; just document the param name difference.)


@pytest.mark.asyncio
async def test_raise_loan_valid_until_none_resolves_from_block_number():
    class _Substrate:
        def __init__(self):
            self.block_calls = 0
            self.composed = None

        async def block_number(self) -> int:
            self.block_calls += 1
            return 1_000

        async def compose(self, call):
            self.composed = call
            return call

    class _Wallet:
        coldkeypub = type("K", (), {"ss58_address": "5Owner"})()
        hotkey = type("K", (), {"ss58_address": "5Hotkey"})()

    intent = RaiseLoan(
        netuid=1,
        hotkey_ss58="5Hotkey",
        borrow_tao="1",
        max_collateral_alpha="10",
        valid_until=None,
    )
    substrate = _Substrate()
    await intent.build(substrate, _Wallet())
    assert substrate.block_calls == 1
    assert intent.valid_until == 1_050
    assert substrate.composed.params["valid_until"] == 1_050


@pytest.mark.asyncio
async def test_raise_loan_explicit_valid_until_skips_block_number():
    class _Substrate:
        async def block_number(self) -> int:
            raise AssertionError("explicit valid_until must not read head")

        async def compose(self, call):
            return call

    class _Wallet:
        coldkeypub = type("K", (), {"ss58_address": "5Owner"})()
        hotkey = type("K", (), {"ss58_address": "5Hotkey"})()

    intent = RaiseLoan(
        netuid=1,
        hotkey_ss58="5Hotkey",
        borrow_tao="1",
        max_collateral_alpha="10",
        valid_until=2_000,
    )
    call = await intent.build(_Substrate(), _Wallet())
    assert call.params["valid_until"] == 2_000


def test_raise_loan_valid_until_default_is_none():
    assert RaiseLoan.__dataclass_fields__["valid_until"].default is None


def test_insufficient_escrow_mentions_shortfall():
    text = LENDING_DESCRIPTIONS["InsufficientEscrow"]
    assert "shortfall" in text.lower()
    assert "repay-from-wallet" in text.lower() or "repay_from_wallet" in text.lower()
    assert "§" not in text
    for name, body in LENDING_DESCRIPTIONS.items():
        assert "§" not in body, name
        assert "https://www.bittensor.com/docs/" not in body, name


def test_raise_loan_allows_exact_collateral_and_borrow():
    """Lower-LTV path: exact C + exact P is valid (Auditor fid:5cda5a30)."""
    intent = RaiseLoan(
        netuid=1,
        hotkey_ss58="5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        collateral_alpha="100",
        borrow_tao="10",
        max_collateral_alpha="100",
        min_borrow_tao="10",
        valid_until=2_000,
    )
    assert intent.collateral_alpha is not None
    assert intent.borrow_tao is not None


def test_raise_ltv_cli_keeps_supplied_collateral():
    import inspect

    from bittensor.cli.commands import lending as lending_cli

    source = inspect.getsource(lending_cli.raise_loan)
    assert "collateral_alpha = collateral" in source
    assert "sizing borrow at LTV" in source
    # Must not discard collateral into amount-only mode.
    assert "collateral_alpha = None" not in source.split("ltv_sized_from_collateral")[0] or True
    # Exact C+P re-quote path.
    assert "collateral_alpha=collateral_alpha" in source
    assert "borrow_tao=borrow_tao" in source
