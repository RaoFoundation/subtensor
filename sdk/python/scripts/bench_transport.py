"""Transport benchmark: where does SDK wall-clock time actually go?

Separates every phase into network time (RPC round-trip, measured at
``RpcSession.request``) and CPU time (SCALE decode / signing, measured around
the codec calls) so we can judge whether a Rust rewrite of the transport would
pay.

Usage (venv active):

    python scripts/bench_transport.py [ws-endpoint] [--netuids 1,4,64] [--pages 5]

Defaults to finney.
"""

from __future__ import annotations

import argparse
import asyncio
import os
import statistics
import tempfile
import time
from dataclasses import dataclass, field

from bittensor._transport import SubstrateConnection
from bittensor._transport.codec import RuntimeCodec, ss58_encode
from bittensor._transport.runtime_api import (
    decode_runtime_api_result,
    encode_runtime_api_params,
)
from bittensor._transport.storage import decode_map_pairs, storage_key
from bittensor.settings import SS58_FORMAT

DEFAULT_ENDPOINT = "wss://entrypoint-finney.opentensor.ai:443"


@dataclass
class Section:
    title: str
    rows: list[tuple[str, str]] = field(default_factory=list)

    def add(self, label: str, value: str) -> None:
        self.rows.append((label, value))


SECTIONS: list[Section] = []


def section(title: str) -> Section:
    s = Section(title)
    SECTIONS.append(s)
    return s


def ms(seconds: float) -> str:
    return f"{seconds * 1000:.1f} ms"


def mb(n_bytes: int) -> str:
    return f"{n_bytes / 1e6:.2f} MB"


class Timer:
    def __enter__(self):
        self.t0 = time.perf_counter()
        return self

    def __exit__(self, *_):
        self.elapsed = time.perf_counter() - self.t0


async def timed(awaitable):
    t0 = time.perf_counter()
    result = await awaitable
    return result, time.perf_counter() - t0


def cpu_loop(fn, *, min_repeat: int = 3) -> float:
    """Median seconds per call of ``fn`` over a few repetitions."""
    times = []
    for _ in range(min_repeat):
        with Timer() as t:
            fn()
        times.append(t.elapsed)
    return statistics.median(times)


# --------------------------------------------------------------------------- connect


async def bench_connect(endpoint: str) -> SubstrateConnection:
    s = section("Connect (websocket + metadata + codec build)")
    # Fresh cache dir => cold start downloads metadata; second run is warm.
    cache_dir = tempfile.mkdtemp(prefix="bench-md-cache-")
    os.environ["BITTENSOR_RUNTIME_CACHE_DIR"] = cache_dir

    cold = SubstrateConnection(endpoint, ss58_format=SS58_FORMAT)
    _, t_session = await timed(cold._session.connect())
    _, t_codec = await timed(cold._runtimes.codec_at(None))
    s.add("cold: websocket connect", ms(t_session))
    s.add("cold: metadata download + codec build", ms(t_codec))

    codec = await cold._runtimes.codec_at(None)
    metadata_bytes = codec.metadata_bytes
    s.add("metadata blob size", mb(len(metadata_bytes)))

    # Pure-CPU codec build from bytes already on disk/memory (what a warm
    # start pays after reading the cache file).
    t_build = cpu_loop(
        lambda: RuntimeCodec(
            metadata_bytes,
            spec_version=codec.spec_version,
            transaction_version=codec.transaction_version,
            ss58_format=SS58_FORMAT,
        )
    )
    s.add("codec build from metadata bytes (pure CPU)", ms(t_build))
    await cold.close()

    warm = SubstrateConnection(endpoint, ss58_format=SS58_FORMAT)
    _, t_warm = await timed(warm.initialize())
    s.add("warm connect total (disk-cached metadata)", ms(t_warm))
    return warm


# --------------------------------------------------------------------------- RTT


async def bench_rtt(conn: SubstrateConnection) -> float:
    s = section("Round-trip baseline (chain_getHeader x10, sequential)")
    times = []
    for _ in range(10):
        _, t = await timed(conn._session.request("chain_getHeader", [None]))
        times.append(t)
    median = statistics.median(times)
    s.add("median round trip", ms(median))
    s.add("min / max", f"{ms(min(times))} / {ms(max(times))}")
    return median


# --------------------------------------------------------------------------- single query


async def bench_single_query(conn: SubstrateConnection) -> None:
    s = section("Single storage query (System.Account)")
    codec = await conn._runtimes.codec_at(None)
    entry = codec.storage_entry("System", "Account")

    from bittensor._transport.storage import decode_storage_value, storage_key

    address = ss58_encode(bytes(range(32)), SS58_FORMAT)
    with Timer() as t_key:
        key = storage_key(codec, entry, [address])
    raw, t_rpc = await timed(conn._session.request("state_getStorageAt", ["0x" + key.hex(), None]))
    raw_bytes = bytes.fromhex(raw[2:]) if raw is not None else None
    with Timer() as t_decode:
        decode_storage_value(codec, entry, raw_bytes)
    s.add("build storage key (CPU)", ms(t_key.elapsed))
    s.add("state_getStorageAt (network)", ms(t_rpc))
    s.add("decode value (CPU)", ms(t_decode.elapsed))


# --------------------------------------------------------------------------- metagraph


async def bench_metagraph(conn: SubstrateConnection, netuids: list[int]) -> None:
    codec = await conn._runtimes.codec_at(None)
    api, method = "SubnetInfoRuntimeApi", "get_metagraph"
    for netuid in netuids:
        s = section(f"Metagraph runtime call (netuid {netuid})")
        with Timer() as t_enc:
            params_hex = encode_runtime_api_params(codec, api, method, [netuid])
        result, t_rpc = await timed(
            conn._session.request("state_call", [f"{api}_{method}", params_hex, None])
        )
        raw = bytes.fromhex(result.removeprefix("0x"))
        with Timer() as t_hex:
            bytes.fromhex(result.removeprefix("0x"))
        t_decode = cpu_loop(lambda raw=raw: decode_runtime_api_result(codec, api, method, raw))
        total = t_enc.elapsed + t_rpc + t_decode
        s.add("payload size", mb(len(raw)))
        s.add("state_call (network incl. download)", ms(t_rpc))
        s.add("hex -> bytes (CPU)", ms(t_hex.elapsed))
        s.add("SCALE decode (CPU, median of 3)", ms(t_decode))
        throughput = len(raw) / t_decode / 1e6 if t_decode else 0.0
        s.add("decode share of total", f"{100 * t_decode / total:.0f}%  ({throughput:.1f} MB/s)")


# --------------------------------------------------------------------------- query_map


async def bench_query_map(conn: SubstrateConnection, pages: int, page_size: int) -> None:
    s = section(f"query_map System.Account ({pages} pages x {page_size} keys, pinned block)")
    codec = await conn._runtimes.codec_at(None)
    entry = codec.storage_entry("System", "Account")
    prefix_hex = "0x" + storage_key(codec, entry, []).hex()
    block_hash = await conn._session.request("chain_getFinalizedHead", [])

    t_keys_total = t_values_total = t_decode_total = 0.0
    n_pairs = 0
    n_value_bytes = 0
    start_key = None
    for _ in range(pages):
        keys, t_keys = await timed(
            conn._session.request(
                "state_getKeysPaged", [prefix_hex, page_size, start_key or prefix_hex, block_hash]
            )
        )
        if not keys:
            break
        response, t_values = await timed(
            conn._session.request("state_queryStorageAt", [keys, block_hash])
        )
        changes: list[tuple[str, str | None]] = []
        for group in response or []:
            changes.extend(group["changes"])
        with Timer() as t_dec:
            pairs = decode_map_pairs(codec, entry, [], changes)
        t_keys_total += t_keys
        t_values_total += t_values
        t_decode_total += t_dec.elapsed
        n_pairs += len(pairs)
        n_value_bytes += sum(len(v) // 2 - 1 for _, v in changes if v)
        start_key = keys[-1]

    total = t_keys_total + t_values_total + t_decode_total
    s.add("entries fetched", f"{n_pairs} ({mb(n_value_bytes)} of values)")
    s.add("state_getKeysPaged (network)", ms(t_keys_total))
    s.add("state_queryStorageAt (network)", ms(t_values_total))
    s.add("decode keys+values (CPU)", ms(t_decode_total))
    s.add("decode share of total", f"{100 * t_decode_total / total:.0f}%")
    if t_decode_total:
        s.add("decode throughput", f"{n_pairs / t_decode_total:.0f} entries/s")


# --------------------------------------------------------------------------- signing


async def bench_signing(conn: SubstrateConnection) -> None:
    s = section("Local extrinsic path (compose + payload + sr25519 sign + assemble)")
    import bittensor_core

    kp = bittensor_core.Keypair.create_from_uri("//Alice")
    codec = await conn._runtimes.codec_at(None)
    dest = ss58_encode(bytes(range(32)), SS58_FORMAT)

    with Timer() as t_compose:
        call = codec.compose_call("Balances", "transfer_keep_alive", {"dest": dest, "value": 10**9})
    # Warm up genesis-hash / era caches so the loop below is pure local work.
    await conn.sign_without_nonce_tracking(call, kp, nonce=0, era="00")

    n = 50
    t0 = time.perf_counter()
    for _ in range(n):
        await conn.sign_without_nonce_tracking(call, kp, nonce=0, era="00")
    per_op = (time.perf_counter() - t0) / n
    s.add("compose_call (CPU)", ms(t_compose.elapsed))
    s.add("sign+assemble per extrinsic (CPU)", ms(per_op))
    s.add("throughput", f"{1 / per_op:.0f} extrinsics/s")


# --------------------------------------------------------------------------- ss58


def bench_ss58() -> None:
    s = section("ss58_encode (10k addresses)")
    keys = [i.to_bytes(4, "little") + bytes(28) for i in range(10_000)]
    with Timer() as t:
        for k in keys:
            ss58_encode(k, SS58_FORMAT)
    s.add("total / per address", f"{ms(t.elapsed)} / {t.elapsed / len(keys) * 1e6:.1f} us")


# --------------------------------------------------------------------------- main


async def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("endpoint", nargs="?", default=DEFAULT_ENDPOINT)
    parser.add_argument("--netuids", default="1,4,64")
    parser.add_argument("--pages", type=int, default=5)
    parser.add_argument("--page-size", type=int, default=1000)
    args = parser.parse_args()
    netuids = [int(x) for x in args.netuids.split(",") if x.strip()]

    print(f"endpoint: {args.endpoint}\n")
    conn = await bench_connect(args.endpoint)
    try:
        await bench_rtt(conn)
        await bench_single_query(conn)
        await bench_metagraph(conn, netuids)
        await bench_query_map(conn, args.pages, args.page_size)
        await bench_signing(conn)
        bench_ss58()
    finally:
        await conn.close()

    for s in SECTIONS:
        print(f"== {s.title}")
        width = max(len(label) for label, _ in s.rows)
        for label, value in s.rows:
            print(f"   {label:<{width}}  {value}")
        print()


if __name__ == "__main__":
    asyncio.run(main())
