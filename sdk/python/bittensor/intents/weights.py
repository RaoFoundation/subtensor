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
from typing import Any, Mapping, Optional, Sequence

# Module import + attribute access (not `from bittensor_core import ...`):
# ty cannot see into the compiled extension, so named imports fail its check.
import bittensor_core as _core

from .._generated import calls
from .._generated.storage import SubtensorModule as st
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
    if len(uids) != len(weights):
        raise BittensorError(
            f"uids and weights must be equal length: {len(uids)} vs {len(weights)}."
        )
    if any(w < 0 for w in weights):
        raise BittensorError("Weights must be non-negative.")
    if any(u < 0 for u in uids):
        raise BittensorError("UIDs must be non-negative.")

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


async def _preflight(substrate, hotkey_ss58: str, netuid: int, mechid: int) -> _Preflight:
    """Check registration and the rate limit before signing anything.

    Raises a :class:`ChainError` carrying the same :class:`ErrorCode` the on-chain
    failure would map to, so callers branch identically whether the problem was
    caught client-side or on-chain. Other chain-side checks (minimum stake,
    validator permit, version key) are not preflighted here.
    """
    current_block = await substrate.block_number()
    block_hash = await substrate.block_hash(current_block)
    # LastUpdate (like the weights themselves) is keyed by the mechanism storage
    # index; for mechid 0 that index equals the netuid.
    storage_index = mechid * GLOBAL_MAX_SUBNET_COUNT + netuid
    uid_raw, cr_raw, rate_raw, last_raw, min_raw, max_raw = await asyncio.gather(
        substrate.query(*st.Uids, [netuid, hotkey_ss58], block_hash=block_hash),
        substrate.query(*st.CommitRevealWeightsEnabled, [netuid], block_hash=block_hash),
        substrate.query(*st.WeightsSetRateLimit, [netuid], block_hash=block_hash),
        substrate.query(*st.LastUpdate, [storage_index], block_hash=block_hash),
        substrate.query(*st.MinAllowedWeights, [netuid], block_hash=block_hash),
        substrate.query(*st.MaxWeightsLimit, [netuid], block_hash=block_hash),
    )

    if uid_raw is None:
        raise ChainError(
            f"Hotkey {hotkey_ss58} is not registered on netuid {netuid}.",
            code=ErrorCode.NOT_REGISTERED,
        )
    uid = int(uid_raw)

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
        min_allowed_weights=int(min_raw or 0),
        max_weight_limit=int(max_raw) if max_raw is not None else U16_MAX,
    )


def _conform(
    uids: list[int], weights: list[float], preflight: _Preflight, netuid: int
) -> tuple[list[int], list[int]]:
    """Fit the weights to the subnet's hyperparameters and quantize them.

    Applies the ``max_weight_limit`` clip, quantizes to u16 (dropping zeros), and
    enforces ``min_allowed_weights`` on what will actually be submitted. A single
    self-weight is exempt from both constraints, matching the chain's rules.
    """
    is_self_weight = uids == [preflight.uid]
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
    return norm_uids, norm_vals


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
    weights, must hold a validator permit to set non-self weights (the subnet
    owner is exempt), and ``version_key`` must not be older than the subnet's
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

    def __post_init__(self):
        self.uids, self.weights = _as_pairs(self.uids, self.weights)

    async def build(self, substrate, wallet: Any):
        preflight = await _preflight(
            substrate, self.hotkey_address(wallet), self.netuid, self.mechid
        )
        norm_uids, norm_vals = _conform(self.uids, self.weights, preflight, self.netuid)
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
        warning = await _would_clip(substrate, self.netuid, self.weights)
        return [warning] if warning else []

    def summary(self) -> str:
        return f"set weights on netuid {self.netuid} for {len(self.uids)} uids"


async def _root_weights_preflight(substrate, hotkey_ss58: str) -> None:
    """Check root registration and the root weights rate limit before signing.

    The root path has no commit-reveal and no max-weight/min-count
    hyperparameters, so only the two checks that would reject an otherwise
    well-formed submission are preflighted; stake and version-key checks stay
    chain-side.
    """
    current_block = await substrate.block_number()
    block_hash = await substrate.block_hash(current_block)
    uid_raw, rate_raw, last_raw = await asyncio.gather(
        substrate.query(*st.Uids, [0, hotkey_ss58], block_hash=block_hash),
        substrate.query(*st.WeightsSetRateLimit, [0], block_hash=block_hash),
        substrate.query(*st.LastUpdate, [0], block_hash=block_hash),
    )
    if uid_raw is None:
        raise ChainError(
            f"Hotkey {hotkey_ss58} is not registered on the root network (netuid 0).",
            code=ErrorCode.NOT_REGISTERED,
        )
    uid = int(uid_raw)

    rate_limit = int(rate_raw or 0)
    if (
        rate_limit
        and isinstance(last_raw, (list, tuple))
        and uid < len(last_raw)
        and int(last_raw[uid]) > 0
    ):
        since = current_block - int(last_raw[uid])
        if since < rate_limit:
            remaining = rate_limit - since
            seconds = remaining * await substrate.block_time()
            raise ChainError(
                f"Root weights were last set {since} blocks ago; the rate limit is "
                f"{rate_limit} blocks. Retry in ~{remaining} blocks (~{int(seconds)}s).",
                code=ErrorCode.RATE_LIMITED,
            )


@register
@dataclass
class SetRootWeights(Intent):
    """Set how your root dividends are deployed across subnets (basket weights).

    Declares the root validator's dividend distribution vector: each epoch
    the chain sells the validator's root alpha dividend for TAO and splits it
    across the listed subnets in proportion to these weights, buying each
    subnet's alpha into the validator's basket — the escrowed
    per-validator index fund that root stakers redeem with
    ``claim_root_with_hotkey`` (or coldkey-wide ``claim_root``).
    Netuid 0 is a valid destination: that share is held as TAO (root stake)
    instead of subnet alpha, letting a validator keep part of the basket out
    of subnet exposure. Weights are relative; they are max-upscaled and
    quantized to u16 before submission, and zero weights are dropped. Signed
    by the hotkey, which must be registered on the root network and hold the
    minimum stake to set weights; the root weights rate limit applies, and
    every destination must be netuid 0 or an existing subnet. At least 8
    positive destinations are required (softened when fewer networks exist).
    Validators with no stored root weights accumulate dividends in place —
    each subnet's dividend stays in that subnet's alpha, trade-free; setting
    this vector is what turns on the sell-and-redeploy engine.
    Weight setting launched gated off network-wide (every call fails with
    ``RootWeightSettingDisabled`` until governance or a later upgrade enables
    it), so all funds start on the null accumulate strategy.
    Read it back with the ``validator_root_weights`` read.
    """

    op = "set_root_weights"
    signer = "hotkey"
    wraps = (("SubtensorModule", "set_root_weights"),)

    netuids: Optional[list[int]] = field(
        default=None,
        metadata={
            "help": "Destination subnets, as a list parallel to weights (0 = hold as "
            "TAO / root stake). Omit when weights is given as a netuid-to-weight mapping."
        },
    )
    weights: Optional[list[float]] = field(
        default=None,
        metadata={
            "help": "Relative weight per destination subnet: either a JSON object "
            "mapping netuid to weight, or a list parallel to netuids. Values are "
            "normalized and quantized before submission."
        },
    )

    def __post_init__(self):
        self.netuids, self.weights = _as_pairs(self.netuids, self.weights)

    async def build(self, substrate, wallet: Any):
        await _root_weights_preflight(substrate, self.hotkey_address(wallet))
        dests, values = normalize(self.netuids, self.weights)
        if not dests:
            raise BittensorError("All weights are zero; nothing to set.")
        return await substrate.compose(
            calls.SubtensorModule.set_root_weights(
                dests=dests,
                weights=values,
            )
        )

    def summary(self) -> str:
        return f"set root dividend weights across {len(self.netuids)} destinations"

    def touches_netuids(self) -> list[int]:
        return list(self.netuids or [])


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
        norm_uids, norm_vals = _conform(self.uids, self.weights, preflight, self.netuid)
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
