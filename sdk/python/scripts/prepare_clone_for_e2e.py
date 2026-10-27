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
- Mainnet's ``NetworkRegistrationStartBlock`` is far beyond the clone's
  restarted block height, so registration reads as not yet open -> zero it
  via a raw storage write (no AdminUtils setter exists).

Usage:
    uv run python scripts/prepare_clone_for_e2e.py [ws://127.0.0.1:9944]
"""

from __future__ import annotations

import asyncio
import sys
from types import SimpleNamespace

import bittensor as bt
from bittensor.client import Client
from bittensor.keyfiles import Keypair

DEFAULT_ENDPOINT = "ws://127.0.0.1:9944"
SUBNET_HEADROOM = 32

# twox128("SubtensorModule") ++ twox128("NetworkRegistrationStartBlock").
# Mainnet stores a mainnet-scale start block here, but the clone's chain
# restarts from block 0, so do_register_network reads "not started yet" and
# fails with SubNetRegistrationDisabled. No AdminUtils setter exists for it,
# hence the raw System.set_storage write.
REGISTRATION_START_BLOCK_KEY = "0x658faa385070e074c85bf6b568cf0555450757a33d9ee73139121004b3b10b2e"
SCALE_U64_ZERO = "0x0000000000000000"


def _alice() -> SimpleNamespace:
    cold = Keypair.create_from_uri("//Alice")
    return SimpleNamespace(
        coldkey=cold,
        coldkeypub=Keypair(ss58_address=cold.ss58_address),
        hotkey=Keypair.create_from_uri("//Alice//hot"),
    )


async def _sudo(client: Client, wallet: SimpleNamespace, call) -> None:
    inner = await client.compose(call)
    result = await client.submit_call(bt.calls.Sudo.sudo(call=inner), wallet)
    if not result.success:
        raise SystemExit(f"sudo({call}) failed: {result.message}")
    print(f"ok: {call}")


async def main(endpoint: str) -> None:
    alice = _alice()
    async with Client(endpoint) as client:
        total_networks = int(await client.query(bt.storage.SubtensorModule.TotalNetworks))
        await _sudo(client, alice, bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0))
        await _sudo(
            client,
            alice,
            bt.calls.AdminUtils.sudo_set_subnet_limit(max_subnets=total_networks + SUBNET_HEADROOM),
        )
        await _sudo(client, alice, bt.calls.AdminUtils.sudo_set_lock_reduction_interval(interval=1))
        await _sudo(
            client,
            alice,
            bt.calls.System.set_storage(items=[(REGISTRATION_START_BLOCK_KEY, SCALE_U64_ZERO)]),
        )


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1] if len(sys.argv) > 1 else DEFAULT_ENDPOINT))
