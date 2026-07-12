"""Verify live Finney EVM association buckets are within the runtime cap.

Before proposing spec 426 (``MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS`` + reverse-index
migration), scan mainnet state and fail if any ``(netuid, evm_address)`` bucket
already exceeds the cap — the migration would prune overflow entries.

Usage:
    uv run python scripts/verify_finney_evm_association_cap.py
    uv run python scripts/verify_finney_evm_association_cap.py wss://entrypoint-finney.opentensor.ai:443
"""

from __future__ import annotations

import argparse
import asyncio
from collections import Counter
from typing import Any

from bittensor._generated import storage as st
from bittensor.client import Client

DEFAULT_ENDPOINT = "wss://entrypoint-finney.opentensor.ai:443"
DEFAULT_CAP = 32


def _evm_key_hex(value: Any) -> str:
    if isinstance(value, str):
        return value.lower().removeprefix("0x")
    if isinstance(value, (bytes, bytearray)):
        return bytes(value).hex()
    if isinstance(value, dict):
        for key in ("evm_key", "EvmKey", "address"):
            if key in value:
                return _evm_key_hex(value[key])
    return str(value).lower().removeprefix("0x")


def _bucket_counts(entries: list[tuple[Any, Any]]) -> Counter[tuple[int, str]]:
    counts: Counter[tuple[int, str]] = Counter()
    for key, value in entries:
        if isinstance(key, tuple):
            netuid, _uid = key[0], key[1]
        else:
            netuid, _uid = key, None
        evm_key = _evm_key_hex(value[0] if isinstance(value, (list, tuple)) else value)
        counts[(int(netuid), evm_key)] += 1
    return counts


async def verify(endpoint: str, *, cap: int) -> int:
    async with Client(endpoint) as client:
        entries = await client.query_map(st.SubtensorModule.AssociatedEvmAddress)

    counts = _bucket_counts(entries)
    total = len(entries)
    max_bucket = max(counts.values(), default=0)
    offenders = sorted(
        ((netuid, evm_key, count) for (netuid, evm_key), count in counts.items() if count > cap),
        key=lambda row: (-row[2], row[0], row[1]),
    )

    print(
        f"Finney EVM associations: {total} forward-map entries, "
        f"{len(counts)} unique (netuid, evm_address) buckets, "
        f"largest bucket {max_bucket} (cap {cap})"
    )

    if offenders:
        print("OVER CAP — buckets that exceed the runtime limit:")
        for netuid, evm_key, count in offenders[:20]:
            print(f"  netuid={netuid} evm=0x{evm_key} count={count}")
        if len(offenders) > 20:
            print(f"  ... and {len(offenders) - 20} more")
        return 1

    print("ok: every bucket is within cap")
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "endpoint",
        nargs="?",
        default=DEFAULT_ENDPOINT,
        help=f"Finney websocket endpoint (default: {DEFAULT_ENDPOINT})",
    )
    parser.add_argument(
        "--max-per-address",
        type=int,
        default=DEFAULT_CAP,
        help=f"Maximum UIDs per EVM address per subnet (default: {DEFAULT_CAP})",
    )
    args = parser.parse_args()
    raise SystemExit(asyncio.run(verify(args.endpoint, cap=args.max_per_address)))


if __name__ == "__main__":
    main()
