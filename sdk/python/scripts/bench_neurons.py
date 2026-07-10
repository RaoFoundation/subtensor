"""Follow-up probe: decode cost of the heaviest runtime-API payloads
(NeuronInfoRuntimeApi.get_neurons, weights/bonds included) across subnets.

    python scripts/bench_neurons.py [ws-endpoint] [--netuids 1,3,8,19,64]
"""

from __future__ import annotations

import argparse
import asyncio
import statistics
import time

from bittensor._transport import SubstrateConnection
from bittensor._transport.runtime_api import (
    decode_runtime_api_result,
    encode_runtime_api_params,
)
from bittensor.settings import SS58_FORMAT

DEFAULT_ENDPOINT = "wss://entrypoint-finney.opentensor.ai:443"


async def probe(conn: SubstrateConnection, api: str, method: str, netuid: int) -> None:
    codec = await conn._runtimes.codec_at(None)
    params_hex = encode_runtime_api_params(codec, api, method, [netuid])
    t0 = time.perf_counter()
    result = await conn._session.request("state_call", [f"{api}_{method}", params_hex, None])
    t_rpc = time.perf_counter() - t0
    raw = bytes.fromhex(result.removeprefix("0x"))

    times = []
    decoded = None
    for _ in range(3):
        t0 = time.perf_counter()
        decoded = decode_runtime_api_result(codec, api, method, raw)
        times.append(time.perf_counter() - t0)
    t_dec = statistics.median(times)
    n = len(decoded) if isinstance(decoded, list) else 1
    throughput = len(raw) / t_dec / 1e6 if t_dec else 0.0
    print(
        f"{method} netuid={netuid:<3} {len(raw) / 1e6:6.2f} MB  {n:5} items  "
        f"rpc {t_rpc * 1000:7.1f} ms  decode {t_dec * 1000:8.1f} ms  "
        f"({throughput:.1f} MB/s, decode {100 * t_dec / (t_dec + t_rpc):.0f}% of total)"
    )


async def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("endpoint", nargs="?", default=DEFAULT_ENDPOINT)
    parser.add_argument("--netuids", default="1,3,8,19,64")
    args = parser.parse_args()
    netuids = [int(x) for x in args.netuids.split(",") if x.strip()]

    conn = SubstrateConnection(args.endpoint, ss58_format=SS58_FORMAT)
    await conn.initialize()
    try:
        for netuid in netuids:
            await probe(conn, "NeuronInfoRuntimeApi", "get_neurons_lite", netuid)
        for netuid in netuids:
            await probe(conn, "NeuronInfoRuntimeApi", "get_neurons", netuid)
    finally:
        await conn.close()


if __name__ == "__main__":
    asyncio.run(main())
