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
from .._generated import storage as st
from ..evm.addresses import (
    h160_to_ss58,
    normalize_h160,
    resolve_evm_deposit,
    resolve_evm_funding_recipient,
    resolve_evm_recipient,
)
from ..result import BittensorError
from ._money import ALL, UNBOUNDED, Money, Spend, tao_amount
from .base import BuiltCall, Intent
from .registry import register


@register
@dataclass
class FundEvmKey(Intent):
    """Fund an EVM (h160) address with TAO from the signing coldkey.

    An ordinary EVM account's native balance lives at its legacy ss58 mirror.
    Registered hashed aliases instead share their full native account's balance.
    This intent resolves the chain's active mapping and transfers TAO to it;
    the funds then appear as the EVM account's balance
    in MetaMask or any Ethereum tool (displayed with 18 decimals there:
    1 TAO = 1e18). Like any transfer this is irreversible, so double-check
    the address. Protected aliases require their native hashed authorization.
    When hashed aliases are enabled, native funding of unprotected aliases is
    refused: their mapping cannot be guarded against a concurrent registration.
    Send to those H160 addresses from an EVM wallet instead.
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
        """The legacy mirror only; actual funding resolves the chain's active mapping."""
        return h160_to_ss58(self.evm_address)

    async def build(self, substrate, wallet: Any):
        from ..hashed import prepare_recipient_intent, with_recipient_registration
        from .transfer import Transfer

        recipient = await resolve_evm_funding_recipient(substrate, self.evm_address)
        transfer = Transfer(dest_ss58=recipient.address, amount_tao=self.amount_tao)
        prepared, recipients = await prepare_recipient_intent(substrate, transfer)
        call = await prepared.build(substrate, wallet)
        call, extras = await with_recipient_registration(
            substrate, wallet, prepared, call, recipients=recipients
        )
        return BuiltCall(call, extras) if extras else call

    def summary(self) -> str:
        amount = "ALL TAO" if self.amount_tao == ALL else str(self.amount_tao)
        return f"fund EVM address {self.evm_address} with {amount}"

    async def effects(self, substrate, signer_address: str) -> list[str]:
        recipient = await resolve_evm_funding_recipient(substrate, self.evm_address)
        return [f"{self.summary()} (native receiving address {recipient.address})"]

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        recipient = await resolve_evm_recipient(substrate, self.evm_address)
        if recipient.descriptor is not None:
            return ["this EVM alias credits its registered hashed native account directly"]
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
    the amount. Pass ``all`` to claim the entire deposit.
    """

    op = "evm_withdraw"
    signer = "coldkey"
    wraps = (("EVM", "withdraw"),)
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_tao",)

    amount_tao: Money = field(
        metadata={"help": "How much TAO to pull from the mirror, or ``all``."}
    )

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao, allow_all=True)

    async def build(self, substrate, wallet: Any):
        truncated, recipient = await resolve_evm_deposit(substrate, self.coldkey_address(wallet))
        if recipient.descriptor is not None:
            raise BittensorError(
                "EVM deposits are already credited to this native account; "
                "no claim transaction is needed"
            )
        if self.amount_tao == ALL:
            account = await substrate.query(*st.System.Account, [recipient.account])
            rao = int(((account or {}).get("data") or {}).get("free") or 0)
            if rao <= 0:
                raise BittensorError("nothing to claim: the EVM deposit address is empty")
        else:
            rao = self.amount_tao.rao
        return await substrate.compose(calls.EVM.withdraw(address=truncated, value=rao))

    def summary(self) -> str:
        amount = "ALL TAO" if self.amount_tao == ALL else str(self.amount_tao)
        return f"claim {amount} from the coldkey's EVM mirror"
