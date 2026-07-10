"""Multisig: dispatch a call from a shared-custody composite account.

A multisig account is a deterministic address derived from a set of signatories
and a threshold; a call runs from it once ``threshold`` of them approve. The flow:

  1. the first signer calls ``multisig_execute`` (or ``multisig_approve``) with
     ``timepoint=None`` to open the operation;
  2. other signers ``multisig_approve`` with the opening ``timepoint`` (from the
     ``multisig`` read) until ``threshold - 1`` approvals exist;
  3. the final signer calls ``multisig_execute`` with the ``timepoint`` and the
     full inner call, which runs it.

For a 1-of-N multisig, ``multisig_threshold_1`` dispatches immediately. The inner
call is given as ``{"op": <intent op>, ...args}`` (like a batch child), and its
args must be fully explicit — it dispatches as the multisig account, not the
signer's wallet.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ..signing import public_view
from ..sp_core import ss58_decode
from .base import BuiltCall, Intent
from .registry import build as build_intent
from .registry import register


def _sorted_signatories(signatories: list) -> list:
    """Other signatories must be sorted by raw account id (not ss58 text) and unique."""
    return sorted(set(signatories), key=lambda s: bytes(ss58_decode(s)))


def _validate_multisig(threshold: int, other_signatories: list, signer_ss58: str) -> None:
    """Reject parameters the chain would refuse, before composing anything."""
    if threshold < 2:
        raise ValueError(
            f"multisig threshold must be at least 2, got {threshold} "
            "(use multisig_threshold_1 for 1-of-N)"
        )
    if threshold > len(other_signatories) + 1:
        raise ValueError(
            f"multisig threshold {threshold} exceeds the signatory count "
            f"{len(other_signatories) + 1} (signer plus other_signatories)"
        )
    signer_id = bytes(ss58_decode(signer_ss58))
    if any(bytes(ss58_decode(s)) == signer_id for s in other_signatories):
        raise ValueError(
            "other_signatories must not include the signer's own account "
            f"({signer_ss58}); list only the other members"
        )


async def _compose_inner(substrate, wallet: Any, spec: dict):
    """Build the inner call from a ``{"op": ..., ...args}`` spec (like a batch child)."""
    args = dict(spec)
    op = args.pop("op", None)
    if not op:
        raise ValueError("multisig inner call needs an 'op' key")
    built = await build_intent(op, args).build(substrate, wallet)
    return built.call if isinstance(built, BuiltCall) else built


def _timepoint(value: Optional[dict]):
    """Normalize a timepoint dict to the chain's ``{height, index}`` (or None)."""
    if value is None:
        return None
    return {"height": int(value["height"]), "index": int(value["index"])}


def _call_bytes_hex(value: Any) -> str:
    """0x-hex of a composed call's SCALE bytes (a CallBytes or raw bytes)."""
    raw = getattr(value, "data", value)  # CallBytes keeps its bytes in .data
    if isinstance(raw, (bytes, bytearray)):
        return "0x" + bytes(raw).hex()
    text = str(value)
    return text if text.startswith("0x") else "0x" + text


def _inner_call_extras(inner) -> dict:
    """The inner call's coordinates, surfaced into the execution result.

    The CLI's co-signer followup needs the hash (to look up the pending op)
    and the encoded bytes (so any signer can verify or replay the call without
    this machine's cache).
    """
    return {
        "multisig_call_hash": "0x" + bytes(inner.call_hash).hex(),
        "multisig_call_data": _call_bytes_hex(inner.data),
    }


THRESHOLD_HELP = (
    "Number of approvals required to execute, counting the signer. Together with "
    "the full signatory set it identifies the multisig account, so it must match "
    "on every approval."
)

OTHER_SIGNATORIES_HELP = (
    "The other members of the multisig (every signatory except the signer), as a "
    "JSON list of addresses. The same set must be given on every approval; order "
    "does not matter (sorted automatically)."
)

CALL_HELP = (
    "The inner call to dispatch from the multisig account, as a JSON object "
    '{"op": <intent name>, ...args}. All approvals must describe the identical '
    "call — it is matched by hash. Its arguments must be fully explicit, since it "
    "runs as the multisig account, not the signer's wallet."
)

TIMEPOINT_HELP = (
    "Block height and extrinsic index of the approval that opened the operation, "
    'as a JSON object {"height": ..., "index": ...}. Omit on the first approval; '
    "required on every later one (read it with the multisig query)."
)


@register
@dataclass
class MultisigThreshold1(Intent):
    """Dispatch a 1-of-N multisig call immediately (single approval).

    For multisig accounts with threshold 1, where any single member may act
    alone: the call executes in this same extrinsic, with no approval round,
    no timepoint, and no deposit. The multisig account is derived from the
    signer plus ``other_signatories``, so the full member set must still be
    supplied even though nobody else signs. For thresholds above 1 use
    ``multisig_execute`` / ``multisig_approve`` instead.
    """

    op = "multisig_threshold_1"
    signer = "coldkey"
    wraps = (("Multisig", "as_multi_threshold_1"),)

    other_signatories: list = field(metadata={"help": OTHER_SIGNATORIES_HELP})
    call: dict = field(metadata={"help": CALL_HELP})

    async def build(self, substrate, wallet: Any):
        inner = await _compose_inner(substrate, wallet, self.call)
        return await substrate.compose(
            calls.Multisig.as_multi_threshold_1(
                other_signatories=_sorted_signatories(self.other_signatories), call=inner
            )
        )

    def summary(self) -> str:
        return f"multisig (1-of-N) dispatch {self.call.get('op')}"


@register
@dataclass
class MultisigExecute(Intent):
    """Approve and, if the threshold is met, execute a multisig call (final approval).

    Sends the full inner call along with an approval. If this is the first
    approval (omit ``timepoint``), it opens the operation and reserves a
    deposit from the signer, returned when the operation completes or is
    cancelled. If it is the final approval — bringing the count to
    ``threshold`` — the inner call executes as the multisig account in the
    same extrinsic. Intermediate signers can use the cheaper
    ``multisig_approve`` (hash only), but whoever approves last must use this
    intent so the chain has the call to run. Every approval must repeat the
    same threshold, signatory set, and call; later approvals must also pass
    the opening ``timepoint`` or they will not match the pending operation.
    """

    op = "multisig_execute"
    signer = "coldkey"
    wraps = (("Multisig", "as_multi"),)

    threshold: int = field(metadata={"help": THRESHOLD_HELP})
    other_signatories: list = field(metadata={"help": OTHER_SIGNATORIES_HELP})
    call: dict = field(metadata={"help": CALL_HELP})
    timepoint: Optional[dict] = field(default=None, metadata={"help": TIMEPOINT_HELP})

    async def build(self, substrate, wallet: Any):
        _validate_multisig(self.threshold, self.other_signatories, self.coldkey_address(wallet))
        inner = await _compose_inner(substrate, wallet, self.call)
        max_weight = await substrate.estimate_weight(inner, public_view(wallet, "coldkey"))
        composed = await substrate.compose(
            calls.Multisig.as_multi(
                threshold=self.threshold,
                other_signatories=_sorted_signatories(self.other_signatories),
                maybe_timepoint=_timepoint(self.timepoint),
                call=inner,
                max_weight=max_weight,
            )
        )
        return BuiltCall(composed, _inner_call_extras(inner))

    def summary(self) -> str:
        return (
            f"multisig {self.threshold}-of-{len(self.other_signatories) + 1} "
            f"execute {self.call.get('op')}"
        )


@register
@dataclass
class MultisigApprove(Intent):
    """Register approval for a multisig call (non-final approvals).

    Records the signer's approval for a pending multisig operation without
    dispatching anything. The *opening* approval (omit ``timepoint``; reserves
    a deposit from the signer) embeds the full call in the extrinsic, so every
    co-signer can recover the call — and a ready-to-run command — straight
    from ``multisig pending``, with no out-of-band call data. Intermediate
    approvals (pass the opening ``timepoint``) go up hash-only, which stays
    cheap even for huge calls like a runtime-upgrade blob. It never executes —
    once ``threshold - 1`` approvals exist, the last signatory must send
    ``multisig_execute`` with the full call. Approving twice from the same
    signer, or with a mismatched timepoint, threshold, or signatory set, fails.
    """

    op = "multisig_approve"
    signer = "coldkey"
    wraps = (("Multisig", "as_multi"), ("Multisig", "approve_as_multi"))

    threshold: int = field(metadata={"help": THRESHOLD_HELP})
    other_signatories: list = field(metadata={"help": OTHER_SIGNATORIES_HELP})
    call: dict = field(metadata={"help": CALL_HELP})
    timepoint: Optional[dict] = field(default=None, metadata={"help": TIMEPOINT_HELP})

    async def build(self, substrate, wallet: Any):
        _validate_multisig(self.threshold, self.other_signatories, self.coldkey_address(wallet))
        inner = await _compose_inner(substrate, wallet, self.call)
        max_weight = await substrate.estimate_weight(inner, public_view(wallet, "coldkey"))
        if self.timepoint is None:
            # Opening approval: as_multi with the call embedded. With one
            # approval the threshold (>= 2) cannot be met, so nothing executes;
            # the call bytes land in the opening extrinsic where co-signers'
            # `multisig pending` recovers them from the timepoint. The opener
            # pays the call's length fee once; later approvals are hash-only.
            composed = await substrate.compose(
                calls.Multisig.as_multi(
                    threshold=self.threshold,
                    other_signatories=_sorted_signatories(self.other_signatories),
                    maybe_timepoint=None,
                    call=inner,
                    max_weight=max_weight,
                )
            )
        else:
            composed = await substrate.compose(
                calls.Multisig.approve_as_multi(
                    threshold=self.threshold,
                    other_signatories=_sorted_signatories(self.other_signatories),
                    maybe_timepoint=_timepoint(self.timepoint),
                    call_hash=inner.call_hash,
                    max_weight=max_weight,
                )
            )
        return BuiltCall(composed, _inner_call_extras(inner))

    def summary(self) -> str:
        return (
            f"multisig {self.threshold}-of-{len(self.other_signatories) + 1} "
            f"approve {self.call.get('op')}"
        )


@register
@dataclass
class MultisigCancel(Intent):
    """Cancel an ongoing multisig operation (only the original depositor may).

    Abandons a pending operation before it collects enough approvals: the
    stored approvals are discarded and the deposit reserved at opening is
    returned. Only the signatory who opened the operation (and paid the
    deposit) may cancel it. The threshold, signatory set, call, and opening
    ``timepoint`` must all match the pending operation exactly.
    """

    op = "multisig_cancel"
    signer = "coldkey"
    wraps = (("Multisig", "cancel_as_multi"),)

    threshold: int = field(metadata={"help": THRESHOLD_HELP})
    other_signatories: list = field(metadata={"help": OTHER_SIGNATORIES_HELP})
    call: dict = field(metadata={"help": CALL_HELP})
    timepoint: dict = field(
        metadata={
            "help": "Block height and extrinsic index of the approval that opened the "
            'operation, as a JSON object {"height": ..., "index": ...}. Required — '
            "it identifies which pending operation to cancel."
        }
    )

    async def build(self, substrate, wallet: Any):
        inner = await _compose_inner(substrate, wallet, self.call)
        return await substrate.compose(
            calls.Multisig.cancel_as_multi(
                threshold=self.threshold,
                other_signatories=_sorted_signatories(self.other_signatories),
                timepoint=_timepoint(self.timepoint),
                call_hash=inner.call_hash,
            )
        )

    def summary(self) -> str:
        return f"cancel multisig operation for {self.call.get('op')}"
