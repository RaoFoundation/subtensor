"""EVM money movement: funding an EVM key and withdrawing from its mirror.

The Subtensor EVM's accounts (h160) and native accounts (ss58) are disjoint
signing domains bridged by deterministic address mappings (see
``bittensor.evm.addresses``). These intents are the substrate-side halves of
the two money flows across that seam.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, ClassVar

from .._generated import calls
from ..evm.addresses import h160_to_ss58, normalize_h160, ss58_to_h160_truncated
from ._money import ALL, UNBOUNDED, Money, Spend, tao_amount
from .base import Intent
from .registry import register


@register
@dataclass
class FundEvmKey(Intent):
    """Fund an EVM (h160) address with TAO from the signing coldkey.

    An EVM account's native balance lives at its ss58 *mirror* address
    (``blake2_256("evm:" ++ h160)``). This intent computes the mirror and
    transfers TAO to it; the funds then appear as the EVM account's balance
    in MetaMask or any Ethereum tool (displayed with 18 decimals there:
    1 TAO = 1e18). Like any transfer this is irreversible — and only the
    holder of the EVM private key can move the funds afterwards, so
    double-check the address.
    """

    op = "fund_evm_key"
    signer = "coldkey"
    wraps = (("Balances", "transfer_keep_alive"),)
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_tao",)

    evm_address: str = field(metadata={"help": "EVM address to fund, as 0x-prefixed h160 hex."})
    amount_tao: Money = field(metadata={"help": "How much TAO to send."})

    def __post_init__(self):
        self.evm_address = normalize_h160(self.evm_address)
        self.amount_tao = tao_amount(self.amount_tao, allow_all=True)

    @property
    def mirror_ss58(self) -> str:
        return h160_to_ss58(self.evm_address)

    async def build(self, substrate, wallet: Any):
        if self.amount_tao == ALL:
            return await substrate.compose(
                calls.Balances.transfer_all(dest=self.mirror_ss58, keep_alive=True)
            )
        return await substrate.compose(
            calls.Balances.transfer_keep_alive(dest=self.mirror_ss58, value=self.amount_tao.rao)
        )

    def summary(self) -> str:
        amount = "ALL TAO" if self.amount_tao == ALL else str(self.amount_tao)
        return f"fund EVM address {self.evm_address} (mirror {self.mirror_ss58}) with {amount}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "only the EVM private key for this address can move the funds afterwards",
        ]

    def spend(self) -> Spend:
        if self.amount_tao == ALL:
            return UNBOUNDED
        return self.amount_tao


@register
@dataclass
class EvmWithdraw(Intent):
    """Claim TAO deposited to the coldkey's truncated EVM mirror.

    Every native account controls one EVM address: the first 20 bytes of its
    public key (the *truncated* mapping). TAO sent from MetaMask to that
    address's mirror can be pulled into the native account with this call —
    the EVM-to-substrate path that needs no EVM gas. The flow: send TAO from
    the EVM wallet to the address shown by ``btcli evm deposit-address``, then
    claim it with ``btcli evm claim-deposit`` (or ``btcli tx evm-withdraw``).
    This is not ``btcli evm send-to-ss58``, which spends from a stored EVM key
    via the balance-transfer precompile. Fails if the mirror holds less than
    the amount.
    """

    op = "evm_withdraw"
    signer = "coldkey"
    wraps = (("EVM", "withdraw"),)

    amount_tao: Money = field(metadata={"help": "How much TAO to pull from the mirror."})

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)

    async def build(self, substrate, wallet: Any):
        truncated = ss58_to_h160_truncated(self.coldkey_address(wallet))
        return await substrate.compose(
            calls.EVM.withdraw(address=truncated, value=self.amount_tao.rao)
        )

    def summary(self) -> str:
        return f"claim {self.amount_tao} from the coldkey's EVM mirror"
