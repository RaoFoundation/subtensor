"""Transfer intents."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, ClassVar

from .._generated import calls
from ._money import ALL, UNBOUNDED, Money, Spend, tao_amount
from .base import Intent
from .registry import register

KEEP_ALIVE_HELP = (
    "Refuse to drop the sender below the existential deposit. Disable it only when "
    "you intend to empty the account, which lets the chain reap it."
)


@register
@dataclass
class Transfer(Intent):
    """Transfer TAO from the coldkey to a destination address.

    Sends free balance from the signing coldkey to the destination. On-chain
    transfers are irreversible — funds sent to a wrong address cannot be
    recovered, so double-check the destination. A transaction fee is deducted
    from the sender on top of the amount, and the call fails if the free
    balance cannot cover both. Pass ``all`` to sweep the entire transferable
    balance. With ``keep_alive`` (the default) the transfer refuses to drop
    the sender below the existential deposit; disable it only when
    intentionally emptying the account.
    """

    op = "transfer"
    signer = "coldkey"
    wraps = (
        ("Balances", "transfer_keep_alive"),
        ("Balances", "transfer_allow_death"),
        ("Balances", "transfer_all"),
    )
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_tao",)

    dest_ss58: str = field(metadata={"help": "Account the funds are sent to."})
    amount_tao: Money = field(metadata={"help": "How much to send."})
    keep_alive: bool = field(default=True, metadata={"help": KEEP_ALIVE_HELP})

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao, allow_all=True)

    async def build(self, substrate, wallet: Any):
        if self.amount_tao == ALL:
            return await substrate.compose(
                calls.Balances.transfer_all(dest=self.dest_ss58, keep_alive=self.keep_alive)
            )
        value = self.amount_tao.rao
        call = (
            calls.Balances.transfer_keep_alive(dest=self.dest_ss58, value=value)
            if self.keep_alive
            else calls.Balances.transfer_allow_death(dest=self.dest_ss58, value=value)
        )
        return await substrate.compose(call)

    def summary(self) -> str:
        amount = "ALL TAO" if self.amount_tao == ALL else str(self.amount_tao)
        return f"transfer {amount} to {self.dest_ss58}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.amount_tao == ALL:
            return ["transfers the entire transferable balance"]
        return []

    def spend(self) -> Spend:
        if self.amount_tao == ALL:
            # Unbounded: a max_spend policy should block draining the whole account.
            return UNBOUNDED
        return self.amount_tao


@register
@dataclass
class TransferAll(Intent):
    """Transfer the entire transferable balance to a destination address.

    Drains the signing coldkey's transferable balance to the destination in
    one irreversible call — equivalent to ``transfer`` with ``all`` as the
    amount. Staked or otherwise reserved funds are not included; unstake first
    to move those. A spend-cap policy treats this as an unbounded spend and
    blocks it until the cap is raised. With ``keep_alive`` (the default) the
    existential deposit stays behind; disable it to empty and reap the account.
    """

    op = "transfer_all"
    signer = "coldkey"
    wraps = (("Balances", "transfer_all"),)

    dest_ss58: str = field(metadata={"help": "Account the funds are sent to."})
    keep_alive: bool = field(default=True, metadata={"help": KEEP_ALIVE_HELP})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Balances.transfer_all(dest=self.dest_ss58, keep_alive=self.keep_alive)
        )

    def summary(self) -> str:
        return f"transfer ALL TAO to {self.dest_ss58}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["transfers the entire transferable balance"]

    def spend(self) -> Spend:
        # Unbounded: a max_spend policy should block draining the whole account.
        return UNBOUNDED
