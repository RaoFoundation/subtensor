"""Unit tests for ``bittensor.evm`` address math, conversions, and ABI helpers."""

from __future__ import annotations

import stat

import pytest

from bittensor.balance import Balance
from bittensor.evm import addresses, precompiles, rpc, transactions
from bittensor.evm.keys import require_eth_account, write_keystore_file
from tests.harness.samples import ALICE, ALICE_HOT, BOB_HOT

pytest.importorskip("eth_account")
pytest.importorskip("eth_abi")
pytest.importorskip("eth_utils")

from eth_account import Account
from eth_account.messages import encode_defunct
from eth_utils import keccak

# Vectors for Frontier HashedAddressMapping<BlakeTwo256>: ss58(blake2_256(b"evm:" ++ h160)).
H160_TO_SS58_VECTORS = {
    "0x1111111111111111111111111111111111111111": (
        "5DDYKUgyHe2Sx8a9oTZPAXrgFaTiGnYZXrecNdeC9bG7TbtX"
    ),
    "0xabababababababababababababababababababab": (
        "5F2S3KWCNrTLuuwPbA6b3kSBveGS27yVvQVipym8TwcvP2cs"
    ),
    "0x1234567890123456789012345678901234567890": (
        "5Ettm6fSye3WnsW22Z1UNYa7Mo4gm2KQdcVYHs6rYuJHnhtj"
    ),
}


class TestAddressMath:
    @pytest.mark.parametrize("h160,expected", H160_TO_SS58_VECTORS.items())
    def test_h160_to_ss58(self, h160: str, expected: str):
        assert addresses.h160_to_ss58(h160) == expected

    def test_ss58_to_pubkey_round_trip(self):
        pubkey = addresses.ss58_to_pubkey(ALICE_HOT)
        assert addresses.pubkey_to_ss58(pubkey) == ALICE_HOT

    def test_ss58_to_h160_truncated_is_twenty_bytes(self):
        truncated = addresses.ss58_to_h160_truncated(ALICE)
        assert len(truncated) == 42
        assert truncated.startswith("0x")
        pubkey_prefix = addresses.ss58_to_pubkey(ALICE)[2:42]
        assert truncated == f"0x{pubkey_prefix}"


class TestWeiConversion:
    def test_balance_to_wei_and_back(self):
        balance = Balance.from_tao("1.5")
        wei = rpc.balance_to_wei(balance)
        assert wei == 1_500_000_000 * rpc.WEI_PER_TAO // 1_000_000_000
        assert rpc.wei_to_balance(wei) == balance

    def test_wei_to_balance_truncates_sub_rao_dust(self):
        one_rao_wei = rpc.WEI_PER_TAO // 1_000_000_000
        assert rpc.wei_to_balance(one_rao_wei - 1).rao == 0


class TestAssociationProof:
    def test_block_hash_matches_scale_u64_le(self):
        block_number = 42
        expected = keccak(block_number.to_bytes(8, "little"))
        hotkey_pubkey = bytes.fromhex(addresses.ss58_to_pubkey(BOB_HOT)[2:])
        account = Account.create()
        signature, _ = transactions.association_proof(account, BOB_HOT, block_number)
        assert len(signature) == 132  # 0x + 65-byte ECDSA signature hex
        recovered = Account.recover_message(
            encode_defunct(primitive=hotkey_pubkey + expected),
            signature=signature,
        )
        assert recovered == account.address


class TestPrecompileEncoding:
    def test_coerce_argument_accepts_ss58_for_bytes32(self):
        fn_abi = precompiles.get_precompile("staking-v2").function("addStake")
        data = precompiles.encode_call(fn_abi, [BOB_HOT, 1_000_000_000, 1])
        assert data.startswith("0x")
        assert len(data) > 10

    def test_balance_transfer_encode(self):
        fn_abi = precompiles.get_precompile("balance-transfer").function("transfer")
        data = precompiles.encode_call(fn_abi, [addresses.ss58_to_pubkey(ALICE)])
        assert data.startswith("0x")


class TestKeystorePermissions:
    def test_write_keystore_file_is_owner_only(self, tmp_path):
        require_eth_account()
        path = tmp_path / "nested" / "key"
        write_keystore_file(path, {"address": "0x" + "11" * 20, "crypto": {}})
        mode = path.stat().st_mode & 0o777
        assert mode == stat.S_IRUSR | stat.S_IWUSR
