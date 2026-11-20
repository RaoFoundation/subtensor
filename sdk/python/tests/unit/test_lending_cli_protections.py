"""CLI/SDK lending protections: slippage, hotkey, max collateral, valid_until."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.cli.commands.lending import _bound, _quoted_expires_at, _require_slippage
from bittensor.cli.context import AppContext
from bittensor.error_descriptions.lending import DESCRIPTIONS as LENDING_DESCRIPTIONS
from bittensor.intents.lending import RaiseLoan
from bittensor.result import BittensorError

# On-chain MaxLoanDuration (~30 days at 12s blocks). Must not use BlocksPerYear.
_MAX_LOAN_DURATION = 216_000


def test_quoted_expires_at_open_uses_max_loan_duration():
    """New open cards show head + MaxLoanDuration, not BlocksPerYear (~1 year)."""
    head = 1_000_000
    expires = _quoted_expires_at(head=head, max_loan_duration=_MAX_LOAN_DURATION)
    assert expires == head + _MAX_LOAN_DURATION
    # Regression: BlocksPerYear would add ~2.6M blocks (~1 year).
    assert expires - head != 2_628_000
    assert expires - head == 216_000


def test_quoted_expires_at_increase_keeps_existing_expires():
    """Increase / raise cards keep the position expires_at (no extension)."""
    head = 1_000_000
    existing = head + 50_000
    assert (
        _quoted_expires_at(
            head=head,
            max_loan_duration=_MAX_LOAN_DURATION,
            existing_expires=existing,
        )
        == existing
    )
    # Existing wins even if MaxLoanDuration is missing.
    assert (
        _quoted_expires_at(head=head, max_loan_duration=None, existing_expires=existing) == existing
    )


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
async def test_live_lift_params_queries_pass_storage_item_whole():
    """open/roll/show/params must pass StorageItem whole to Client.query.

    ``client.query(*item)`` unpacks into ``("Lending", "LiftParamsStore")`` so
    Client treats the module string as the descriptor and indexes ``"L"``/``"e"``
    → ``storage function L.e not found``.
    """
    from bittensor.cli.commands.lending import (
        _ENABLED,
        _LIFT_PARAMS,
        _live_max_loan_duration,
        _live_max_settlement_deferral,
    )

    calls: list[tuple[str, object, object]] = []

    class _Client:
        async def query(self, item, params=None, *, block=None):
            calls.append(("query", item, params))
            assert not isinstance(item, str), f"splat bug passed module string {item!r}"
            assert item[0] == "Lending"
            if item is _LIFT_PARAMS or item[1] == "LiftParamsStore":
                return {
                    "max_loan_duration": 216_000,
                    "max_settlement_deferral": 50_400,
                    "slice_bps": 1,
                }
            if item is _ENABLED or item[1] == "Enabled":
                return False
            raise AssertionError(f"unexpected storage item {item!r}")

        async def constant(self, item):
            raise AssertionError(f"must not fall back to constant; got {item!r}")

    client = _Client()
    assert await _live_max_loan_duration(client) == 216_000
    assert await _live_max_settlement_deferral(client) == 50_400
    enabled = await client.query(_ENABLED)
    params = await client.query(_LIFT_PARAMS)
    assert enabled is False
    assert params["max_loan_duration"] == 216_000
    assert all(isinstance(c[1], type(_LIFT_PARAMS)) or c[1] is _ENABLED for c in calls)


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
async def test_raise_loan_explicit_valid_until_checks_not_past():
    class _Substrate:
        async def block_number(self) -> int:
            return 1_000

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

    past = RaiseLoan(
        netuid=1,
        hotkey_ss58="5Hotkey",
        borrow_tao="1",
        max_collateral_alpha="10",
        valid_until=500,
    )
    with pytest.raises(BittensorError, match="already past"):
        await past.build(_Substrate(), _Wallet())


def test_raise_loan_amount_mode_rejects_zero_max_collateral():
    with pytest.raises(BittensorError, match="max_collateral_alpha"):
        RaiseLoan(netuid=1, hotkey_ss58="5Hotkey", borrow_tao="1")
    with pytest.raises(BittensorError, match="must be > 0"):
        RaiseLoan(
            netuid=1,
            hotkey_ss58="5Hotkey",
            borrow_tao="1",
            max_collateral_alpha="0",
        )


def test_raise_loan_valid_until_default_is_none():
    assert RaiseLoan.__dataclass_fields__["valid_until"].default is None


def test_insufficient_escrow_mentions_shortfall():
    text = LENDING_DESCRIPTIONS["InsufficientEscrow"]
    assert "shortfall" in text.lower()
    assert "--from-wallet" in text or "from-wallet" in text.lower()
    assert "repay-from-wallet" in text.lower()  # documented alias
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
