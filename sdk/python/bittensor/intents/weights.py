"""Weight-setting intents: adaptive set, timelock commit, and legacy reveal.

``SetWeights`` is the one entry point validators need: it accepts weights as a
``{uid: weight}`` mapping or parallel lists, conforms them to the subnet's
hyperparameters, preflights registration, the rate limit, and the weight
count, and dispatches to the plaintext or timelocked
commit-reveal extrinsic based on the subnet's on-chain configuration — so the
caller never has to know which regime the subnet runs.

``normalize`` (max-upscale + u16 quantize, dropping zeros) lives here as the one
canonical implementation and is re-exported from ``bittensor.intents``.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from itertools import accumulate
from math import isfinite
from typing import Any, Mapping, Optional, Sequence

# Module import + attribute access (not `from bittensor_core import ...`):
# ty cannot see into the compiled extension, so named imports fail its check.
import bittensor_core as _core

from .._generated import calls
from .._generated.storage import SubtensorModule as st
from ..result import BittensorError, ChainError, ErrorCode
from ..settings import GLOBAL_MAX_SUBNET_COUNT, U16_MAX
from ..sp_core import ss58_decode
from .base import BuiltCall, Intent
from .registry import register

DEFAULT_COMMIT_REVEAL_VERSION = 4
TOTAL_NULL_WEIGHT_BUDGET = 2_500

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
FOR_HOTKEY_HELP = (
    "Validator hotkey to act for. Requires the hotkey to have authorized the signing "
    "hotkey as its weight delegate on this netuid and mechanism; normal transaction fees "
    "are paid by the delegate."
)


def _for_call(name: str, netuid: int, mechid: int, hotkey: Optional[str], **params):
    """Use the exact delegate dispatch when requested, otherwise the hotkey call."""
    if hotkey is None:
        return getattr(calls.SubtensorModule, name)(netuid=netuid, mecid=mechid, **params)
    ss58_decode(hotkey)
    return calls.Call(
        "SubtensorModule",
        f"{name}_for",
        {"netuid": netuid, "mecid": mechid, "hotkey": hotkey, **params},
    )


def _reveal_call(netuid: int, mechid: int, for_hotkey: Optional[str], **params):
    """Keep the legacy main-mechanism call unless a mechanism/delegate is requested."""
    if for_hotkey is not None:
        return _for_call("reveal_mechanism_weights", netuid, mechid, for_hotkey, **params)
    if mechid == 0:
        return calls.SubtensorModule.reveal_weights(netuid=netuid, **params)
    return calls.SubtensorModule.reveal_mechanism_weights(netuid=netuid, mecid=mechid, **params)


def _weight_keys(intent: Intent, wallet: Any, for_hotkey: Optional[str]) -> tuple[str, bytes]:
    """The validator identity: never substitute the delegate for its hotkey."""
    if for_hotkey is not None:
        return for_hotkey, bytes(ss58_decode(for_hotkey))
    return intent.hotkey_address(wallet), intent.hotkey_public_key(wallet)


async def _ensure_delegate(substrate, netuid: int, mechid: int, hotkey: str) -> None:
    """Reject an absent/revoked authorization before encrypting or signing."""
    delegate = await substrate.runtime_call(
        "SubnetRegistrationRuntimeApi", "get_mech_weight_delegate", [netuid, mechid, hotkey]
    )
    if delegate is None:
        raise ChainError(
            f"Hotkey {hotkey} has no weight delegate on netuid {netuid}, mechanism {mechid}. "
            "Set one before submitting a delegated weight call.",
            code=ErrorCode.NOT_AUTHORIZED,
        )


async def _delegate_warning(
    substrate, netuid: int, mechid: int, hotkey: str, origin: str
) -> list[str]:
    delegate = await substrate.runtime_call(
        "SubnetRegistrationRuntimeApi", "get_mech_weight_delegate", [netuid, mechid, hotkey]
    )
    if str(delegate) != str(origin):
        return [
            f"{origin} is not the current weight delegate for {hotkey} on netuid {netuid}, "
            f"mechanism {mechid} (delegate: {delegate or 'none'}); the chain will reject the call"
        ]
    return []


def normalize(uids: Sequence[int], weights: Sequence[float]) -> tuple[list[int], list[int]]:
    """Max-upscale and quantize weights to u16, dropping zeros.

    Returns parallel ``(uids, values)`` ready for the chain. All-zero input
    returns two empty lists.
    """
    if len(uids) != len(weights):
        raise BittensorError(
            f"uids and weights must be equal length: {len(uids)} vs {len(weights)}."
        )
    if any(not isfinite(w) or w < 0 for w in weights):
        raise BittensorError("Weights must be finite and non-negative.")
    if any(u < 0 or u > U16_MAX for u in uids):
        raise BittensorError("UIDs must be in 0..65535.")

    top = max(weights) if weights else 0
    if top == 0:
        return [], []

    out_uids: list[int] = []
    out_vals: list[int] = []
    for uid, weight in zip(uids, weights):
        value = round((weight / top) * U16_MAX)
        if value != 0:
            out_uids.append(int(uid))
            out_vals.append(int(value))
    return out_uids, out_vals


def clip_to_max_weight(weights: Sequence[float], limit: float) -> list[float]:
    """Normalize to sum 1 with no element above ``limit`` (the chain's
    ``max_weight_limit`` check is ``max/sum <= limit``).

    Weights already within the limit are just sum-normalized. Otherwise a cutoff
    is found such that clipping everything above it lands the largest normalized
    weight exactly on the limit — mass is redistributed rather than discarded.
    Empty or all-zero input raises; a limit no distribution can satisfy
    (``len * limit <= 1``) returns the uniform distribution.
    """
    n = len(weights)
    total = float(sum(weights))
    if n == 0 or total <= 0:
        raise BittensorError("All weights are zero; nothing to set.")
    if n * limit <= 1:
        return [1.0 / n] * n

    estimation = sorted(w / total for w in weights)
    if estimation[-1] <= limit:
        return [w / total for w in weights]

    epsilon = 1e-7
    cumsum = list(accumulate(estimation))
    n_values = sum(
        1 for i, e in enumerate(estimation) if e / ((n - i - 1) * e + cumsum[i] + epsilon) < limit
    )
    cutoff_scale = (limit * cumsum[n_values - 1] - epsilon) / (1 - limit * (n - n_values))
    cutoff = cutoff_scale * total

    clipped = [min(float(w), cutoff) for w in weights]
    clipped_total = sum(clipped)
    return [w / clipped_total for w in clipped]


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
    # Null only: the most entries one weight row may hold on a mechanism,
    # max(1, 2500 // mechanism_count). None on Yuma, which has no such cap.
    max_row_len: Optional[int] = None


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
    (
        uid_raw,
        cr_raw,
        rate_raw,
        last_raw,
        min_raw,
        max_raw,
        mode,
        mechanism_count_raw,
        subnet_n_raw,
    ) = await asyncio.gather(
        substrate.query(*st.Uids, [netuid, hotkey_ss58], block_hash=block_hash),
        substrate.query(*st.CommitRevealWeightsEnabled, [netuid], block_hash=block_hash),
        substrate.query(*st.WeightsSetRateLimit, [netuid], block_hash=block_hash),
        substrate.query(*st.LastUpdate, [storage_index], block_hash=block_hash),
        substrate.query(*st.MinAllowedWeights, [netuid], block_hash=block_hash),
        substrate.query(*st.MaxWeightsLimit, [netuid], block_hash=block_hash),
        substrate.query(*st.SubnetEpochConsensus, [netuid], block_hash=block_hash),
        substrate.query(*st.MechanismCountCurrent, [netuid], block_hash=block_hash),
        substrate.query(*st.SubnetworkN, [netuid], block_hash=block_hash),
    )

    if uid_raw is None:
        raise ChainError(
            f"Hotkey {hotkey_ss58} is not registered on netuid {netuid}.",
            code=ErrorCode.NOT_REGISTERED,
        )
    uid = int(uid_raw)
    min_allowed_weights = int(min_raw or 0)
    max_row_len: Optional[int] = None
    if mode == "Null" or mode == {"Null": None} or mode == 1:
        permits = await substrate.query(*st.ValidatorPermit, [netuid], block_hash=block_hash)
        if not isinstance(permits, (list, tuple)) or uid >= len(permits) or not permits[uid]:
            raise ChainError(
                f"Only the sole Null validator permit holder may set weights on netuid {netuid}.",
                code=ErrorCode.NOT_AUTHORIZED,
            )
        # ValueQuery defaults to one mechanism; preserve the minimum-one cap for
        # any inconsistent zero value rather than dividing by zero client-side.
        mechanism_count = max(1, int(mechanism_count_raw or 1))
        row_cap = max(1, TOTAL_NULL_WEIGHT_BUDGET // mechanism_count)
        min_allowed_weights = min(min_allowed_weights, row_cap, int(subnet_n_raw or 0))
        max_row_len = row_cap

    rate_limit = int(rate_raw or 0)
    if (
        rate_limit
        and isinstance(last_raw, (list, tuple))
        and uid < len(last_raw)
        and int(last_raw[uid]) > 0
    ):
        since = current_block - int(last_raw[uid])
        if since < rate_limit:  # chain allows when since >= rate_limit
            remaining = rate_limit - since
            seconds = remaining * await substrate.block_time()
            raise ChainError(
                f"Weights on netuid {netuid} were last set {since} blocks ago; the "
                f"rate limit is {rate_limit} blocks. Retry in ~{remaining} blocks "
                f"(~{int(seconds)}s).",
                code=ErrorCode.RATE_LIMITED,
            )

    return _Preflight(
        uid=uid,
        commit_reveal=bool(cr_raw),
        min_allowed_weights=min_allowed_weights,
        max_weight_limit=int(max_raw) if max_raw is not None else U16_MAX,
        max_row_len=max_row_len,
    )


def _conform(
    uids: list[int], weights: list[float], preflight: _Preflight, netuid: int, raw_u16: bool = False
) -> tuple[list[int], list[int]]:
    """Fit the weights to the subnet's hyperparameters and quantize them.

    Applies the ``max_weight_limit`` clip, quantizes to u16 (dropping zeros), and
    enforces ``min_allowed_weights`` on what will actually be submitted. A single
    self-weight is exempt from both constraints, matching the chain's rules.
    On a Null subnet it also enforces the row cap before anything is signed or
    encrypted: the chain cannot check a timelocked commit until its reveal, and
    it skips a reveal whose row is above the cap.
    """
    is_self_weight = uids == [preflight.uid]
    if raw_u16:
        if len(uids) != len(weights) or any(u < 0 or u > U16_MAX for u in uids):
            raise BittensorError("Raw weights require equal-length lists and u16 UIDs.")
        if any(not isfinite(w) or w < 0 or w > U16_MAX or int(w) != w for w in weights):
            raise BittensorError("Raw weights must be integers in 0..65535.")
        pairs = [(uid, int(w)) for uid, w in zip(uids, weights) if w > 0]
        norm_uids = [uid for uid, _ in pairs]
        norm_vals = [value for _, value in pairs]
        if not norm_vals:
            raise BittensorError("All weights are zero; nothing to set.")
        if not is_self_weight and max(norm_vals) * U16_MAX > preflight.max_weight_limit * sum(
            norm_vals
        ):
            raise BittensorError(
                "Raw weights exceed max_weight_limit; change the integer ratios or subnet limit."
            )
    else:
        floats = list(weights)
        if not is_self_weight and preflight.max_weight_limit < U16_MAX:
            floats = clip_to_max_weight(floats, preflight.max_weight_limit / U16_MAX)

        norm_uids, norm_vals = normalize(uids, floats)
    if not norm_uids:
        raise BittensorError("All weights are zero; nothing to set.")
    if not is_self_weight and len(norm_uids) < preflight.min_allowed_weights:
        raise ChainError(
            f"Subnet {netuid} requires at least {preflight.min_allowed_weights} "
            f"weights; only {len(norm_uids)} are nonzero after normalization.",
            code=ErrorCode.INVALID_ARGUMENT,
        )
    if preflight.max_row_len is not None and len(norm_uids) > preflight.max_row_len:
        raise ChainError(
            f"Null subnet {netuid} accepts at most {preflight.max_row_len} weights in one "
            f"row at its current mechanism count (2500 // mechanism count, at least 1); "
            f"{len(norm_uids)} are nonzero after normalization.",
            name="TooManyWeightsForNullMechanism",
            code=ErrorCode.LIMIT_EXCEEDED,
        )
    return norm_uids, norm_vals


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
    for_hotkey: Optional[str] = None,
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
        _for_call(
            "commit_timelocked_mechanism_weights",
            netuid,
            mechid,
            for_hotkey,
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
    its owner and self-weight exceptions. On a Null subnet one row holds at most
    ``max(1, 2500 // mechanism_count)`` weights (2,500 with one mechanism, 156
    with sixteen); a longer row is refused before it is signed or encrypted.
    ``version_key`` must not be older than the subnet's
    required version. Prefer this over ``commit_weights``/``reveal_weights``
    unless you specifically need to force one path.
    """

    op = "set_weights"
    signer = "hotkey"
    wraps = (
        ("SubtensorModule", "set_mechanism_weights"),
        ("SubtensorModule", "commit_timelocked_mechanism_weights"),
        ("SubtensorModule", "set_mechanism_weights_for"),
        ("SubtensorModule", "commit_timelocked_mechanism_weights_for"),
    )

    netuid: int = field(metadata={"help": NETUID_HELP})
    uids: Optional[list[int]] = field(default=None, metadata={"help": UIDS_HELP})
    weights: Optional[list[float]] = field(default=None, metadata={"help": WEIGHTS_HELP})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    version_key: int = field(default=0, metadata={"help": VERSION_KEY_HELP})
    for_hotkey: Optional[str] = field(default=None, metadata={"help": FOR_HOTKEY_HELP})
    raw_u16: bool = field(
        default=False,
        metadata={
            "help": "Preserve exact u16 integer ratios on Null subnets; no scaling or clipping."
        },
    )

    def __post_init__(self):
        self.uids, self.weights = _as_pairs(self.uids, self.weights)

    async def build(self, substrate, wallet: Any):
        target_hotkey, target_public_key = _weight_keys(self, wallet, self.for_hotkey)
        if self.for_hotkey is not None:
            await _ensure_delegate(substrate, self.netuid, self.mechid, target_hotkey)
        preflight = await _preflight(substrate, target_hotkey, self.netuid, self.mechid)
        if self.raw_u16:
            await _ensure_raw_null(substrate, self.netuid)
        norm_uids, norm_vals = _conform(
            self.uids, self.weights, preflight, self.netuid, self.raw_u16
        )
        if preflight.commit_reveal:
            return await _build_timelocked(
                substrate,
                target_public_key,
                self.netuid,
                self.mechid,
                norm_uids,
                norm_vals,
                self.version_key,
                DEFAULT_COMMIT_REVEAL_VERSION,
                self.for_hotkey,
            )
        return await substrate.compose(
            _for_call(
                "set_mechanism_weights",
                self.netuid,
                self.mechid,
                self.for_hotkey,
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
        if self.for_hotkey is not None:
            return await _delegate_warning(
                substrate, self.netuid, self.mechid, self.for_hotkey, signer_address
            )
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
    wraps = (
        ("SubtensorModule", "commit_timelocked_mechanism_weights"),
        ("SubtensorModule", "commit_timelocked_mechanism_weights_for"),
    )

    netuid: int = field(metadata={"help": NETUID_HELP})
    uids: Optional[list[int]] = field(default=None, metadata={"help": UIDS_HELP})
    weights: Optional[list[float]] = field(default=None, metadata={"help": WEIGHTS_HELP})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    version_key: int = field(default=0, metadata={"help": VERSION_KEY_HELP})
    for_hotkey: Optional[str] = field(default=None, metadata={"help": FOR_HOTKEY_HELP})
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
        target_hotkey, target_public_key = _weight_keys(self, wallet, self.for_hotkey)
        if self.for_hotkey is not None:
            await _ensure_delegate(substrate, self.netuid, self.mechid, target_hotkey)
        preflight = await _preflight(substrate, target_hotkey, self.netuid, self.mechid)
        if self.raw_u16:
            await _ensure_raw_null(substrate, self.netuid)
        norm_uids, norm_vals = _conform(
            self.uids, self.weights, preflight, self.netuid, self.raw_u16
        )
        return await _build_timelocked(
            substrate,
            target_public_key,
            self.netuid,
            self.mechid,
            norm_uids,
            norm_vals,
            self.version_key,
            self.commit_reveal_version,
            self.for_hotkey,
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.for_hotkey is not None:
            return await _delegate_warning(
                substrate, self.netuid, self.mechid, self.for_hotkey, signer_address
            )
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
    wraps = (
        ("SubtensorModule", "reveal_weights"),
        ("SubtensorModule", "reveal_mechanism_weights"),
        ("SubtensorModule", "reveal_mechanism_weights_for"),
    )

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
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    for_hotkey: Optional[str] = field(default=None, metadata={"help": FOR_HOTKEY_HELP})

    async def build(self, substrate, wallet: Any):
        norm_uids, norm_vals = normalize(self.uids, self.weights)
        if not norm_uids:
            raise BittensorError("All weights are zero; nothing to reveal.")
        if self.for_hotkey is not None:
            await _ensure_delegate(substrate, self.netuid, self.mechid, self.for_hotkey)
        return await substrate.compose(
            _reveal_call(
                self.netuid,
                self.mechid,
                self.for_hotkey,
                uids=norm_uids,
                values=norm_vals,
                salt=[int(x) for x in self.salt],
                version_key=self.version_key,
            )
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.for_hotkey is None:
            return []
        return await _delegate_warning(
            substrate, self.netuid, self.mechid, self.for_hotkey, signer_address
        )

    def summary(self) -> str:
        return f"reveal weights on netuid {self.netuid} for {len(self.uids)} uids"


@register
@dataclass
class CommitWeightHash(Intent):
    """Submit a legacy salt-based weight hash for one subnet mechanism.

    Use this only with the legacy commit/reveal protocol, followed by
    ``RevealWeights`` with the same mechanism, uids, values, salt, and version
    key. Most validators should use ``CommitWeights`` for a timelocked commit
    or ``SetWeights`` to select the subnet's configured path automatically.
    With ``for_hotkey``, the signing hotkey must be that validator's authorized
    delegate on this mechanism; the delegate pays the normal transaction fee.
    """

    op = "commit_weight_hash"
    signer = "hotkey"
    wraps = (
        ("SubtensorModule", "commit_mechanism_weights"),
        ("SubtensorModule", "commit_mechanism_weights_for"),
    )

    netuid: int = field(metadata={"help": NETUID_HELP})
    commit_hash: str = field(metadata={"help": "Legacy weight commit hash (0x-prefixed H256)."})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    for_hotkey: Optional[str] = field(default=None, metadata={"help": FOR_HOTKEY_HELP})

    async def build(self, substrate, wallet: Any):
        if self.for_hotkey is not None:
            await _ensure_delegate(substrate, self.netuid, self.mechid, self.for_hotkey)
        return await substrate.compose(
            _for_call(
                "commit_mechanism_weights",
                self.netuid,
                self.mechid,
                self.for_hotkey,
                commit_hash=self.commit_hash,
            )
        )

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.for_hotkey is None:
            return []
        return await _delegate_warning(
            substrate, self.netuid, self.mechid, self.for_hotkey, signer_address
        )

    def summary(self) -> str:
        return f"commit weight hash on netuid {self.netuid}, mechanism {self.mechid}"


@register
@dataclass
class SetMechWeightDelegate(Intent):
    """Authorize or revoke one hotkey's weight delegate on one subnet mechanism.

    Signed by the registered validator hotkey. The delegate may submit direct,
    legacy commit/reveal, or timelocked weight calls *for* this hotkey and
    mechanism only, paying normal transaction fees itself. Authorization does
    not transfer stake or ownership and is independent on every mechanism.
    Pass ``delegate=None`` (or use the CLI's ``weights delegate revoke``) to
    revoke it. Existing delegated calls remain subject to the validator's
    registration, permit, stake, rate-limit, and weight rules.
    """

    op = "set_mech_weight_delegate"
    signer = "hotkey"
    verify = "mech_weight_delegate"
    wraps = (("SubtensorModule", "set_mech_weight_delegate"),)

    netuid: int = field(metadata={"help": "Subnet containing the validator hotkey."})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    delegate: Optional[str] = field(
        default=None,
        metadata={
            "help": "Account authorized to submit weights for this hotkey; null revokes. "
            "The runtime accepts any account, but delegated intents sign with a hotkey."
        },
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Call(
                "SubtensorModule",
                "set_mech_weight_delegate",
                {"netuid": self.netuid, "mecid": self.mechid, "delegate": self.delegate},
            )
        )

    def summary(self) -> str:
        action = f"authorize {self.delegate}" if self.delegate else "revoke weight delegate"
        return f"{action} for the signing hotkey on netuid {self.netuid}, mechanism {self.mechid}"
