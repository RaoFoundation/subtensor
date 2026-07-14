"""Key association: link a hotkey to a coldkey, or an EVM key to a hotkey."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from .base import Intent
from .registry import register


@register
@dataclass
class AssociateHotkey(Intent):
    """Associate a hotkey with the signing coldkey.

    Records on chain that the signing coldkey owns the hotkey, without
    registering it on any subnet or staking anything. Useful when the pairing
    should be visible before the hotkey's first registration (registration and
    staking establish it as a side effect). The chain only associates hotkeys
    that are not already owned: if the hotkey already belongs to a coldkey the
    call succeeds but is silently a no-op — it does not error, and it does not
    take over the hotkey.
    """

    op = "associate_hotkey"
    signer = "coldkey"
    wraps = (("SubtensorModule", "try_associate_hotkey"),)

    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey to record as owned by the signing coldkey."},
    )

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(calls.SubtensorModule.try_associate_hotkey(hotkey=hotkey))

    def summary(self) -> str:
        return f"associate hotkey {self.hotkey_ss58 or 'the wallet hotkey'} with the coldkey"


@register
@dataclass
class AssociateEvmKey(Intent):
    """Associate an EVM key with a hotkey on a subnet.

    Links an Ethereum-style (H160) address to the signing hotkey on one subnet,
    letting EVM-side activity be attributed to that neuron. Signed by the
    hotkey, and additionally proven by the EVM key itself: ``signature`` must
    be an EIP-191 personal-sign signature by the EVM key over the message
    ``hotkey_pubkey (32 bytes) ++ keccak_256(scale(block_number))``, where the
    block number is SCALE-encoded (u64 little-endian). Use
    ``bittensor.evm.transactions.association_proof`` to produce it (it needs
    the EVM private key); a wrong message, block number, or key makes the
    chain reject the call. Prerequisites: the hotkey must be registered on
    ``netuid`` (else HotKeyNotRegisteredInSubNet) and have an owning coldkey,
    and re-association is rate-limited to once per 7,200 blocks (~1 day) per
    neuron.
    """

    op = "associate_evm_key"
    signer = "hotkey"
    wraps = (("SubtensorModule", "associate_evm_key"),)

    netuid: int = field(metadata={"help": "Subnet on which the EVM key association is recorded."})
    evm_key: str = field(
        metadata={"help": "EVM address to link to the hotkey, as 0x-prefixed H160 hex."}
    )
    block_number: int = field(
        metadata={
            "help": "Block number the signature was produced for; part of the signed message."
        }
    )
    signature: str = field(
        metadata={
            "help": "The EVM key's ownership proof, as 0x-prefixed hex: an EIP-191 "
            "personal-sign signature over the hotkey public key concatenated with "
            "keccak_256 of the SCALE-encoded block number."
        }
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.associate_evm_key(
                netuid=self.netuid,
                evm_key=self.evm_key,
                block_number=self.block_number,
                signature=bytes.fromhex(self.signature.removeprefix("0x")),
            )
        )

    def summary(self) -> str:
        return f"associate EVM key {self.evm_key} with the hotkey on netuid {self.netuid}"
