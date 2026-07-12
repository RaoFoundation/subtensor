"""Unit tests for extension-backed signing account selection."""

from __future__ import annotations

import pytest

from bittensor.extension.signer import account_crypto_type
from bittensor.sp_core import CRYPTO_ED25519, CRYPTO_SR25519


def test_account_crypto_type_sr25519() -> None:
    assert account_crypto_type("sr25519") == CRYPTO_SR25519


def test_account_crypto_type_ed25519() -> None:
    assert account_crypto_type("ed25519") == CRYPTO_ED25519


@pytest.mark.parametrize("account_type", ["ecdsa", "ethereum", "ECDSA"])
def test_account_crypto_type_rejects_unsupported(account_type: str) -> None:
    with pytest.raises(ValueError, match="not supported for Substrate signing"):
        account_crypto_type(account_type)
