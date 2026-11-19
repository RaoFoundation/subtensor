"""Live fee and announcement-deposit budgets for a protected controller.

All amounts are rao. These are estimates for representative calls, not a fee
guarantee: runtime upgrades, congestion and larger payloads can change costs.
Reserved announcement deposits are refundable and are never counted as fees.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

from ._generated import calls


@dataclass(frozen=True)
class ControllerFunding:
    controller: str
    free_rao: int
    frozen_rao: int
    reserved_rao: int
    announcement_deposit_rao: int
    next_announcement_deposit_rao: int
    transfer_fee_rao: int
    staking_fee_rao: int
    target_rao: int
    warning_rao: int
    existential_deposit_rao: int
    fee_payer: str
    signature_estimate: str = "standard sr25519 envelope"

    @property
    def available_rao(self) -> int:
        return max(0, self.free_rao - self.frozen_rao)

    @property
    def low(self) -> bool:
        return self.available_rao < self.warning_rao

    @property
    def top_up_rao(self) -> int:
        return max(0, self.target_rao - self.available_rao)

    def to_dict(self) -> dict:
        return {
            **asdict(self),
            "available_rao": self.available_rao,
            "low": self.low,
            "top_up_rao": self.top_up_rao,
            "can_announce": self.available_rao
            >= self.next_announcement_deposit_rao
            + self.transfer_fee_rao
            + self.existential_deposit_rao,
            "estimate": True,
            "fee_scope": "controller; multisig signatories also need their own fees",
        }


def funding_budget(
    *,
    controller,
    account,
    announcement_count,
    announcement_deposit,
    deposit_base,
    deposit_factor,
    existential_deposit,
    announce_fee,
    transfer_execution_fee,
    staking_execution_fee,
    fee_payer,
) -> ControllerFunding:
    """Allow 20 pending transfers or 10 pending staking operations, whichever costs more."""

    def reserve(additional):
        return max(
            0,
            deposit_base
            + deposit_factor * (announcement_count + additional)
            - announcement_deposit,
        )

    transfer_fee = announce_fee + transfer_execution_fee
    staking_fee = announce_fee + staking_execution_fee
    target = existential_deposit + max(
        reserve(20) + 20 * transfer_fee,
        reserve(10) + 10 * staking_fee,
    )
    warning = existential_deposit + max(
        reserve(5) + 5 * transfer_fee,
        reserve(2) + 2 * staking_fee,
    )
    data = account["data"]
    return ControllerFunding(
        controller=controller,
        free_rao=int(data["free"]),
        frozen_rao=int(data.get("frozen", 0)),
        reserved_rao=int(data["reserved"]),
        announcement_deposit_rao=announcement_deposit,
        next_announcement_deposit_rao=reserve(1),
        transfer_fee_rao=transfer_fee,
        staking_fee_rao=staking_fee,
        target_rao=target,
        warning_rao=warning,
        existential_deposit_rao=existential_deposit,
        fee_payer=fee_payer,
    )


async def controller_funding(client, record, public) -> ControllerFunding:
    """Read spendable funds and quote the actual announce/execute envelopes."""
    substrate = client._substrate
    controller, pure = record["controller_ss58"], record["protected_ss58"]
    from dataclasses import replace

    from .hashed import descriptor_bytes
    from .sp_core import Keypair

    signature_estimate = "standard sr25519 envelope; multisig members pay their own fees"
    for storage in ("MlDsaAccounts", "Accounts"):
        registered = await substrate.query("HashedAccounts", storage, [controller])
        if registered is not None:
            public = Keypair.from_hashed_descriptor(descriptor_bytes(registered["descriptor"]))
            signature_estimate = f"registered controller signature type {public.crypto_type}"
            break
    account = await substrate.query("System", "Account", [controller])
    pending, deposit = await substrate.query("Proxy", "Announcements", [controller])
    base = int(await substrate.constant("Proxy", "AnnouncementDepositBase"))
    factor = int(await substrate.constant("Proxy", "AnnouncementDepositFactor"))
    ed = int(await substrate.constant("Balances", "ExistentialDeposit"))
    announce = await substrate.compose(calls.Proxy.announce(real=pure, call_hash="0x" + "00" * 32))
    announce_fee = (await substrate.estimate_fee(announce, public)).rao
    fees = []
    for inner in (
        calls.Balances.transfer_keep_alive(dest=controller, value=10**9),
        calls.SubtensorModule.add_stake(hotkey=controller, netuid=0, amount_staked=10**9),
    ):
        # Include the proposed refill in the sample payload as well, so the
        # estimate includes the extra transfer and Utility envelope.
        batch = await substrate.compose(
            calls.Utility.batch_all(
                calls=[
                    await substrate.compose(
                        calls.Balances.transfer_keep_alive(
                            dest=controller,
                            value=10**9,
                        )
                    ),
                    await substrate.compose(inner),
                ]
            )
        )
        execute = await substrate.compose(
            calls.Proxy.proxy_announced(
                delegate=controller,
                real=pure,
                force_proxy_type="Any",
                call=batch,
            )
        )
        fees.append((await substrate.estimate_fee(execute, public)).rao)
    state = funding_budget(
        controller=controller,
        account=account,
        announcement_count=len(pending),
        announcement_deposit=int(deposit),
        deposit_base=base,
        deposit_factor=factor,
        existential_deposit=ed,
        announce_fee=announce_fee,
        transfer_execution_fee=fees[0],
        staking_execution_fee=fees[1],
        fee_payer=public.ss58_address,
    )
    return replace(state, signature_estimate=signature_estimate)
