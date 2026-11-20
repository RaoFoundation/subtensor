"""Unit tests for extension-backed signing account selection."""

from __future__ import annotations

from unittest.mock import AsyncMock, Mock

import pytest

from bittensor.extension.client import ExtensionAccount
from bittensor.extension.signer import ExtensionSigner, account_crypto_type
from bittensor.sp_core import CRYPTO_ED25519, CRYPTO_SR25519


def test_account_crypto_type_sr25519() -> None:
    assert account_crypto_type("sr25519") == CRYPTO_SR25519


def test_account_crypto_type_ed25519() -> None:
    assert account_crypto_type("ed25519") == CRYPTO_ED25519


@pytest.mark.parametrize("account_type", ["ecdsa", "ethereum", "ECDSA"])
def test_account_crypto_type_rejects_unsupported(account_type: str) -> None:
    with pytest.raises(ValueError, match="not supported for Substrate signing"):
        account_crypto_type(account_type)


@pytest.mark.asyncio
async def test_two_stage_labels_shielded_payloads() -> None:
    bridge = Mock()
    bridge.sign_extrinsic_payload = AsyncMock(return_value={"signature": "0x00"})
    signer = ExtensionSigner(
        ExtensionAccount(address="5F", name="alice", source="talisman", type="sr25519"),
        bridge,
        public_key=b"\x00" * 32,
        crypto_type=CRYPTO_SR25519,
    )
    signer.two_stage = True
    payload = {"method": "0x01"}

    await signer.sign_extrinsic_payload(payload)
    await signer.sign_extrinsic_payload(payload)

    first, second = bridge.sign_extrinsic_payload.await_args_list
    assert first.kwargs["stage"] == "approve 1 of 2 — the shielded transaction"
    assert first.kwargs["more_coming"] is True
    assert second.kwargs["stage"] == "approve 2 of 2 — the encrypted carrier"
    assert second.kwargs["more_coming"] is False
