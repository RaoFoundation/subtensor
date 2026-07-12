"""Multisig accounts: sign a call with an M-of-N signer set.

A ``Multisig`` is a plain description of a signer set (its signatories and the
threshold of approvals needed) plus its deterministic on-chain address. It is the
signing counterpart to a call: any call — a transfer, a ``Sudo.sudo`` runtime
upgrade, a batch — can be dispatched by a multisig instead of a single key.

Each signatory calls :meth:`approve` with the *same* call. The first N-1
approvals are recorded on-chain; the Nth reaches the threshold and executes the
inner call atomically. The address is where the account's funds live (fund it,
query its balance) and, for governance, is the account that holds sudo.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from .signing import WalletLike, resolve_signer

if TYPE_CHECKING:
    from .client import Client


@dataclass
class Multisig:
    """An M-of-N signer set and its derived account address.

    Create one via ``await client.multisig(signatories, threshold)`` so the
    address is derived against the connected runtime. Signatory order does not
    matter — the address is deterministic in the *set* and threshold.
    """

    signatories: list[str]
    threshold: int
    address: str
    _client: "Client"
    _account: Any  # transport MultiAccountId, carries the derived id

    async def approve(
        self,
        call,
        wallet: WalletLike,
        *,
        signer: str = "coldkey",
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = False,
    ):
        """Approve ``call`` as one signatory; executes it once the threshold is met.

        ``call`` is a generated builder from ``bittensor.calls`` (composed
        automatically). Returns a typed ``ExtrinsicResult``: the executing
        (threshold-reaching) approval carries the inner call's events, earlier
        approvals just record consent. Every signatory must pass the identical
        call for the on-chain hashes to match.
        """
        composed = await self._client.compose(call)
        keypair = resolve_signer(wallet, signer)
        return await self._client._substrate.submit_multisig(
            composed,
            keypair,
            self._account,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    def __repr__(self) -> str:
        return (
            f"Multisig(address={self.address!r}, "
            f"threshold={self.threshold}, signatories={len(self.signatories)})"
        )
