"""F8/F11/F15 lending list defaults, runway cap, show/quote, input validation."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.cli.commands import lending as lending_cli
from bittensor.cli.commands.lending import (
    VALID_UNTIL_NONE,
    _explain_band_deferral,
    _format_expiry_field,
    _format_runway_field,
    _format_valid_until,
    _parse_positive_amount,
    _position_json,
    _require_slippage,
    _terms_version_label,
)
from bittensor.error_descriptions.lending import DESCRIPTIONS as LENDING_DESCRIPTIONS
from bittensor.reads import lending as lending_reads
from bittensor.result import BittensorError


def test_parse_positive_amount_rejects_non_numeric():
    with pytest.raises(BittensorError, match="positive number"):
        _parse_positive_amount("abc", tao=True, label="amount")
    with pytest.raises(BittensorError, match="greater than zero"):
        _parse_positive_amount("0", tao=True, label="amount")
    with pytest.raises(BittensorError, match=r"positive number|greater than zero|non-negative"):
        _parse_positive_amount("-1", tao=True, label="collateral")


def test_parse_positive_amount_accepts_decimal_string():
    bal = _parse_positive_amount("1.5", tao=True, label="amount")
    assert bal.rao == 1_500_000_000
    assert bal.netuid == 0
    alpha = _parse_positive_amount("2", tao=False, netuid=7, label="collateral")
    assert alpha.rao == 2_000_000_000
    assert alpha.netuid == 7


def test_format_valid_until_none_sentinel():
    # Match Transaction card wording (N15): "no deadline", not "none".
    assert _format_valid_until(VALID_UNTIL_NONE) == "no deadline"
    assert _format_valid_until(2**32 - 1) == "no deadline"
    assert "block 100" in _format_valid_until(100, head=50)


def test_position_json_uses_rao_ints():
    record = {
        "coldkey": "5Owner",
        "side": "Short",
        "principal": Balance.from_rao(10, 1),
        "collateral": Balance.from_rao(20, 0),
        "runway_days": 12.3456789,
        "underwater": False,
        "expires_at": 99,
    }
    out = _position_json(record)
    assert out["principal"] == 10
    assert out["collateral"] == 20
    assert out["runway_days"] == pytest.approx(12.345679)
    assert out["coldkey"] == "5Owner"


def test_runway_capped_at_expires_at():
    """Interest runway cannot exceed days remaining until expires_at."""

    class _View:
        block = 1_000

        def balance(self, rao, netuid=0):
            return Balance.from_rao(rao, netuid)

    # Huge collateral vs tiny coupon → uncapped runway would be enormous.
    # With BlocksPerYear=365, one remaining block ≈ 1 day.
    raw = {
        "side": "Short",
        "principal": 1_000,
        "collateral": 10_000_000_000,  # 10 τ
        "proceeds": 500,
        "escrow_alpha": 0,
        "lift_tao": 0,
        "lift_alpha": 0,
        "annual_interest": 1_000_000,  # 0.001 τ/yr → huge runway
        "last_accrued": 1_000,
        "interest_remainder": 0,
        "due": 2_000,
        "expires_at": 1_000 + 1,
        "terms_version": 1,
    }
    record = lending_reads._position_record(_View(), "5Owner", 1, raw, (1_000, 365))
    assert record is not None
    assert record["expires_at"] == 1_001
    assert record["runway_days"] == pytest.approx(1.0)
    uncapped = 10_000_000_000 * 365 / 1_000_000
    assert uncapped > 1_000
    assert record["runway_days"] < uncapped
    assert record["underwater"] is False
    assert record["escrow"].rao == 500
    assert "opened_at" not in record


def test_underwater_when_interest_exhausts_collateral():
    class _View:
        block = 2_000

        def balance(self, rao, netuid=0):
            return Balance.from_rao(rao, netuid)

    raw = {
        "side": "Long",
        "principal": 1,
        "collateral": 100,
        "proceeds": 0,
        "escrow_alpha": 50,
        "lift_tao": 0,
        "lift_alpha": 0,
        "annual_interest": 100,
        "last_accrued": 1_000,
        "interest_remainder": 0,
        "due": 3_000,
        "expires_at": 10_000,
        "terms_version": 1,
    }
    # accrued_blocks=1000 → interest_due = 100*1000/100 = 1000 > collateral 100
    record = lending_reads._position_record(_View(), "5Owner", 2, raw, (2_000, 100))
    assert record is not None
    assert record["underwater"] is True
    assert record["escrow"].rao == 50
    assert record["escrow"].netuid == 2


def test_lending_cli_registers_show_and_quote():
    names = {cmd.name for cmd in lending_cli.app.registered_commands}
    assert "list" in names
    assert "show" in names
    assert "quote" in names
    assert "open" in names
    assert "repay" in names


def test_list_defaults_coldkey_unless_all():
    """list_positions resolves wallet coldkey by default; --all lists subnet-wide."""
    import inspect

    source = inspect.getsource(lending_cli.list_positions)
    assert 'resolve_address("coldkey_ss58"' in source or "resolve_address('coldkey_ss58'" in source
    assert "list_all" in source
    assert "lending_positions" in source  # --all path
    doc = lending_cli.list_positions.__doc__ or ""
    assert "wallet" in doc.lower() or "coldkey" in doc.lower()


def test_slippage_still_rejects_zero():
    with pytest.raises(BittensorError, match="cannot be 0"):
        _require_slippage(0)


def test_error_descriptions_slippage_and_capacity_guidance():
    slip = LENDING_DESCRIPTIONS["SlippageExceeded"]
    assert "stale" in slip.lower() or "re-quote" in slip.lower() or "requote" in slip.lower()
    assert "max-slippage" in slip.lower()
    side = LENDING_DESCRIPTIONS["SideCapacity"]
    assert "headroom" in side.lower()
    assert "reserve depth" in side.lower() or "not amm" in side.lower()
    too_new = LENDING_DESCRIPTIONS["SubnetTooNew"]
    assert "LiftParams" in too_new
    assert "7 days" in too_new or "50_400" in too_new
    not_started = LENDING_DESCRIPTIONS["SubnetNotStarted"]
    assert "owner" in not_started.lower() and "start" in not_started.lower()
    escrow = LENDING_DESCRIPTIONS["InsufficientEscrow"]
    assert "shortfall" in escrow.lower()


def test_open_help_matches_runtime_minimum_loan_value():
    import inspect

    assert lending_cli.MINIMUM_LOAN_VALUE_TAO == "0.001"
    # Decorators may clear __doc__; the source (and error text) must match runtime.
    source = inspect.getsource(lending_cli.open_loan)
    assert "MINIMUM_LOAN_VALUE_TAO" in source or "0.001" in source
    assert "0.1 TAO" not in source
    assert "``CloseLoan``" not in inspect.getsource(lending_cli.close_loan)
    # Human AmountTooSmall copy uses τ (F12); still names the ~0.001 minimum.
    assert "~0.001 τ" in LENDING_DESCRIPTIONS["AmountTooSmall"]


def test_proceeds_docstring_no_longer_claims_zero_for_new_shorts():
    doc = lending_reads.lending_position.__doc__ or ""
    assert "zero for newly opened shorts" not in doc
    assert "escrowed TAO" in doc or "E_S" in doc


def test_interest_due_stops_at_expires_at():
    """Display accrual matches runtime: no interest past expires_at."""

    class _View:
        block = 5_000

        def balance(self, rao, netuid=0):
            return Balance.from_rao(rao, netuid)

    raw = {
        "side": "Short",
        "principal": 1_000,
        "collateral": 10_000_000,
        "proceeds": 0,
        "escrow_alpha": 0,
        "lift_tao": 0,
        "lift_alpha": 0,
        "annual_interest": 365_000,
        "last_accrued": 1_000,
        "interest_remainder": 0,
        "due": 2_000,
        "expires_at": 1_100,
        "terms_version": 1,
        "blocks_per_year": 365,
    }
    # now=5000, but accrual window is only expires_at-last_accrued=100.
    record = lending_reads._position_record(_View(), "5Owner", 1, raw, (5_000, 365))
    assert record is not None
    assert record["interest_due"].rao == 365_000 * 100 // 365
    raw_fully_accrued = {**raw, "last_accrued": 1_100}
    zero = lending_reads._position_record(_View(), "5Owner", 1, raw_fully_accrued, (5_000, 365))
    assert zero is not None
    assert zero["interest_due"].rao == 0


def test_terms_version_read_strictly():
    """N26: missing / null terms_version must not become 1."""

    class _View:
        block = 1

        def balance(self, rao, netuid=0):
            return Balance.from_rao(rao, netuid)

    base = {
        "side": "Short",
        "principal": 1,
        "collateral": 2,
        "proceeds": 0,
        "escrow_alpha": 0,
        "lift_tao": 0,
        "lift_alpha": 0,
        "annual_interest": 0,
        "last_accrued": 0,
        "interest_remainder": 0,
        "due": 10,
        "expires_at": 20,
    }
    with pytest.raises(BittensorError, match="terms_version"):
        lending_reads._position_record(_View(), "5Owner", 1, dict(base), (1, 365))
    zero = lending_reads._position_record(
        _View(), "5Owner", 1, {**base, "terms_version": 0}, (1, 365)
    )
    assert zero is not None
    assert zero["terms_version"] == 0
    assert _terms_version_label(0) == "0"
    assert _terms_version_label(None) == "-"


def test_past_expiry_and_band_deferral_wording():
    """U2: past-expiry labels and SettlementPriceOutsideEmaBand explanation."""
    position = {"expires_at": 100, "runway_days": 0.0}
    assert "past expiry" in _format_expiry_field(position, head=200)
    assert "awaiting settlement" in _format_expiry_field(position, head=200)
    assert _format_runway_field(position, head=200) == "past expiry"
    assert "0d" not in _format_runway_field(position, head=200)
    explained = _explain_band_deferral(
        "Lending::SettlementPriceOutsideEmaBand",
        spot_vs_ema_pct=22.5,
        band_bps=1_500,
        retry_at=350,
        force_at=7_400,
    )
    assert "22.5%" in explained
    assert "±15%" in explained
    assert "retries at block 350" in explained
    assert "forced settle at block 7400" in explained
