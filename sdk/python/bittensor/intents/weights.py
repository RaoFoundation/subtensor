"""Weight-setting intents: adaptive set, timelock commit, and legacy reveal.

``SetWeights`` is the one entry point validators need: it accepts weights as a
``{uid: weight}`` mapping or parallel lists, conforms them to the subnet's
hyperparameters, preflights registration, the rate limit, and the weight
count, and dispatches to the plaintext or timelocked
commit-reveal extrinsic based on the subnet's on-chain configuration — so the
caller never has to know which regime the subnet runs.

``normalize`` (max-upscale + u16 quantize, dropping zeros) delegates to the
shared Rust policy and is re-exported from ``bittensor.intents``.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Mapping, Optional, Sequence

# Module import + attribute access (not `from bittensor_core import ...`):
# ty cannot see into the compiled extension, so named imports fail its check.
import bittensor_core as _core

from .._generated import calls
from .._generated.storage import SubtensorModule as st
from .._policy import evaluate_policy
from ..result import BittensorError, ChainError, ErrorCode
from ..settings import GLOBAL_MAX_SUBNET_COUNT, U16_MAX
from .base import BuiltCall, Intent
from .registry import register

DEFAULT_COMMIT_REVEAL_VERSION = 4

NETUID_HELP = "Subnet whose miners the weights score."
UIDS_HELP = (
    "Miner UIDs being weighted, as a list parallel to weights. Omit when weights "
    "is given as a uid-to-weight mapping."
)
WEIGHTS_HELP = (
    "Relative weight per miner: either a JSON object mapping uid to weight, or a "
    "list parallel to uids. Values are relative, not absolute; they are clipped to "
    "the subnet's max-weight limit, normalized, and quantized before submission."
)
MECHID_HELP = (
    "Mechanism index within the subnet. 0 is the default (and for most subnets the only) mechanism."
)
VERSION_KEY_HELP = (
    "Weights version key checked against the subnet's required version. Leave 0 "
    "unless the subnet owner requires a specific value."
)


def normalize(uids: Sequence[int], weights: Sequence[float]) -> tuple[list[int], list[int]]:
    """Max-upscale and quantize weights to u16, dropping zeros.

    Returns parallel ``(uids, values)`` ready for the chain. All-zero input
    returns two empty lists.
    """
    try:
        result = evaluate_policy(
            "validation.weights.normalize", {"uids": list(uids), "weights": list(weights)}
        )
    except (ValueError, TypeError) as error:
        raise BittensorError(str(error)) from None
    return result[0], result[1]


def clip_to_max_weight(weights: Sequence[float], limit: float) -> list[float]:
    """Apply the same Rust normalization and clipping policy as Node and WASM."""
    try:
        return evaluate_policy(
            "validation.weights.clip", {"weights": list(weights), "limit": limit}
        )
    except (ValueError, TypeError) as error:
        raise BittensorError(str(error)) from None


def _as_pairs(uids: Optional[Sequence[int]], weights: Any) -> tuple[list[int], list[float]]:
    """Resolve the two accepted input shapes into parallel lists.

    Either ``weights`` is a ``{uid: weight}`` mapping (keys may be strings after
    a JSON round-trip) and ``uids`` is omitted, or both parallel lists are given.
    """
    if isinstance(weights, Mapping):
        if uids is not None:
            raise BittensorError(
                "Pass weights as a {uid: weight} mapping OR as parallel uids/weights "
                "lists, not both."
            )
        items = sorted((int(uid), float(weight)) for uid, weight in weights.items())
        map_uids = [uid for uid, _ in items]
        if len(set(map_uids)) != len(map_uids):
            raise BittensorError("Duplicate UIDs in weights; each UID may appear once.")
        return map_uids, [weight for _, weight in items]
    if uids is None or weights is None:
        raise BittensorError(
            "Provide weights as a {uid: weight} mapping, or uids and weights as parallel lists."
        )
    out_uids = [int(u) for u in uids]
    if len(set(out_uids)) != len(out_uids):
        raise BittensorError("Duplicate UIDs in weights; each UID may appear once.")
    return out_uids, [float(w) for w in weights]


@dataclass
class _Preflight:
    """Chain state relevant to a weight submission, read at one block."""

    uid: int
    commit_reveal: bool
    min_allowed_weights: int
    max_weight_limit: int


async def _preflight(substrate, hotkey_ss58: str, netuid: int, mechid: int) -> _Preflight:
    """Check registration and the rate limit before signing anything.

    Raises a :class:`ChainError` carrying the same :class:`ErrorCode` the on-chain
    failure would map to, so callers branch identically whether the problem was
    caught client-side or on-chain. Other chain-side checks (minimum stake,
    Yuma validator permit, version key) are not preflighted here. Null's sole
    validator permit is checked before signing.
    """
    current_block = await substrate.block_number()
    block_hash = await substrate.block_hash(current_block)
    # LastUpdate (like the weights themselves) is keyed by the mechanism storage
    # index; for mechid 0 that index equals the netuid.
    storage_index = mechid * GLOBAL_MAX_SUBNET_COUNT + netuid
    uid_raw, cr_raw, rate_raw, last_raw, min_raw, max_raw, mode = await asyncio.gather(
        substrate.query(*st.Uids, [netuid, hotkey_ss58], block_hash=block_hash),
        substrate.query(*st.CommitRevealWeightsEnabled, [netuid], block_hash=block_hash),
        substrate.query(*st.WeightsSetRateLimit, [netuid], block_hash=block_hash),
        substrate.query(*st.LastUpdate, [storage_index], block_hash=block_hash),
        substrate.query(*st.MinAllowedWeights, [netuid], block_hash=block_hash),
        substrate.query(*st.MaxWeightsLimit, [netuid], block_hash=block_hash),
        substrate.query(*st.SubnetEpochConsensus, [netuid], block_hash=block_hash),
    )

    uid = None if uid_raw is None else int(uid_raw)
    null = mode == "Null" or mode == {"Null": None} or mode == 1
    permits = (
        await substrate.query(*st.ValidatorPermit, [netuid], block_hash=block_hash)
        if null and uid is not None
        else []
    )
    rate_limit = int(rate_raw or 0)
    previous = (
        int(last_raw[uid])
        if uid is not None and isinstance(last_raw, (list, tuple)) and uid < len(last_raw)
        else 0
    )
    decision = evaluate_policy(
        "validation.weights.admission",
        {
            "uid": uid,
            "null": null,
            "permit": uid is not None
            and isinstance(permits, (list, tuple))
            and uid < len(permits)
            and bool(permits[uid]),
            "last": previous,
            "rate": rate_limit,
            "current": current_block,
        },
    )
    if decision:
        if decision["code"] == "NOT_REGISTERED":
            raise ChainError(
                f"Hotkey {hotkey_ss58} is not registered on netuid {netuid}.",
                code=ErrorCode.NOT_REGISTERED,
            )
        if decision["code"] == "NOT_AUTHORIZED":
            raise ChainError(
                f"Only the sole Null validator permit holder may set weights on netuid {netuid}.",
                code=ErrorCode.NOT_AUTHORIZED,
            )
        remaining = int(decision["wait"])
        seconds = remaining * await substrate.block_time()
        raise ChainError(
            f"Weights on netuid {netuid} were last set {current_block - previous} blocks ago; the "
            f"rate limit is {rate_limit} blocks. Retry in ~{remaining} blocks (~{int(seconds)}s).",
            code=ErrorCode.RATE_LIMITED,
        )
    assert uid is not None

    return _Preflight(
        uid=uid,
        commit_reveal=bool(cr_raw),
        min_allowed_weights=int(min_raw or 0),
        max_weight_limit=int(max_raw) if max_raw is not None else U16_MAX,
    )


def _conform(
    uids: list[int], weights: list[float], preflight: _Preflight, netuid: int, raw_u16: bool = False
) -> tuple[list[int], list[int]]:
    """Apply the portable Rust admission, clipping, and quantization policy."""
    try:
        result = evaluate_policy(
            "validation.weights.finalize",
            {
                "uids": uids,
                "weights": weights,
                "uid": preflight.uid,
                "minimum": preflight.min_allowed_weights,
                "maximum": preflight.max_weight_limit,
                "raw": raw_u16,
                "null": raw_u16,
            },
        )
    except (TypeError, ValueError) as error:
        raise BittensorError(str(error)) from None
    if result["insufficient"]:
        raise ChainError(
            f"Subnet {netuid} requires at least {preflight.min_allowed_weights} "
            f"weights; only {len(result['uids'])} are nonzero after normalization.",
            code=ErrorCode.INVALID_ARGUMENT,
        )
    return result["uids"], result["values"]


async def _ensure_raw_null(substrate, netuid: int) -> None:
    """Raw integer submission is explicit and only available in Null mode."""
    mode = await substrate.query("SubtensorModule", "SubnetEpochConsensus", [netuid])
    if mode != "Null" and mode != {"Null": None}:
        raise BittensorError("raw_u16 requires a subnet running Null consensus.")


async def _build_timelocked(
    substrate,
    hotkey_public_key: bytes,
    netuid: int,
    mechid: int,
    uids: list[int],
    values: list[int],
    version_key: int,
    commit_reveal_version: int,
) -> BuiltCall:
    """Timelock-encrypt the weights and compose the commit call.

    The chain auto-reveals timelocked commits at the drand ``reveal_round``; the
    round is surfaced in the result extras so callers can track it.
    """
    current_block = await substrate.block_number()
    block_hash = await substrate.block_hash(current_block)
    # The epoch schedule is stateful (owner-triggered pending epochs, tempo
    # changes), so v2 needs the full schedule state — all read at one pinned
    # block so the simulated pipeline matches what the chain will actually run.
    # The reveal-round math needs the real netuid; the mechanism storage index
    # (mechid * 4096 + netuid) is only a chain storage key and would shift the
    # reveal round for mechid >= 1.
    (
        tempo_raw,
        reveal_raw,
        last_epoch_raw,
        pending_raw,
        epoch_index_raw,
        since_step_raw,
        block_time,
    ) = await asyncio.gather(
        substrate.query(*st.Tempo, [netuid], block_hash=block_hash),
        substrate.query(*st.RevealPeriodEpochs, [netuid], block_hash=block_hash),
        substrate.query(*st.LastEpochBlock, [netuid], block_hash=block_hash),
        substrate.query(*st.PendingEpochAt, [netuid], block_hash=block_hash),
        substrate.query(*st.SubnetEpochIndex, [netuid], block_hash=block_hash),
        substrate.query(*st.BlocksSinceLastStep, [netuid], block_hash=block_hash),
        substrate.block_time(),
    )
    commit_bytes, reveal_round = _core.get_encrypted_commit_v2(
        uids=uids,
        weights=values,
        version_key=version_key,
        last_epoch_block=int(last_epoch_raw or 0),
        pending_epoch_at=int(pending_raw or 0),
        subnet_epoch_index=int(epoch_index_raw or 0),
        tempo=int(tempo_raw),
        blocks_since_last_step=int(since_step_raw or 0),
        current_block=current_block,
        subnet_reveal_period_epochs=int(reveal_raw),
        # Detected from the chain: the drand reveal-round math depends on real
        # wall-clock block time, which is 0.25s on fast-blocks chains.
        block_time=block_time,
        hotkey=hotkey_public_key,
    )
    call = await substrate.compose(
        calls.SubtensorModule.commit_timelocked_mechanism_weights(
            netuid=netuid,
            mecid=mechid,
            commit=commit_bytes,
            reveal_round=reveal_round,
            commit_reveal_version=commit_reveal_version,
        )
    )
    return BuiltCall(call, {"reveal_round": reveal_round})


async def _would_clip(substrate, netuid: int, weights: list[float]) -> Optional[str]:
    """A warning line if the weights exceed the subnet's max_weight_limit, else None."""
    max_raw = await substrate.query(*st.MaxWeightsLimit, [netuid])
    limit = (int(max_raw) if max_raw is not None else U16_MAX) / U16_MAX
    total = sum(weights)
    if len(weights) > 1 and total > 0 and limit < 1.0 and max(weights) / total > limit:
        return (
            f"largest weight exceeds the subnet's max_weight_limit ({limit:.4f} of "
            "total); weights will be clipped to conform"
        )
    return None


@register
@dataclass
class SetWeights(Intent):
    """Set validator weights, auto-selecting plaintext or commit-reveal.

    The one entry point validators need for scoring miners: it conforms the
    weights to the subnet's hyperparameters (max-weight clip, u16 quantization,
    minimum weight count) and submits via whichever path the subnet runs — a
    plain ``set_weights`` when commit-reveal is off, or a timelock-encrypted
    commit (auto-revealed by the chain at the drand reveal round) when it is on.
    Signed by the hotkey, which must be registered on the subnet. Before
    signing it preflights registration and the rate limit, so those failures
    are caught fast with the same error the chain would return; the rate-limit
    error says how many blocks to wait. The chain additionally enforces checks
    that are not preflighted: the hotkey must hold the minimum stake to set
    weights and a validator permit. Null has one permit holder, the highest-stake
    UID; self weights and subnet ownership do not bypass that permit. Yuma retains
    its owner and self-weight exceptions. ``version_key`` must not be older than the subnet's
    required version. Prefer this over ``commit_weights``/``reveal_weights``
    unless you specifically need to force one path.
    """

    op = "set_weights"
    signer = "hotkey"
    wraps = (
        ("SubtensorModule", "set_mechanism_weights"),
        ("SubtensorModule", "commit_timelocked_mechanism_weights"),
    )

    netuid: int = field(metadata={"help": NETUID_HELP})
    uids: Optional[list[int]] = field(default=None, metadata={"help": UIDS_HELP})
    weights: Optional[list[float]] = field(default=None, metadata={"help": WEIGHTS_HELP})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    version_key: int = field(default=0, metadata={"help": VERSION_KEY_HELP})
    raw_u16: bool = field(
        default=False,
        metadata={
            "help": "Preserve exact u16 integer ratios on Null subnets; no scaling or clipping."
        },
    )

    def __post_init__(self):
        self.uids, self.weights = _as_pairs(self.uids, self.weights)

    async def build(self, substrate, wallet: Any):
        preflight = await _preflight(
            substrate, self.hotkey_address(wallet), self.netuid, self.mechid
        )
        if self.raw_u16:
            await _ensure_raw_null(substrate, self.netuid)
        norm_uids, norm_vals = _conform(
            self.uids, self.weights, preflight, self.netuid, self.raw_u16
        )
        if preflight.commit_reveal:
            return await _build_timelocked(
                substrate,
                self.hotkey_public_key(wallet),
                self.netuid,
                self.mechid,
                norm_uids,
                norm_vals,
                self.version_key,
                DEFAULT_COMMIT_REVEAL_VERSION,
            )
        return await substrate.compose(
            calls.SubtensorModule.set_mechanism_weights(
                netuid=self.netuid,
                mecid=self.mechid,
                dests=norm_uids,
                weights=norm_vals,
                version_key=self.version_key,
            )
        )

    async def effects(self, substrate, signer_address: str) -> list[str]:
        enabled = await substrate.query(*st.CommitRevealWeightsEnabled, [self.netuid])
        mode = (
            "timelocked commit (commit-reveal enabled; chain auto-reveals)"
            if enabled
            else "direct plaintext set"
        )
        return [self.summary(), f"submission path: {mode}"]

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.raw_u16:
            return []
        warning = await _would_clip(substrate, self.netuid, self.weights)
        return [warning] if warning else []

    def summary(self) -> str:
        return f"set weights on netuid {self.netuid} for {len(self.uids)} uids"


@register
@dataclass
class CommitWeights(Intent):
    """Timelock-encrypt and commit weights, forcing the commit-reveal path.

    Encrypts the weights with drand timelock encryption and submits the
    ciphertext; the weights stay hidden until the chain auto-decrypts and
    applies them at the reveal round (no separate reveal call is needed).
    Hiding weights until the round closes prevents other validators from
    copying them. Signed by the hotkey, with the same preflight, conforming,
    and rate-limit behavior as ``set_weights``. Use ``set_weights`` instead
    unless you need to force the commit path regardless of the subnet's
    commit-reveal setting.
    """

    op = "commit_weights"
    signer = "hotkey"
    wraps = (("SubtensorModule", "commit_timelocked_mechanism_weights"),)

    netuid: int = field(metadata={"help": NETUID_HELP})
    uids: Optional[list[int]] = field(default=None, metadata={"help": UIDS_HELP})
    weights: Optional[list[float]] = field(default=None, metadata={"help": WEIGHTS_HELP})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    version_key: int = field(default=0, metadata={"help": VERSION_KEY_HELP})
    raw_u16: bool = field(
        default=False,
        metadata={
            "help": "Preserve exact u16 integer ratios on Null subnets; no scaling or clipping."
        },
    )
    commit_reveal_version: int = field(
        default=DEFAULT_COMMIT_REVEAL_VERSION,
        metadata={
            "help": "Commit-reveal protocol version to tag the commit with. Keep the "
            "default unless the subnet requires an older protocol."
        },
    )

    def __post_init__(self):
        self.uids, self.weights = _as_pairs(self.uids, self.weights)

    async def build(self, substrate, wallet: Any):
        preflight = await _preflight(
            substrate, self.hotkey_address(wallet), self.netuid, self.mechid
        )
        if self.raw_u16:
            await _ensure_raw_null(substrate, self.netuid)
        norm_uids, norm_vals = _conform(
            self.uids, self.weights, preflight, self.netuid, self.raw_u16
        )
        return await _build_timelocked(
            substrate,
            self.hotkey_public_key(wallet),
            self.netuid,
            self.mechid,
            norm_uids,
            norm_vals,
            self.version_key,
            self.commit_reveal_version,
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.raw_u16:
            return []
        warning = await _would_clip(substrate, self.netuid, self.weights)
        return [warning] if warning else []

    def summary(self) -> str:
        return f"commit weights on netuid {self.netuid} for {len(self.uids)} uids"


@register
@dataclass
class RevealWeights(Intent):
    """Reveal previously committed weights (legacy salt-based commit-reveal).

    Completes the old two-step commit-reveal flow: the uids, weights, salt, and
    version key must hash to exactly what was committed earlier. The reveal is
    valid on exactly one epoch — the commit's epoch plus the subnet's reveal
    period; revealing earlier fails, and once that epoch has passed the commit
    is expired and dropped. The chain hashes the quantized u16 values, so the
    inputs here must be the same values passed at commit time (``build()``
    re-normalizes floats: identical proportions produce identical u16s).
    Signed by the hotkey that made the commit. Only needed for legacy
    salt-based commits — the timelocked path used by ``set_weights`` and
    ``commit_weights`` is auto-revealed by the chain and never needs this call.
    """

    op = "reveal_weights"
    signer = "hotkey"
    wraps = (("SubtensorModule", "reveal_weights"),)

    netuid: int = field(metadata={"help": NETUID_HELP})
    uids: list[int] = field(
        metadata={"help": "Miner UIDs exactly as committed, as a list parallel to weights."}
    )
    weights: list[float] = field(
        metadata={"help": "Weights exactly as committed, as a list parallel to uids."}
    )
    salt: list[int] = field(
        metadata={"help": "Salt used when the commit was made; must match to reveal."}
    )
    version_key: int = field(
        default=0, metadata={"help": "Version key used when the commit was made."}
    )

    async def build(self, substrate, wallet: Any):
        norm_uids, norm_vals = normalize(self.uids, self.weights)
        if not norm_uids:
            raise BittensorError("All weights are zero; nothing to reveal.")
        return await substrate.compose(
            calls.SubtensorModule.reveal_weights(
                netuid=self.netuid,
                uids=norm_uids,
                values=norm_vals,
                salt=[int(x) for x in self.salt],
                version_key=self.version_key,
            )
        )

    def summary(self) -> str:
        return f"reveal weights on netuid {self.netuid} for {len(self.uids)} uids"
