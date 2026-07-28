"""Hyperparameter unit semantics: ratio normalization keyed on type identity,
the metadata-primary kind resolution, the `to_raw` fraction bound check, and
the codegen `--units` drift gate."""

from __future__ import annotations

from typing import NamedTuple

import pytest

from bittensor import hyperparams as hp
from bittensor.settings import U16_MAX
from codegen.check import check_units


class _Item(NamedTuple):
    """Stand-in for a post-regen storage descriptor (three-field Item)."""

    container: str
    name: str
    value_type_ident: str = ""


# --- ratio_fraction -----------------------------------------------------------


def test_ratio_fraction_known_idents():
    assert hp.ratio_fraction("PerU16", 65535) == 1.0
    assert hp.ratio_fraction("PerU16", 32767) == 32767 / U16_MAX
    assert hp.ratio_fraction("Percent", 50) == 0.5
    assert hp.ratio_fraction("Permill", 900_000) == 0.9
    assert hp.ratio_fraction("Perbill", 250_000_000) == 0.25
    assert hp.ratio_fraction("PerU16", 0) == 0.0


def test_ratio_fraction_non_ratio_idents_return_none():
    for ident in ("u16", "u64", "TaoBalance", "AlphaBalance", "Vec<u16>", "", None):
        assert hp.ratio_fraction(ident, 123) is None


# --- metadata-primary kind resolution ------------------------------------------


def test_committed_descriptors_agree_with_hand_table():
    # The committed _generated/storage.py is regenerated against the newtype
    # TypeInfo runtime: descriptors that carry a unit-bearing identity must
    # resolve to the same kind the hand table records, and the rest fall back.
    for name in hp.HYPERPARAMS:
        derived = hp.metadata_kind(name)
        assert derived in (None, hp.HYPERPARAMS[name].kind)
        assert hp.kind_of(name) == hp.HYPERPARAMS[name].kind


def test_metadata_identity_decides_kind(monkeypatch):
    monkeypatch.setitem(hp.STORAGE_ITEMS, "kappa", _Item("SubtensorModule", "Kappa", "PerU16"))
    monkeypatch.setitem(
        hp.STORAGE_ITEMS, "min_burn", _Item("SubtensorModule", "MinBurn", "TaoBalance")
    )
    assert hp.metadata_kind("kappa") == "u16"
    assert hp.kind_of("kappa") == "u16"
    assert hp.metadata_kind("min_burn") == "rao"
    # A bare primitive identity (pre-newtype regen) is not unit-bearing.
    monkeypatch.setitem(hp.STORAGE_ITEMS, "kappa", _Item("SubtensorModule", "Kappa", "u16"))
    assert hp.metadata_kind("kappa") is None
    assert hp.kind_of("kappa") == "u16"  # hand table


def test_metadata_identity_overrides_hand_table(monkeypatch):
    # If the metadata says a parameter is a PerU16 ratio, that wins even when
    # the hand table disagrees (the --units gate exists to flag the mismatch).
    monkeypatch.setitem(hp.STORAGE_ITEMS, "tempo", _Item("SubtensorModule", "Tempo", "PerU16"))
    assert hp.kind_of("tempo") == "u16"
    assert hp.normalized("tempo", 32767) == 32767 / U16_MAX


# --- the --units drift gate -----------------------------------------------------


def test_units_gate_passes_on_committed_metadata(capsys):
    # The committed descriptors must agree with the hand table regardless of
    # how many carry a derivable identity (that count grows as the runtime
    # newtypes more storage values).
    assert check_units() == 0
    assert "metadata-derived" in capsys.readouterr().out


def test_units_gate_flags_disagreement(monkeypatch, capsys):
    monkeypatch.setitem(hp.STORAGE_ITEMS, "tempo", _Item("SubtensorModule", "Tempo", "PerU16"))
    assert check_units() == 1
    out = capsys.readouterr().out
    assert "tempo" in out and "'blocks'" in out and "'u16'" in out


def test_units_gate_accepts_agreement(monkeypatch):
    monkeypatch.setitem(hp.STORAGE_ITEMS, "kappa", _Item("SubtensorModule", "Kappa", "PerU16"))
    monkeypatch.setitem(
        hp.STORAGE_ITEMS, "min_burn", _Item("SubtensorModule", "MinBurn", "TaoBalance")
    )
    monkeypatch.setitem(
        hp.STORAGE_ITEMS,
        "bonds_moving_avg",
        _Item("SubtensorModule", "BondsMovingAverage", "Permill"),
    )
    assert check_units() == 0


# --- to_raw ---------------------------------------------------------------------


def test_to_raw_integer_is_raw_within_bounds():
    assert hp.to_raw("kappa", 32767) == 32767
    assert hp.to_raw("kappa", U16_MAX) == U16_MAX
    assert hp.to_raw("bonds_moving_avg", 1_000_000) == 1_000_000
    assert hp.to_raw("adjustment_alpha", hp.U64_MAX) == hp.U64_MAX


def test_to_raw_rejects_raw_above_denominator():
    with pytest.raises(ValueError, match=r"must be within 0\.\.65535"):
        hp.to_raw("kappa", U16_MAX + 1)
    with pytest.raises(ValueError, match=r"0\.\.65535"):
        hp.to_raw("max_weights_limit", 10**6)
    with pytest.raises(ValueError, match=r"must be within 0\.\.1000000"):
        hp.to_raw("bonds_moving_avg", 1_000_001)
    with pytest.raises(ValueError, match="must be within"):
        hp.to_raw("adjustment_alpha", hp.U64_MAX + 1)


def test_to_raw_rejects_outside_codec_width():
    with pytest.raises(ValueError, match=r"must be within 0\.\.65535"):
        hp.to_raw("rho", 65_536)
    with pytest.raises(ValueError, match=r"must be within 360\.\.50400"):
        hp.to_raw("tempo", 10**12)
    with pytest.raises(ValueError, match=r"must be within 0\.\."):
        hp.to_raw("min_burn", 2**64)
    assert hp.to_raw("difficulty", hp.U64_MAX) == hp.U64_MAX
    assert hp.to_raw("tempo", 360) == 360
    assert hp.to_raw("min_burn", 10**18) == 10**18


def test_to_raw_bool_only_zero_one():
    assert hp.to_raw("registration_allowed", True) == 1
    assert hp.to_raw("registration_allowed", False) == 0
    assert hp.to_raw("registration_allowed", "true") == 1
    assert hp.to_raw("registration_allowed", "false") == 0
    assert hp.to_raw("registration_allowed", 0) == 0
    assert hp.to_raw("registration_allowed", 1) == 1
    for bad in (2, 999, "yes", "on", "2"):
        with pytest.raises(ValueError, match="true/false"):
            hp.to_raw("registration_allowed", bad)


def test_to_raw_rejects_negative_human_rao_and_fixed128():
    with pytest.raises(ValueError, match="cannot be negative"):
        hp.to_raw("min_burn", -0.5)
    with pytest.raises(ValueError, match="cannot be negative"):
        hp.to_raw("burn_increase_mult", -1.5)
    with pytest.raises(ValueError, match="must be within"):
        hp.to_raw("burn_increase_mult", 0.5)


def test_to_raw_overflow_becomes_value_error():
    with pytest.raises(ValueError, match="out of range"):
        hp.to_raw("burn_increase_mult", "1e10000")


def test_to_raw_human_forms_unchanged():
    assert hp.to_raw("kappa", 0.5) == round(0.5 * U16_MAX)
    assert hp.to_raw("kappa", "0.5") == round(0.5 * U16_MAX)
    assert hp.to_raw("bonds_moving_avg", 0.9) == 900_000
    assert hp.to_raw("min_burn", 0.7) == 700_000_000
    assert hp.to_raw("registration_allowed", "true") == 1
    with pytest.raises(ValueError, match="normalized fraction"):
        hp.to_raw("kappa", 1.5)
    with pytest.raises(ValueError, match="must be within"):
        hp.to_raw("kappa", -1)
