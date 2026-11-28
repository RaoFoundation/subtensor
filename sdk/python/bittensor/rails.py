"""The canonical rails on other chains: where a spoke's contracts are, and how to call them.

The hub of the rails is on Bittensor and is reached like any pallet. A *spoke* is
another chain with a vault that locks a stablecoin and a portal that sends orders to
the hub. This module holds the spokes that are live and the ABI encodings of the few
calls a client makes on one, so the CLI needs no web3 dependency.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from eth_abi import decode as abi_decode
from eth_abi import encode as abi_encode
from eth_utils import keccak, to_checksum_address

from .evm.addresses import is_h160, ss58_to_pubkey

# What an order asks for, as the contracts number it (`Wire.sol`).
TO_STABLE, TO_TAO, TO_ALPHA, TO_CREDITS, TO_NATIVE_TUSD, TO_NATIVE_TAO = range(6)

# Units: the hub and the canonical tokens count in 9 decimals.
HUB_DECIMALS = 9

_DESTINATION = "(uint8,uint16,bytes32,address)"
_ZERO_ADDRESS = "0x" + "00" * 20

_ORDER_SENT = keccak(
    text=f"OrderSent(uint64,address,uint8,uint16,uint64,{_DESTINATION},uint64,uint64,bytes32)"
)
_CREDITED = keccak(text="Credited(uint64,address,uint8,uint16,uint64,bool,uint64,bool)")


@dataclass(frozen=True)
class Spoke:
    """One chain's side of the rails."""

    name: str
    # The id the hub knows this spoke's portal by.
    bridge: int
    chain_id: int
    rpc_url: str
    vault: str
    portal: str
    stable: str
    stable_symbol: str = "USDC"
    stable_decimals: int = 6

    @property
    def stable_scale(self) -> int:
        """What one unit of the stablecoin is in the hub's units."""
        return 10 ** (HUB_DECIMALS - self.stable_decimals)


# The spokes that are live, by name. A spoke enters this table when its vault and portal
# are deployed and the hub has enabled its bridge.
SPOKES: dict[str, Spoke] = {}

# The gateway contract on Bittensor's EVM, once it is deployed on mainnet.
GATEWAY: Optional[str] = None


def load_gateway(manifest: Optional[str] = None) -> str:
    """The gateway contract on Bittensor's EVM, which passes messages for the hub.

    From a manifest (see :func:`load_spoke`) if one is given or named by
    ``BT_RAILS_MANIFEST``, else :data:`GATEWAY`.
    """
    path = manifest or os.getenv("BT_RAILS_MANIFEST")
    gateway = json.loads(Path(path).expanduser().read_text())["hub"]["gateway"] if path else GATEWAY
    if not gateway or not is_h160(gateway):
        raise ValueError(
            "the rails are not live on a public chain yet; point --manifest (or "
            "BT_RAILS_MANIFEST) at a rig's manifest.json"
        )
    return to_checksum_address(gateway)


def load_spoke(name: Optional[str] = None, manifest: Optional[str] = None) -> Spoke:
    """The spoke called ``name``.

    A manifest is a JSON file that lists spokes, as the local rig writes one
    (``.rails/manifest.json``); ``BT_RAILS_MANIFEST`` names one too. Without a
    manifest the spoke comes from :data:`SPOKES`.
    """
    path = manifest or os.getenv("BT_RAILS_MANIFEST")
    if path:
        entries = json.loads(Path(path).expanduser().read_text())["spokes"]
        spokes = {
            key: Spoke(
                name=key,
                bridge=int(entry["bridge"]),
                chain_id=int(entry["chainId"]),
                rpc_url=entry["rpcUrl"],
                vault=entry["vault"],
                portal=entry["portal"],
                stable=entry["stable"],
                stable_symbol=entry.get("stableSymbol", "USDC"),
                stable_decimals=int(entry.get("stableDecimals", 6)),
            )
            for key, entry in entries.items()
        }
    else:
        spokes = SPOKES
    if not spokes:
        raise ValueError(
            "the rails are not live on a public chain yet; point --manifest (or "
            "BT_RAILS_MANIFEST) at a rig's manifest.json"
        )
    if name is None:
        if len(spokes) > 1:
            raise ValueError(f"name the spoke with --spoke: one of {sorted(spokes)}")
        return next(iter(spokes.values()))
    try:
        return spokes[name]
    except KeyError:
        raise ValueError(f"unknown spoke {name!r}; known: {sorted(spokes)}") from None


# ---------------------------------------------------------------- destinations


def _recipient(address: str) -> str:
    """A 0x address that can be paid. The portal refuses an order for the zero address:
    the hub would settle it, and a token cannot be minted there."""
    if not is_h160(address):
        raise ValueError(f"not a 0x address of 20 bytes: {address!r}")
    if int(address, 16) == 0:
        raise ValueError("the zero address cannot receive")
    return address


def _padded(address: str) -> bytes:
    return bytes(12) + bytes.fromhex(_recipient(address)[2:])


def _account(ss58: str) -> bytes:
    return bytes.fromhex(ss58_to_pubkey(ss58)[2:])


@dataclass(frozen=True)
class Destination:
    """What an order asks for, in the two shapes a client needs: the tuple the spoke's
    contracts take, and the value the hub's runtime API takes."""

    kind: int
    netuid: int
    recipient: bytes
    beneficiary: str
    hub: Any

    @property
    def evm(self) -> tuple:
        return (self.kind, self.netuid, self.recipient, to_checksum_address(self.beneficiary))


def to_stable(to: str) -> Destination:
    return Destination(TO_STABLE, 0, _padded(to), _ZERO_ADDRESS, {"Stable": {"to": to}})


def to_tao(to: str) -> Destination:
    return Destination(TO_TAO, 0, _padded(to), _ZERO_ADDRESS, {"Tao": {"to": to}})


def to_alpha(netuid: int, to: str) -> Destination:
    return Destination(
        TO_ALPHA, netuid, _padded(to), _ZERO_ADDRESS, {"Alpha": {"netuid": netuid, "to": to}}
    )


def to_credits(netuid: int, service_ss58: str, beneficiary: str) -> Destination:
    return Destination(
        TO_CREDITS,
        netuid,
        _account(service_ss58),
        _recipient(beneficiary),
        {"Credits": {"netuid": netuid, "service": service_ss58, "beneficiary": beneficiary}},
    )


def to_native_tusd(account_ss58: str) -> Destination:
    return Destination(
        TO_NATIVE_TUSD,
        0,
        _account(account_ss58),
        _ZERO_ADDRESS,
        {"NativeTusd": {"to": account_ss58}},
    )


def to_native_tao(account_ss58: str) -> Destination:
    return Destination(
        TO_NATIVE_TAO, 0, _account(account_ss58), _ZERO_ADDRESS, {"NativeTao": {"to": account_ss58}}
    )


# ---------------------------------------------------------------- calls on a spoke


def _call(signature: str, types: list[str], values: list) -> str:
    return "0x" + (keccak(text=signature)[:4] + abi_encode(types, values)).hex()


def encode_approve(spender: str, amount: int) -> str:
    """``stable.approve(spender, amount)``."""
    return _call(
        "approve(address,uint256)",
        ["address", "uint256"],
        [to_checksum_address(spender), amount],
    )


def encode_allowance(owner: str, spender: str) -> str:
    return _call(
        "allowance(address,address)",
        ["address", "address"],
        [to_checksum_address(owner), to_checksum_address(spender)],
    )


def encode_balance_of(holder: str) -> str:
    return _call("balanceOf(address)", ["address"], [to_checksum_address(holder)])


def encode_deposit(amount: int, destination: Destination, min_out: int, deadline: int) -> str:
    """``vault.deposit(amount, destination, minOut, deadline)``: ``amount`` in the
    stablecoin's units, ``min_out`` in the hub's 9 decimals, ``deadline`` in Unix seconds."""
    return _call(
        f"deposit(uint256,{_DESTINATION},uint64,uint64)",
        ["uint256", _DESTINATION, "uint64", "uint64"],
        [amount, destination.evm, min_out, deadline],
    )


def encode_sell(amount: int, destination: Destination, min_out: int, deadline: int) -> str:
    """``token.sell(amount, destination, minOut, deadline)`` on a canonical token."""
    return _call(
        f"sell(uint64,{_DESTINATION},uint64,uint64)",
        ["uint64", _DESTINATION, "uint64", "uint64"],
        [amount, destination.evm, min_out, deadline],
    )


def encode_flush(bridge: int, limit: int) -> str:
    """``Gateway.flush``: take up to ``limit`` messages from the head of the queue of
    ``bridge`` and hand them to the transport. A message that is not paid for is deferred."""
    return _call("flush(uint32,uint256)", ["uint32", "uint256"], [bridge, limit])


def encode_send(bridge: int, nonce: int) -> str:
    """``Gateway.send``: hand message ``nonce`` of ``bridge`` to the transport: a deferred
    message, or the head of the queue."""
    return _call("send(uint32,uint64)", ["uint32", "uint64"], [bridge, nonce])


def encode_owed(to: str) -> str:
    """``Portal.owed``: the stablecoin the hub told the portal to pay ``to`` that the
    vault has not released yet, in the stablecoin's units."""
    return _call("owed(address)", ["address"], [to_checksum_address(to)])


def encode_claim(to: str) -> str:
    """``Portal.claim``: have the vault release what its allowance covers of what ``to``
    is owed. The stablecoin goes to ``to``, whoever calls."""
    return _call("claim(address)", ["address"], [to_checksum_address(to)])


def encode_order_fee() -> str:
    return _call("orderFee()", [], [])


def encode_tao_token() -> str:
    return _call("tao()", [], [])


def encode_alpha_token(netuid: int) -> str:
    return _call("alphaTokens(uint16)", ["uint16"], [netuid])


def decode_uint(result: str) -> int:
    return int(result, 16) if result not in ("0x", "") else 0


def decode_address(result: str) -> str:
    return to_checksum_address("0x" + result[-40:])


def order_nonce(receipt: dict, portal: str) -> Optional[int]:
    """The nonce of the order a deposit or a sale sent, from the transaction's receipt."""
    for log in receipt.get("logs", []):
        topics = log.get("topics", [])
        if (
            log.get("address", "").lower() == portal.lower()
            and topics
            and bytes.fromhex(topics[0][2:]) == _ORDER_SENT
        ):
            return int(topics[1], 16)
    return None


def credited_filter(portal: str, order: int, from_block: int) -> dict:
    """The ``eth_getLogs`` filter for the portal's answers to ``order``."""
    return {
        "address": portal,
        "fromBlock": hex(from_block),
        "toBlock": "latest",
        "topics": ["0x" + _CREDITED.hex(), None, None, "0x" + order.to_bytes(32, "big").hex()],
    }


def decode_credited(log: dict) -> dict:
    """A ``Credited`` log of the portal as a record."""
    asset, netuid, amount, answers_order, refund = abi_decode(
        ["uint8", "uint16", "uint64", "bool", "bool"], bytes.fromhex(log["data"][2:])
    )
    return {
        "nonce": int(log["topics"][1], 16),
        "to": decode_address(log["topics"][2]),
        "asset": ("stable", "tao", "alpha")[asset],
        "netuid": netuid if asset == 2 else None,
        "amount": amount,
        "answers_order": answers_order,
        "order": int(log["topics"][3], 16),
        "refund": refund,
        "tx_hash": log.get("transactionHash"),
    }
