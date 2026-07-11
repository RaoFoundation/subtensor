"""Timelock encryption: seal data that anyone can open at a known future time.

Built on the drand "quicknet" randomness beacon: encryption binds the data to a
future beacon round, and once that round's signature is published (every 3
seconds, worldwide, no keys or accounts involved) anyone can decrypt. Nobody --
including the author -- can open the data early. This is the same mechanism the
chain uses for commit-reveal weights; here it is exposed directly so subnet
owners can timelock anything: challenge answers, embargoed announcements,
sealed-bid auctions, delayed configuration.

The reveal moment is wall-clock time, not chain blocks: a round happens every
3 seconds regardless of block speed, so ``reveal_in="1h"`` means one hour on
any network (including fast-blocks local chains).

Quick start::

    from bittensor import timelock

    sealed = timelock.encrypt("the answer is 42", reveal_in="1h")
    sealed.reveal_at          # datetime of the reveal, UTC
    sealed.hex()              # portable ciphertext -- publish it anywhere

    # ... anyone, later ...
    timelock.decrypt(sealed_hex)             # raises TimelockNotReady if early
    timelock.decrypt(sealed_hex, wait=True)  # sleeps until reveal, then opens

Also accepts an absolute moment (``reveal_at="2026-07-08T12:00Z"``) or an
explicit beacon round (``reveal_round=30212846``). Encryption is pure local
cryptography; only decryption fetches the round signature from the drand HTTP
API. Round arithmetic is exact local math (genesis + 3s per round), so nothing
here polls the network to tell the time.
"""

from __future__ import annotations

import asyncio
import re
import struct
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from typing import Optional, Union

# Module import + attribute access (not `from bittensor_core import ...`):
# ty cannot see into the compiled extension, so named imports fail its check.
import bittensor_core as _core

from .result import BittensorError

# Drand quicknet chain parameters (chain hash
# 52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971): round 1 was
# published at the genesis timestamp and a new round appears every period.
QUICKNET_GENESIS = 1_692_803_367
QUICKNET_PERIOD = 3

# The ciphertext ends with this marker followed by the reveal round as a
# little-endian u64, so the round never has to be carried separately.
_ROUND_SUFFIX = b"AES_GCM_"

# After the reveal time passes, the beacon or HTTP relay may lag by a few
# seconds; decryption retries within this grace window before giving up.
_FETCH_GRACE_SECONDS = 30.0

WhenLike = Union[datetime, str, int, float]
DurationLike = Union[timedelta, str, int, float]


class TimelockError(BittensorError):
    """A timelock operation failed (malformed ciphertext, unreachable beacon)."""


class TimelockNotReady(TimelockError):
    """Decryption was attempted before the reveal round exists.

    Carries when the data unlocks so callers can schedule a retry:
    ``reveal_round``, ``reveal_at`` (UTC datetime), and ``remaining``
    (timedelta).
    """

    def __init__(self, reveal_round: int, reveal_at: datetime, remaining: timedelta):
        super().__init__(
            f"not decryptable yet: reveals at {reveal_at.isoformat(timespec='seconds')} "
            f"(round {reveal_round}, {format_duration(remaining)} from now)"
        )
        self.reveal_round = reveal_round
        self.reveal_at = reveal_at
        self.remaining = remaining


def format_duration(value: timedelta) -> str:
    """``2d 3h 5m 30s`` -- the same vocabulary ``reveal_in`` accepts."""
    seconds = int(value.total_seconds())
    if seconds <= 0:
        return "0s"
    parts = []
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60), ("s", 1)):
        amount, seconds = divmod(seconds, size)
        if amount:
            parts.append(f"{amount}{unit}")
    return " ".join(parts)


_DURATION_TOKEN = re.compile(r"(\d+(?:\.\d+)?)\s*([wdhms])", re.IGNORECASE)
_DURATION_UNITS = {"w": 604800.0, "d": 86400.0, "h": 3600.0, "m": 60.0, "s": 1.0}


def parse_duration(value: DurationLike) -> float:
    """A duration in seconds from ``"1h30m"``, ``"90s"``, ``90``, or a timedelta.

    Bare numbers (including numeric strings) are seconds. Units compose in any
    order: w(eeks), d(ays), h(ours), m(inutes), s(econds).
    """
    if isinstance(value, timedelta):
        return value.total_seconds()
    if isinstance(value, (int, float)):
        return float(value)
    text = value.strip()
    try:
        return float(text)
    except ValueError:
        pass
    tokens = _DURATION_TOKEN.findall(text)
    consumed = "".join(f"{n}{u}" for n, u in tokens)
    if not tokens or consumed.lower() != re.sub(r"\s+", "", text).lower():
        raise TimelockError(
            f"cannot parse duration {value!r}; use seconds or units like '90s', "
            "'15m', '1h30m', '2d'"
        )
    return sum(float(n) * _DURATION_UNITS[u.lower()] for n, u in tokens)


def _parse_when(value: WhenLike) -> float:
    """A unix timestamp from a datetime, ISO-8601 string, or numeric timestamp.

    Naive datetimes and ISO strings without an offset are taken as UTC -- the
    beacon is global, so local-time guessing would be a footgun.
    """
    if isinstance(value, datetime):
        if value.tzinfo is None:
            value = value.replace(tzinfo=timezone.utc)
        return value.timestamp()
    if isinstance(value, (int, float)):
        return float(value)
    text = value.strip()
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        raise TimelockError(
            f"cannot parse moment {value!r}; use ISO-8601 like '2026-07-08T12:00Z' "
            "(no offset means UTC) or a unix timestamp"
        ) from None
    return _parse_when(parsed)


def current_round(at: Optional[float] = None) -> int:
    """The latest published beacon round (computed locally, no network)."""
    now = time.time() if at is None else at
    return max(1, int((now - QUICKNET_GENESIS) // QUICKNET_PERIOD) + 1)


def round_at(when: WhenLike) -> int:
    """The first beacon round published at or after ``when``.

    Data encrypted to this round becomes decryptable at that moment (never
    before, at most 3 seconds after).
    """
    ts = _parse_when(when)
    elapsed = ts - QUICKNET_GENESIS
    if elapsed <= 0:
        return 1
    # Round r is published at genesis + (r - 1) * period.
    return -(-int(elapsed * 1000) // (QUICKNET_PERIOD * 1000)) + 1


def reveal_time(reveal_round: int) -> datetime:
    """The UTC moment a beacon round is published (when decryption unlocks)."""
    ts = QUICKNET_GENESIS + (reveal_round - 1) * QUICKNET_PERIOD
    return datetime.fromtimestamp(ts, tz=timezone.utc)


@dataclass(frozen=True)
class Timelocked:
    """A sealed payload: the ciphertext plus the round that opens it.

    The round is also embedded in the ciphertext itself, so only the bytes (or
    their hex) need to be published -- ``Timelocked.parse`` recovers everything.
    """

    ciphertext: bytes
    reveal_round: int

    @classmethod
    def parse(cls, data: Union["Timelocked", bytes, str]) -> "Timelocked":
        """Rehydrate from raw ciphertext bytes or their hex string."""
        if isinstance(data, Timelocked):
            return data
        if isinstance(data, str):
            text = data.strip().removeprefix("0x")
            try:
                data = bytes.fromhex(text)
            except ValueError:
                raise TimelockError(
                    "expected timelock ciphertext as raw bytes or a hex string"
                ) from None
        if len(data) < len(_ROUND_SUFFIX) + 8 or data[-16:-8] != _ROUND_SUFFIX:
            raise TimelockError("not a timelock ciphertext (missing the reveal-round trailer)")
        return cls(ciphertext=data, reveal_round=struct.unpack("<Q", data[-8:])[0])

    @property
    def reveal_at(self) -> datetime:
        """UTC moment the payload becomes decryptable."""
        return reveal_time(self.reveal_round)

    @property
    def remaining(self) -> timedelta:
        """Time until the reveal round; zero once it has passed."""
        seconds = self.reveal_at.timestamp() - time.time()
        return timedelta(seconds=max(0.0, seconds))

    @property
    def revealed(self) -> bool:
        """Whether the reveal round has been published (decryption will work)."""
        return current_round() >= self.reveal_round

    def hex(self) -> str:
        """Portable hex form of the ciphertext (what you publish or store)."""
        return self.ciphertext.hex()

    def __bytes__(self) -> bytes:
        return self.ciphertext

    def decrypt(self, *, wait: bool = False, timeout: Optional[float] = None) -> bytes:
        """Open the payload; see :func:`decrypt`."""
        return decrypt(self, wait=wait, timeout=timeout)

    def __repr__(self) -> str:
        state = "revealed" if self.revealed else f"reveals in {format_duration(self.remaining)}"
        return (
            f"Timelocked({len(self.ciphertext)} bytes, round {self.reveal_round}, "
            f"{self.reveal_at.isoformat(timespec='seconds')}, {state})"
        )


def encrypt(
    data: Union[str, bytes],
    reveal_in: Optional[DurationLike] = None,
    *,
    reveal_at: Optional[WhenLike] = None,
    reveal_round: Optional[int] = None,
) -> Timelocked:
    """Seal data so it can only be decrypted at a chosen future moment.

    Give the moment exactly one way:

    - ``reveal_in`` -- a duration from now: ``"30s"``, ``"15m"``, ``"1h30m"``,
      ``"2d"``, plain seconds, or a timedelta.
    - ``reveal_at`` -- an absolute moment: a datetime or ISO-8601 string
      (``"2026-07-08T12:00Z"``; no offset means UTC).
    - ``reveal_round`` -- an explicit drand round number.

    Strings are encoded UTF-8. Purely local -- no network, no keys, nothing to
    keep secret afterwards except the plaintext itself.
    """
    given = [x is not None for x in (reveal_in, reveal_at, reveal_round)]
    if sum(given) != 1:
        raise TimelockError(
            "give the reveal moment exactly one way: reveal_in ('15m'), "
            "reveal_at ('2026-07-08T12:00Z'), or reveal_round"
        )
    if reveal_in is not None:
        seconds = parse_duration(reveal_in)
        if seconds <= 0:
            raise TimelockError("reveal_in must be a positive duration")
        target_round = round_at(time.time() + seconds)
    elif reveal_at is not None:
        target_round = round_at(reveal_at)
    else:
        target_round = int(reveal_round)  # type: ignore[arg-type]

    latest = current_round()
    if target_round <= latest:
        raise TimelockError(
            f"reveal moment is not in the future (round {target_round}, but the "
            f"beacon is already at round {latest})"
        )

    payload = data.encode() if isinstance(data, str) else bytes(data)
    ciphertext, actual_round = _core.encrypt_at_round(payload, target_round)
    return Timelocked(ciphertext=ciphertext, reveal_round=actual_round)


def decrypt(
    data: Union[Timelocked, bytes, str],
    *,
    wait: bool = False,
    timeout: Optional[float] = None,
) -> bytes:
    """Open a timelocked payload once its reveal round has been published.

    Accepts a :class:`Timelocked`, raw ciphertext bytes, or their hex string.
    Fetches the round signature from the drand HTTP API -- the one network
    call in this module.

    If the reveal moment has not arrived: raises :class:`TimelockNotReady`
    (which says when to come back), or with ``wait=True`` sleeps until the
    exact reveal time and then opens it. ``timeout`` caps the total wait in
    seconds.
    """
    sealed = Timelocked.parse(data)
    deadline = None if timeout is None else time.monotonic() + timeout

    remaining = sealed.reveal_at.timestamp() - time.time()
    if remaining > 0:
        if not wait:
            raise TimelockNotReady(
                sealed.reveal_round, sealed.reveal_at, timedelta(seconds=remaining)
            )
        if deadline is not None and time.monotonic() + remaining > deadline:
            raise TimelockNotReady(
                sealed.reveal_round, sealed.reveal_at, timedelta(seconds=remaining)
            )
        time.sleep(remaining)

    # The round has (nominally) been published; tolerate beacon/relay lag.
    grace = time.monotonic() + _FETCH_GRACE_SECONDS
    if deadline is not None:
        grace = min(grace, deadline)
    last_error: Optional[Exception] = None
    while True:
        try:
            result = _core.decrypt(sealed.ciphertext, False)
            if result is not None:
                return result
        except Exception as error:  # the rust binding raises plain ValueError
            last_error = error
        if time.monotonic() >= grace:
            break
        time.sleep(1.0)
    detail = f": {last_error}" if last_error else ""
    raise TimelockError(
        f"failed to decrypt round {sealed.reveal_round} (beacon unreachable or "
        f"ciphertext corrupt){detail}"
    )


async def decrypt_async(
    data: Union[Timelocked, bytes, str],
    *,
    wait: bool = True,
    timeout: Optional[float] = None,
) -> bytes:
    """Async :func:`decrypt`: waits on the event loop, fetches in a thread."""
    sealed = Timelocked.parse(data)
    remaining = sealed.reveal_at.timestamp() - time.time()
    if remaining > 0:
        if not wait or (timeout is not None and remaining > timeout):
            raise TimelockNotReady(
                sealed.reveal_round, sealed.reveal_at, timedelta(seconds=remaining)
            )
        await asyncio.sleep(remaining)
    fetch_timeout = None if timeout is None else max(1.0, timeout - max(0.0, remaining))
    return await asyncio.to_thread(decrypt, sealed, wait=True, timeout=fetch_timeout)


__all__ = [
    "TimelockError",
    "TimelockNotReady",
    "Timelocked",
    "current_round",
    "decrypt",
    "decrypt_async",
    "encrypt",
    "format_duration",
    "parse_duration",
    "reveal_time",
    "round_at",
]
