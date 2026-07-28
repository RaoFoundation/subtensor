"""Signing and submitting EVM-side transactions, and the key-association proof.

This is the EVM analogue of the substrate executor: estimate, preview, sign
with a stored key, submit, wait for the receipt. Legacy (type-0) transactions
are used throughout — Frontier accepts them unconditionally, so submission
never depends on base-fee dynamics.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from eth_account.messages import encode_defunct
from eth_utils import keccak, to_checksum_address

from ..balance import Balance
from .addresses import ss58_to_pubkey
from .rpc import EvmRpc, wei_to_balance

if TYPE_CHECKING:
    from eth_account.signers.local import LocalAccount


@dataclass
class EvmTxPreview:
    """What an EVM transaction will do, resolved before signing.

    ``to`` is ``None`` for contract creation (the code rides in ``data``).
    """

    sender: str
    to: "str | None"
    value_wei: int
    data: str
    gas: int
    gas_price_wei: int
    nonce: int
    chain_id: int

    @property
    def value(self) -> Balance:
        return wei_to_balance(self.value_wei)

    @property
    def max_fee(self) -> Balance:
        return wei_to_balance(self.gas * self.gas_price_wei)

    def to_dict(self) -> dict:
        return {
            "from": self.sender,
            "to": self.to or "(contract creation)",
            "value_tao": format(self.value.decimal, "f"),
            "value_wei": self.value_wei,
            "data": self.data,
            "gas": self.gas,
            "gas_price_wei": self.gas_price_wei,
            "max_fee_tao": format(self.max_fee.decimal, "f"),
            "nonce": self.nonce,
            "chain_id": self.chain_id,
        }


def prepare_transaction(
    rpc: EvmRpc,
    sender: str,
    to: "str | None",
    *,
    value_wei: int = 0,
    data: str = "0x",
) -> EvmTxPreview:
    """Resolve nonce, gas, and price for a transaction (fails early on estimate).

    Pass ``to=None`` with the init code in ``data`` for contract creation.

    Note subtensor's ``eth_estimateGas`` fails for *any* invalid transaction —
    insufficient balance, an enabled deployment whitelist, a bad call — not
    just gas problems, so a failure here means the transaction itself is bad.
    """
    estimate = {"from": sender, "value": hex(value_wei), "data": data}
    if to is not None:
        estimate["to"] = to
    return EvmTxPreview(
        sender=sender,
        to=to,
        value_wei=value_wei,
        data=data,
        gas=rpc.estimate_gas(estimate),
        gas_price_wei=rpc.gas_price(),
        nonce=rpc.get_nonce(sender),
        chain_id=rpc.chain_id(),
    )


def send_transaction(
    rpc: EvmRpc,
    account: "LocalAccount",
    preview: EvmTxPreview,
    *,
    wait: bool = True,
) -> dict:
    """Sign a prepared transaction and submit it; returns hash (+ receipt facts)."""
    tx: dict = {
        "value": preview.value_wei,
        "data": preview.data,
        "gas": preview.gas,
        "gasPrice": preview.gas_price_wei,
        "nonce": preview.nonce,
        "chainId": preview.chain_id,
    }
    if preview.to is not None:
        # eth-account rejects all-lowercase h160s as failed EIP-55 checksums.
        tx["to"] = to_checksum_address(preview.to)
    signed = account.sign_transaction(tx)
    tx_hash = rpc.send_raw_transaction(signed.raw_transaction)
    result = {"tx_hash": tx_hash}
    if wait:
        receipt = rpc.wait_for_receipt(tx_hash)
        result["block_number"] = int(receipt["blockNumber"], 16)
        result["gas_used"] = int(receipt["gasUsed"], 16)
        result["success"] = receipt.get("status") == "0x1"
        if receipt.get("contractAddress"):
            result["contract_address"] = receipt["contractAddress"]
    return result


def association_proof(
    account: "LocalAccount", hotkey_ss58: str, block_number: int
) -> tuple[str, int]:
    """The EVM key's ownership signature for ``associate_evm_key``.

    The chain expects an EIP-191 personal-message signature over
    ``hotkey_pubkey (32 bytes) ++ keccak_256(scale(block_number))`` where the
    block number is SCALE-encoded (u64 little-endian). Returns the 65-byte
    r||s||v signature as 0x-hex plus the block number it was produced for.
    """
    hotkey_pubkey = bytes.fromhex(ss58_to_pubkey(hotkey_ss58)[2:])
    block_hash = keccak(int(block_number).to_bytes(8, "little"))
    signed = account.sign_message(encode_defunct(primitive=hotkey_pubkey + block_hash))
    return "0x" + bytes(signed.signature).hex(), block_number
