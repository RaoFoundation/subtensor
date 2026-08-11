"""Unit tests for ``bittensor.evm`` address math, conversions, and ABI helpers."""

from __future__ import annotations

import json
import stat
from pathlib import Path

import pytest
from eth_account import Account
from eth_account.messages import encode_defunct
from eth_utils import keccak

from bittensor.balance import Balance
from bittensor.evm import addresses, precompiles, rpc, transactions
from bittensor.evm.keys import write_keystore_file
from tests.harness.samples import ALICE, ALICE_HOT, BOB_HOT

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

    def test_total_coldkey_stake_on_subnet_encode(self):
        fn_abi = precompiles.get_precompile("staking-v2").function("getTotalColdkeyStakeOnSubnet")
        data = precompiles.encode_call(fn_abi, [BOB_HOT, 1])
        assert data.startswith("0x")

    @pytest.mark.parametrize(
        "function_name",
        ["approve", "increaseAllowance", "decreaseAllowance", "transferStakeFrom"],
    )
    def test_staking_v2_state_changers_are_nonpayable(self, function_name: str):
        fn_abi = precompiles.get_precompile("staking-v2").function(function_name)
        assert fn_abi["stateMutability"] == "nonpayable"

    def test_new_bounded_array_calls_encode(self):
        claim_root = precompiles.get_precompile("staking-v2").function("claimRoot")
        assert precompiles.encode_call(claim_root, ["[1, 2]"]).startswith("0x")

        batch_commit = precompiles.get_precompile("neuron").function("batchCommitWeights")
        assert precompiles.encode_call(
            batch_commit,
            ["[1, 2]", ["0x" + "11" * 32, "0x" + "22" * 32]],
        ).startswith("0x")


# The vendored ABIs in bittensor/evm/abi must stay in sync with the canonical
# .abi artifacts in precompiles/src/solidity (see the bittensor.evm.precompiles
# module docstring for why they are vendored). Only checkable in the monorepo;
# skipped in a standalone sdist/wheel checkout where precompiles/ doesn't exist.
_VENDORED_ABI_DIR = Path(precompiles.__file__).parent / "abi"
_CANONICAL_ABI_DIR = Path(__file__).parents[4] / "precompiles" / "src" / "solidity"


@pytest.mark.skipif(
    not _CANONICAL_ABI_DIR.is_dir(),
    reason="canonical .abi files only exist in the subtensor monorepo",
)
class TestVendoredAbiSync:
    @pytest.mark.parametrize(
        "json_path",
        sorted(_VENDORED_ABI_DIR.glob("*.json")),
        ids=lambda p: p.stem,
    )
    def test_vendored_abi_matches_canonical(self, json_path: Path):
        canonical_path = _CANONICAL_ABI_DIR / f"{json_path.stem}.abi"
        assert canonical_path.is_file(), (
            f"{json_path.name} has no counterpart at {canonical_path}; "
            "remove the vendored copy or add the canonical .abi file"
        )
        vendored = json.loads(json_path.read_text())
        canonical = json.loads(canonical_path.read_text())
        assert vendored == canonical, (
            f"{json_path.name} has drifted from {canonical_path.name}; "
            "re-vendor it from precompiles/src/solidity"
        )

    def test_every_registered_abi_file_is_vendored(self):
        for precompile in precompiles.PRECOMPILES.values():
            assert (_VENDORED_ABI_DIR / precompile.abi_file).is_file()


class TestKeystorePermissions:
    def test_write_keystore_file_is_owner_only(self, tmp_path):
        path = tmp_path / "nested" / "key"
        write_keystore_file(path, {"address": "0x" + "11" * 20, "crypto": {}})
        mode = path.stat().st_mode & 0o777
        assert mode == stat.S_IRUSR | stat.S_IWUSR
