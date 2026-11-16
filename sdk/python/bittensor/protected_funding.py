"""Live fee and announcement-deposit budgets for a protected controller.

All amounts are rao. These are estimates for representative calls, not a fee
guarantee: runtime upgrades, congestion and larger payloads can change costs.
Reserved announcement deposits are refundable and are never counted as fees.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

from ._generated import calls
from ._policy import evaluate_policy


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

    def _policy_state(self) -> dict:
        return {
            name: int(value) if name.endswith("_rao") else value
            for name, value in evaluate_policy("protected.funding_state", asdict(self)).items()
        }

    @property
    def available_rao(self) -> int:
        return self._policy_state()["available_rao"]

    @property
    def low(self) -> bool:
        return self._policy_state()["low"]

    @property
    def top_up_rao(self) -> int:
        return self._policy_state()["top_up_rao"]

    def to_dict(self) -> dict:
        return self._policy_state()


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
    result = evaluate_policy(
        "protected.funding", {**locals(), "signature_estimate": "standard sr25519 envelope"}
    )
    return ControllerFunding(
        **{
            name: int(result[name]) if name.endswith("_rao") else result[name]
            for name in ControllerFunding.__dataclass_fields__
        }
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
