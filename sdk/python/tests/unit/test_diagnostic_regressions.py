"""Runtime edge cases exposed while making SDK type checking strict."""

from decimal import Decimal
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from eth_abi import encode as abi_encode
from eth_utils import keccak

from bittensor import Policy
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.extrinsics import watch_status_block
from bittensor.balance import Balance
from bittensor.cli import multisig_helpers
from bittensor.cli.call_names import _resolve_nested_call
from bittensor.evm.precompiles import encode_call
from bittensor.intents._money import UNBOUNDED, normalized_balance
from bittensor.intents.batch import Batch
from bittensor.intents.transfer import Transfer
from bittensor.metagraph import _field_items
from bittensor.reads.subnets import _i64f64
from bittensor.result import BittensorError
from bittensor.signing import public_view, resolve_signer
from tests.harness.samples import BOB


@pytest.mark.parametrize("amount", [1, 0.5, "1", "all", Decimal("1"), None, object()])
def test_unresolved_money_cannot_pass_as_normalized_balance(amount):
    with pytest.raises(BittensorError, match="normalized Balance"):
        normalized_balance(amount)


def test_normalized_balance_preserves_exact_units_and_identity():
    balance = Balance.from_rao(2**80 + 1, 7)
    assert normalized_balance(balance) is balance


@pytest.mark.parametrize("invalid", [1, "all", object()])
def test_invalid_custom_spend_cannot_bypass_a_policy_or_batch(invalid):
    intent = Transfer(BOB, 1)
    intent.spend = lambda: invalid
    with pytest.raises(BittensorError, match="normalized Balance"):
        Policy(max_spend_tao=2).check(intent, None)
    with pytest.raises(BittensorError, match="normalized Balance"):
        Batch([intent]).spend()


def test_batch_retains_unbounded_spend_and_instance_signer():
    batch = Batch([Transfer(BOB, "all")])
    assert batch.spend() is UNBOUNDED
    assert Policy(max_spend_tao=2).check(batch, None)
    hot = Transfer(BOB, 1)
    hot.signer = "hotkey"
    assert Batch([hot]).signer == "hotkey"
    assert Batch.signer == "coldkey"
    assert "signer" not in batch.to_dict()


@pytest.mark.parametrize("status_name", ["inBlock", "finalized"])
@pytest.mark.parametrize("block", [None, 42, [], {}, ""])
def test_inclusion_status_requires_a_block_hash(status_name, block):
    with pytest.raises(SubstrateRequestException, match="requires a block hash"):
        watch_status_block(
            {status_name: block},
            wait_for_inclusion=True,
            wait_for_finalization=status_name == "finalized",
        )


@pytest.mark.parametrize("status_name", ["inBlock", "finalized"])
def test_bare_inclusion_status_is_not_a_success(status_name):
    with pytest.raises(SubstrateRequestException, match="requires a block hash"):
        watch_status_block(
            status_name,
            wait_for_inclusion=True,
            wait_for_finalization=status_name == "finalized",
        )


def test_watch_waits_for_requested_status_and_preserves_hash():
    block_hash = "0x" + "ab" * 32
    assert watch_status_block(
        {"inBlock": block_hash}, wait_for_inclusion=True, wait_for_finalization=False
    ) == (block_hash, False)
    assert (
        watch_status_block(
            {"inBlock": block_hash}, wait_for_inclusion=True, wait_for_finalization=True
        )
        is None
    )
    assert watch_status_block(
        {"finalized": block_hash}, wait_for_inclusion=True, wait_for_finalization=True
    ) == (block_hash, True)


@pytest.mark.parametrize("approvals", [None, [], ["another-signer"]])
async def test_pending_timepoint_allows_missing_or_other_approvals(monkeypatch, approvals):
    row = {"call_hash": "0xab", "timepoint": {"height": 9, "index": 2}}
    if approvals is not None:
        row["approvals"] = approvals
    monkeypatch.setattr(
        multisig_helpers, "list_pending_multisig_ops", AsyncMock(return_value=[row])
    )
    client = SimpleNamespace(multisig=AsyncMock(return_value=SimpleNamespace(address="ms")))
    assert (
        await multisig_helpers.pending_timepoint_for_call(
            client,
            signatories=["signer", "another-signer"],
            threshold=2,
            call_hash="0xab",
            signer_ss58="signer",
        )
        == row["timepoint"]
    )


async def test_duplicate_multisig_approval_still_fails(monkeypatch):
    monkeypatch.setattr(
        multisig_helpers,
        "list_pending_multisig_ops",
        AsyncMock(
            return_value=[
                {
                    "call_hash": "0xab",
                    "approvals": ["signer"],
                    "timepoint": {"height": 9, "index": 2},
                }
            ]
        ),
    )
    client = SimpleNamespace(multisig=AsyncMock(return_value=SimpleNamespace(address="ms")))
    with pytest.raises(ValueError, match="already approved"):
        await multisig_helpers.pending_timepoint_for_call(
            client,
            signatories=["signer", "other"],
            threshold=2,
            call_hash="0xab",
            signer_ss58="signer",
        )


async def test_pending_list_omits_records_without_a_call_hash():
    client = SimpleNamespace(
        query_map=AsyncMock(
            return_value=[
                (["ms"], {"when": {"height": 9, "index": 2}}),
                (["ms", "0xab"], {"when": {"height": 10, "index": 1}}),
            ]
        )
    )
    rows = await multisig_helpers.list_pending_multisig_ops(client, "ms")
    assert [row["call_hash"] for row in rows] == ["0xab"]
    assert rows[0]["approvals"] == []


@pytest.mark.parametrize("value", [{1: {}}, {"System": {2: {}}}, {"unknown": {}}])
def test_unknown_nested_calls_pass_through_for_downstream_validation(value):
    assert _resolve_nested_call(None, value, "call") is value


def test_commitment_variant_names_must_be_strings():
    assert _field_items({7: "invalid", "Raw3": "0x616263"}) == [("Raw3", "0x616263")]


@pytest.mark.parametrize("value,expected", [(None, 0), ({"bits": 2**63}, 0.5), (-(2**64), -1)])
def test_fixed_point_values(value, expected):
    assert _i64f64(value) == expected


def test_fixed_point_rejects_non_numeric_bits():
    with pytest.raises(TypeError, match="numeric bits"):
        _i64f64({"bits": [1]})


def test_precompile_selector_expands_tuple_array_components():
    fn = {
        "name": "submit",
        "type": "function",
        "inputs": [
            {
                "name": "entries",
                "type": "tuple[]",
                "components": [
                    {"name": "id", "type": "uint256"},
                    {"name": "owner", "type": "address"},
                ],
            }
        ],
    }
    selector = keccak(text="submit((uint256,address)[])")[:4]
    expected = selector + abi_encode(["(uint256,address)[]"], [[]])
    assert encode_call(fn, [[]]) == "0x" + expected.hex()


def test_wallet_public_view_prefers_public_hotkey_without_unlocking(tmp_path):
    from bittensor.wallet import Wallet

    class WalletShape(Wallet):
        hotkeypub = object()

        @property
        def hotkey(self):
            raise AssertionError("must not unlock the private hotkey")

    wallet = WalletShape(path=str(tmp_path))
    assert public_view(wallet, "hotkey") is WalletShape.hotkeypub


def test_wallet_without_hotkey_public_view_falls_back_to_legacy_hotkey():
    wallet = SimpleNamespace(coldkey=None, coldkeypub=None, hotkey=object())
    assert public_view(wallet, "hotkey") is wallet.hotkey


async def test_extension_signer_resolves_and_signs_asynchronously():
    from bittensor.extension.client import ExtensionAccount
    from bittensor.extension.signer import ExtensionSigner

    signature = bytes(range(64))
    bridge = SimpleNamespace(
        sign_bytes=AsyncMock(return_value={"signature": "0x" + signature.hex()})
    )
    signer = ExtensionSigner(
        ExtensionAccount(address=BOB, name="bob", source="talisman", type="sr25519"),
        bridge,
        public_key=b"\x01" * 32,
        crypto_type=1,
    )
    assert resolve_signer(signer) is signer
    assert await signer.sign(b"hello") == signature
    bridge.sign_bytes.assert_awaited_once_with(BOB, "0x68656c6c6f")
