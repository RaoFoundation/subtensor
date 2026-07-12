"""Prepare a sudo-enabled mainnet clone so the clone-safe e2e subset can run.

The clone-upgrade CI job runs part of the SDK e2e suite against a mainnet
clone whose sudo key was swapped to //Alice. The suite's ``owned_subnet``
fixture registers a subnet, and mainnet state blocks that three ways, each
neutralized here with sudo:

- ``register_network`` is rate-limited network-wide, and the preceding js
  regression tests bump the last-registration block, so registration fails
  with RateLimitExceeded -> zero ``NetworkRateLimit``.
- Mainnet sits at the subnet limit, where registration queues behind a
  multi-minute on_idle prune of the replaced subnet -> raise ``MaxSubnets``
  so registrations stay on the immediate path.
- The lock cost doubles per registration from mainnet scale, and //Alice's
  clone-spec grant is finite -> decay the lock cost back to the minimum
  after one block so reruns and multiple registrations stay affordable.

Usage:
    uv run python scripts/prepare_clone_for_e2e.py [ws://127.0.0.1:9944]
"""

from __future__ import annotations

import asyncio
import sys
from types import SimpleNamespace

import bittensor as sub
from bittensor.client import Client
from bittensor.keyfiles import Keypair

DEFAULT_ENDPOINT = "ws://127.0.0.1:9944"
SUBNET_HEADROOM = 32


def _alice() -> SimpleNamespace:
    cold = Keypair.create_from_uri("//Alice")
    return SimpleNamespace(
        coldkey=cold,
        coldkeypub=Keypair(ss58_address=cold.ss58_address),
        hotkey=Keypair.create_from_uri("//Alice//hot"),
    )


async def _sudo(client: Client, wallet: SimpleNamespace, call) -> None:
    inner = await client.compose(call)
    result = await client.submit_call(sub.calls.Sudo.sudo(call=inner), wallet)
    if not result.success:
        raise SystemExit(f"sudo({call}) failed: {result.message}")
    print(f"ok: {call}")


async def main(endpoint: str) -> None:
    alice = _alice()
    async with Client(endpoint) as client:
        total_networks = int(await client.query(sub.storage.SubtensorModule.TotalNetworks))
        await _sudo(client, alice, sub.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0))
        await _sudo(
            client,
            alice,
            sub.calls.AdminUtils.sudo_set_subnet_limit(
                max_subnets=total_networks + SUBNET_HEADROOM
            ),
        )
        await _sudo(
            client, alice, sub.calls.AdminUtils.sudo_set_lock_reduction_interval(interval=1)
        )


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1] if len(sys.argv) > 1 else DEFAULT_ENDPOINT))
