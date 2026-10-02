"""Public challenge mining for fee-free subnet registration."""

from __future__ import annotations

import asyncio
import hashlib
import os
import secrets
import time
from concurrent.futures import ThreadPoolExecutor

import bittensor_core
from eth_utils import keccak

from ._generated import storage as st
from ._transport.codec import ss58_decode

DOMAIN = b"subtensor-pow-register-v1"
MAX_WORK_AGE_BLOCKS = 5
CHALLENGE_REFRESH_SECONDS = 12


def registration_prefix(netuid: int, block_hash: str, hotkey: str, coldkey: str) -> bytes:
    """Match the runtime's challenge bytes, without the trailing nonce."""
    if not 0 < netuid < 65536:
        raise ValueError("PoW registration requires a non-root u16 netuid")
    block = bytes.fromhex(block_hash.removeprefix("0x"))
    hot = bytes.fromhex(ss58_decode(hotkey))
    cold = bytes.fromhex(ss58_decode(coldkey))
    if any(len(value) != 32 for value in (block, hot, cold)):
        raise ValueError("PoW requires a 32-byte block hash and AccountId32 addresses")
    return DOMAIN + netuid.to_bytes(2, "little") + block + hot + cold


def registration_seal(prefix: bytes, nonce: int) -> bytes:
    return keccak(hashlib.sha256(prefix + nonce.to_bytes(8, "little")).digest())


def effective_difficulty(value, minimum, maximum) -> int:
    """Match chain bounds; an owner ceiling cannot bypass the team's floor."""
    minimum = max(1, int(minimum or 1))
    maximum = max(minimum, int(maximum if maximum is not None else (1 << 64) - 1))
    return min(max(minimum, int(value or 1)), maximum)


def _mine_chunk(prefix: bytes, difficulty: int, start: int, attempts: int):
    native = getattr(bittensor_core, "pow_mine_cpu", None)
    if native is not None and 0 < difficulty < 1 << 64:
        result = native(prefix, difficulty, start, attempts)
        return None if result is None else (result[0], bytes(result[1]).hex())
    # Compatibility for previously published core wheels. Updated wheels use
    # Rust above; explicit GPU mode requires the new native API.
    target = ((1 << 256) - 1) // max(1, difficulty)
    for offset in range(attempts):
        nonce = (start + offset) % (1 << 64)
        work = registration_seal(prefix, nonce)
        if int.from_bytes(work, "little") <= target:
            return nonce, work.hex()
    return None


def _create_gpu_miner(backend: str, device_ids: tuple[int, ...] | None):
    if backend == "cpu":
        return None
    discover = getattr(bittensor_core, "pow_gpu_devices", None)
    native = getattr(bittensor_core, "PowGpuMiner", None)
    if discover is None or native is None:
        if backend == "gpu" or device_ids is not None:
            raise RuntimeError("GPU mining requires an upgraded bittensor-core wheel")
        return None
    if not discover():
        if backend == "gpu" or device_ids is not None:
            raise RuntimeError("No usable GPU found; install the GPU vendor's OpenCL driver")
        return None
    # A discovered GPU that fails its startup hash self-tests must not be
    # silently ignored, even in automatic mode.
    return native(None if device_ids is None else list(device_ids))


def _mine_gpu_chunk(miner, prefix: bytes, difficulty: int, start: int, attempts: int):
    result = miner.mine(prefix, difficulty, start, attempts)
    return None if result is None else (result[0], bytes(result[1]).hex())


async def mine_registration(
    substrate,
    netuid: int,
    hotkey: str,
    coldkey: str,
    *,
    workers: int = 4,
    max_seconds: float = 300,
    backend: str = "auto",
    device_ids: tuple[int, ...] | None = None,
):
    """Return a PowRegister intent, refreshing stale challenges automatically.

    Only public addresses and hashes enter workers. Signing remains in the
    client's normal executor. Submit the returned intent promptly; a proof is
    accepted for five chain blocks. Transaction-pool validation initializes
    the next block before checking it. Long searches refresh the published
    head challenge every twelve seconds, without polling after every GPU batch.
    Automatic mode uses all discovered GPUs, otherwise native CPU workers.
    Select GPU explicitly to require hardware, or CPU to avoid GPU discovery.
    """
    from .intents.registration import PowRegister

    if not 1 <= workers <= 32 or not 0 < max_seconds <= 3600:
        raise ValueError("workers must be 1..32 and max_seconds must be 0..3600")
    if backend not in {"auto", "gpu", "cpu"}:
        raise ValueError("backend must be auto, gpu, or cpu")
    if device_ids is not None:
        device_ids = tuple(device_ids)
        if (
            backend == "cpu"
            or not device_ids
            or len(device_ids) > 32
            or any(index < 0 for index in device_ids)
            or len(set(device_ids)) != len(device_ids)
        ):
            raise ValueError("select 1..32 unique nonnegative GPU device ids with auto or gpu")
    deadline = time.monotonic() + max_seconds
    loop = asyncio.get_running_loop()
    pool = ThreadPoolExecutor(max_workers=min(workers, os.cpu_count() or 1))
    try:
        gpu = await loop.run_in_executor(pool, _create_gpu_miner, backend, device_ids)
        while time.monotonic() < deadline:
            head = await substrate.block_number()
            if head < 1:
                await asyncio.sleep(1)
                continue
            refresh_at = time.monotonic() + CHALLENGE_REFRESH_SECONDS
            anchor = await substrate.block_hash(head)
            (
                pow_allowed,
                difficulty,
                minimum,
                maximum,
                uid,
                last,
                registrations,
                limit,
            ) = await asyncio.gather(
                substrate.query(
                    *st.SubtensorModule.NetworkPowRegistrationAllowed, [netuid], block_hash=anchor
                ),
                substrate.query(*st.SubtensorModule.Difficulty, [netuid], block_hash=anchor),
                substrate.query(*st.SubtensorModule.MinDifficulty, [netuid], block_hash=anchor),
                substrate.query(*st.SubtensorModule.MaxDifficulty, [netuid], block_hash=anchor),
                substrate.query(*st.SubtensorModule.Uids, [netuid, hotkey], block_hash=anchor),
                substrate.query(
                    *st.SubtensorModule.LastPowRegistrationBlock, [hotkey], block_hash=anchor
                ),
                substrate.query(
                    *st.SubtensorModule.RegistrationsThisBlock, [netuid], block_hash=anchor
                ),
                substrate.query(
                    *st.SubtensorModule.MaxRegistrationsPerBlock, [netuid], block_hash=anchor
                ),
            )
            if not pow_allowed:
                raise ValueError("The subnet owner must enable PoW registration")
            if uid is not None:
                raise ValueError("The hotkey is already registered on this subnet")
            if not limit:
                raise ValueError("The subnet's per-block registration limit must be positive")
            if int(registrations or 0) >= int(limit):
                await asyncio.sleep(1)
                continue
            difficulty = effective_difficulty(difficulty, minimum, maximum)
            # The pool initializes System at head + 1 before validating. Mining
            # the published head keeps the full allowed freshness window.
            work_block = head
            if last is not None and work_block <= int(last):
                await asyncio.sleep(1)
                continue
            block_hash = anchor
            prefix = registration_prefix(netuid, block_hash, hotkey, coldkey)
            while time.monotonic() < min(deadline, refresh_at):
                start = secrets.randbits(64)
                if gpu is not None:
                    result = await loop.run_in_executor(
                        pool, _mine_gpu_chunk, gpu, prefix, difficulty, start, 100_000
                    )
                    results = [result]
                else:
                    futures = [
                        loop.run_in_executor(
                            pool,
                            _mine_chunk,
                            prefix,
                            difficulty,
                            (start + index * 100_000) % (1 << 64),
                            100_000,
                        )
                        for index in range(min(workers, os.cpu_count() or 1))
                    ]
                    results = await asyncio.gather(*futures)
                if time.monotonic() >= deadline:
                    raise TimeoutError(
                        "No fresh registration proof found within the mining time limit"
                    )
                if not any(result is not None for result in results):
                    await asyncio.sleep(0)
                    continue
                # Check the live chain only when a solution exists. Empty
                # batches keep hashing until the periodic challenge refresh.
                current = await substrate.block_number()
                if (
                    current < work_block
                    or current - work_block >= MAX_WORK_AGE_BLOCKS
                    or await substrate.block_hash(work_block) != block_hash
                ):
                    break
                for result in results:
                    if result is not None:
                        nonce, work_hex = result
                        if not 0 <= nonce < 1 << 64 or bytes.fromhex(work_hex) != registration_seal(
                            prefix, nonce
                        ):
                            raise RuntimeError("Miner returned an invalid registration proof")
                        latest = await substrate.block_hash(current)
                        bounds = await asyncio.gather(
                            *(
                                substrate.query(*item, [netuid], block_hash=latest)
                                for item in (
                                    st.SubtensorModule.Difficulty,
                                    st.SubtensorModule.MinDifficulty,
                                    st.SubtensorModule.MaxDifficulty,
                                )
                            )
                        )
                        difficulty = effective_difficulty(*bounds)
                        target = ((1 << 256) - 1) // difficulty
                        if int.from_bytes(bytes.fromhex(work_hex), "little") > target:
                            break
                        if time.monotonic() >= deadline:
                            raise TimeoutError(
                                "No fresh registration proof found within the mining time limit"
                            )
                        return PowRegister(
                            netuid=netuid,
                            work_block=work_block,
                            nonce=nonce,
                            work_hex=work_hex,
                            hotkey_ss58=hotkey,
                            mining_workers=workers,
                            mining_timeout=max_seconds,
                            mining_backend=backend,
                            mining_device_ids=None if device_ids is None else list(device_ids),
                        )
                await asyncio.sleep(0)
        raise TimeoutError("No fresh registration proof found within the mining time limit")
    finally:
        pool.shutdown(wait=False, cancel_futures=True)
