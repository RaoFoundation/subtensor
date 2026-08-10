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

import asyncio
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Optional

from ._generated import calls as generated_calls
from ._generated import constants as generated_constants
from ._generated import storage as generated_storage
from .result import ChainError, ErrorCode
from .signing import WalletLike, public_view, resolve_signer
from .sp_core import ss58_decode

if TYPE_CHECKING:
    from .client import Client


class MultisigFundingError(ChainError):
    """Raised before submission when the signer cannot pay for a multisig approval.

    The opening approval reserves ``DepositBase + threshold * DepositFactor``
    from the signer on top of the transaction fee (the deposit is returned when
    the operation executes or is cancelled); failing client-side keeps the
    exact funding requirement in the diagnostic instead of the chain's bare
    "cannot cover the transaction fee".
    """

    def __init__(self, message: str, *, fund_help: str):
        super().__init__(message, code=ErrorCode.INSUFFICIENT_BALANCE)
        self.fund_help = fund_help

    @property
    def remediation(self) -> str:
        return self.fund_help


def _tao(rao: int) -> str:
    """Exact τ display with trailing zeros trimmed (``τ0.2808``)."""
    whole, frac = divmod(int(rao), 10**9)
    text = f"{whole}.{frac:09d}".rstrip("0").rstrip(".")
    return f"τ{text}"


def _tao_ceil(rao: int, decimals: int = 3) -> str:
    """τ amount rounded UP to ``decimals`` places, for suggested funding."""
    step = 10 ** (9 - decimals)
    units = -(-int(rao) // step)
    whole, frac = divmod(units, 10**decimals)
    text = f"{whole}.{frac:0{decimals}d}".rstrip("0").rstrip(".")
    return f"τ{text}"


async def multisig_deposit_rao(substrate, threshold: int) -> int:
    """The deposit the opener reserves: ``DepositBase + threshold * DepositFactor``."""
    base, factor = await asyncio.gather(
        substrate.constant(*generated_constants.Multisig.DepositBase),
        substrate.constant(*generated_constants.Multisig.DepositFactor),
    )
    return int(base) + int(threshold) * int(factor)


async def _free_balance_rao(substrate, ss58: str) -> int:
    account = await substrate.query(*generated_storage.System.Account, [ss58])
    if not account:
        return 0
    return int(account["data"]["free"])


def _signer_display(ss58: str, label: Optional[str]) -> str:
    if label and label != ss58:
        return f"{ss58} ({label})"
    return ss58


def _funding_error(
    *,
    signer_ss58: str,
    signer_label: Optional[str],
    free_rao: int,
    fee_rao: int,
    deposit_rao: int,
) -> MultisigFundingError:
    display = _signer_display(signer_ss58, signer_label)
    # Suggest the deposit plus twice the estimated fee so fee-estimate drift
    # cannot leave the account short a second time.
    amount = _tao_ceil(max(deposit_rao + 2 * fee_rao - free_rao, 0))
    # The message carries the bare ss58 (the CLI decorates known addresses
    # with local names itself); the help line spells out ``ss58 (name)``.
    if deposit_rao:
        message = (
            "the signing account cannot cover the multisig deposit plus the "
            f"transaction fee: {signer_ss58} holds {_tao(free_rao)} free, but "
            f"opening this operation reserves a {_tao(deposit_rao)} deposit on "
            f"top of a ~{_tao(fee_rao)} fee"
        )
        fund_help = f"fund {display} with ≥ {amount} — the deposit is returned when the op executes"
    else:
        message = (
            "the signing account cannot cover the transaction fee: "
            f"{signer_ss58} holds {_tao(free_rao)} free, but this approval "
            f"costs a ~{_tao(fee_rao)} fee"
        )
        fund_help = f"fund {display} with ≥ {amount}"
    return MultisigFundingError(message, fund_help=fund_help)


async def check_multisig_funds(
    substrate,
    *,
    signer_ss58: str,
    threshold: int,
    opening: bool,
    outer_call: Any = None,
    fee_keypair: Any = None,
    signer_label: Optional[str] = None,
) -> None:
    """Fail early when the signer cannot pay for a multisig approval.

    An opening approval reserves the multisig deposit from the signer on top
    of the fee; later approvals only pay the fee. Best-effort: any failure
    reading chain state skips the check (the chain stays the authority), so
    this can never block an otherwise submittable call.
    """
    try:
        deposit = 0
        if opening and threshold >= 2:
            deposit = await multisig_deposit_rao(substrate, threshold)
        fee = 0
        if outer_call is not None and fee_keypair is not None:
            fee = int((await substrate.estimate_fee(outer_call, fee_keypair)).rao)
        if deposit == 0 and fee == 0:
            return
        free = await _free_balance_rao(substrate, signer_ss58)
    except Exception:
        return
    if free >= deposit + fee:
        return
    raise _funding_error(
        signer_ss58=signer_ss58,
        signer_label=signer_label,
        free_rao=free,
        fee_rao=fee,
        deposit_rao=deposit,
    )


# Covers the fee when only the deposit is known (finney fees are well under this).
_FEE_ALLOWANCE_RAO = 5_000_000


async def multisig_opening_shortfall(
    substrate, *, signer_ss58: str, threshold: int
) -> Optional[str]:
    """Warning text when the signer cannot cover an opening deposit, or None.

    The cheap variant of :func:`check_multisig_funds` for pre-confirm warnings:
    no call is available yet, so it compares the free balance against the
    deposit alone (with a small fee allowance in the suggested amount).
    """
    if threshold < 2:
        return None
    try:
        deposit = await multisig_deposit_rao(substrate, threshold)
        free = await _free_balance_rao(substrate, signer_ss58)
    except Exception:
        return None
    if free >= deposit:
        return None
    amount = _tao_ceil(deposit + _FEE_ALLOWANCE_RAO - free)
    return (
        f"the opening approval reserves a {_tao(deposit)} multisig deposit the "
        f"signer cannot cover: fund {signer_ss58} with ≥ {amount} — the deposit "
        "is returned when the op executes"
    )


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
        await self._preflight_funds(composed, wallet, signer)
        keypair = resolve_signer(wallet, signer)
        return await self._client._substrate.submit_multisig(
            composed,
            keypair,
            self._account,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    async def _preflight_funds(self, composed, wallet: WalletLike, signer: str) -> None:
        """Refuse an approval whose signer cannot pay, before anything signs.

        Whether this approval *opens* the operation (and so reserves the
        deposit) is read from pending state: no entry for this call hash means
        the signer is the opener. The fee is estimated against the same
        ``as_multi`` wrapper the transport will submit. Everything up to the
        comparison is best-effort — a failure here must not block submission.
        """
        substrate = self._client._substrate
        try:
            pub = public_view(wallet, signer)
            call_hash = "0x" + bytes(composed.call_hash).hex()
            pending = await substrate.query(
                *generated_storage.Multisig.Multisigs, [self.address, call_hash]
            )
            outer = None
            if self.threshold >= 2:
                others = sorted(
                    (s for s in self.signatories if s != pub.ss58_address),
                    key=lambda s: bytes(ss58_decode(s)),
                )
                max_weight = await substrate.estimate_weight(composed, pub)
                outer = await self._client.compose(
                    generated_calls.Multisig.as_multi(
                        threshold=self.threshold,
                        other_signatories=others,
                        maybe_timepoint=None,
                        call=composed,
                        max_weight=max_weight,
                    )
                )
        except Exception:
            return
        await check_multisig_funds(
            substrate,
            signer_ss58=pub.ss58_address,
            threshold=self.threshold,
            opening=not pending,
            outer_call=outer,
            fee_keypair=pub,
            signer_label=getattr(wallet, "name", None),
        )

    def __repr__(self) -> str:
        return (
            f"Multisig(address={self.address!r}, "
            f"threshold={self.threshold}, signatories={len(self.signatories)})"
        )
