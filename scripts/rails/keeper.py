"""The rig's keepers, for as long as this runs.

1. The price feed. The chain accepts a TAO price for 300 blocks. On the fast runtime
   that is 75 seconds: without a feed gamma stops minting, and credits orders return as
   refunds.
2. The flusher. The hub keeps its answers and its exits in a queue for each bridge until
   someone asks the gateway to hand them to the transport. An order from a spoke does
   that for its bridge's queue. An exit that starts on Bittensor waits for the next
   order, or for anyone who calls `Gateway.flush`: this keeper does.
3. The validators. The rig's validators score their miner again every half minute, as
   real validators do, so that they stay active and the index hotkeys keep earning.

In production the first is the root-set oracle, anyone may run the second, and the third
is what validators do.
"""

from __future__ import annotations

import asyncio
import json

from eth_abi import decode as abi_decode
from rig import (
    DEPLOYMENTS,
    HUB_RPC,
    KEEPER,
    KEEPER_KEY,
    SPOKE_BRIDGE,
    TAO_PRICE,
    UNIT,
    VALIDATED,
    calldata,
    client,
    oracle_wallet,
    score_miner,
    send,
)

import bittensor as bt
from bittensor.evm.rpc import EvmRpc

# Seconds between feeds: well inside the 75 seconds a price lasts.
FEED_EVERY = 20
# Seconds between looks at the outbox.
FLUSH_EVERY = 2
# Seconds between the validators' weights: well inside the two minutes they stay active.
SCORE_EVERY = 30
# The Rails precompile on Bittensor's EVM.
RAILS_PRECOMPILE = "0x0000000000000000000000000000000000000815"
# The gateway's hand-over of up to eight of the spoke's messages.
FLUSH = calldata("flush(uint32,uint256)", ["uint32", "uint256"], [SPOKE_BRIDGE, 8])


async def feed() -> None:
    oracle = oracle_wallet()
    async with client() as chain:
        while True:
            result = await chain.submit_call(
                bt.calls.SubtensorModule.feed_tao_usd_price(price=TAO_PRICE * UNIT), oracle
            )
            if not result.success:
                print(f"[keeper] the price feed failed: {result.message}", flush=True)
            await asyncio.sleep(FEED_EVERY)


def outbox_waits(hub: EvmRpc) -> bool:
    """Whether the hub has a message for the spoke that the gateway has not sent."""
    data = calldata("nextOutbound(uint32)", ["uint32"], [SPOKE_BRIDGE])
    result = hub.eth_call({"to": RAILS_PRECOMPILE, "data": data})
    return abi_decode(["bool"], bytes.fromhex(result[2:66]))[0]


def would_send(hub: EvmRpc, gateway: str) -> int:
    """How many messages a hand-over would send now. None leave while the transport
    charges more than the hub pays for one: then somebody has to pay the difference, and
    this keeper does not."""
    result = hub.eth_call({"from": KEEPER, "to": gateway, "data": FLUSH})
    return abi_decode(["uint256"], bytes.fromhex(result[2:]))[0]


async def flush() -> None:
    hub = EvmRpc(HUB_RPC)
    deployment = DEPLOYMENTS / "hub.local.json"
    while True:
        await asyncio.sleep(FLUSH_EVERY)
        try:
            # The gateway is deployed after this keeper starts.
            if not deployment.exists() or not outbox_waits(hub):
                continue
            gateway = json.loads(deployment.read_text())["gateway"]
            if would_send(hub, gateway) == 0:
                continue
            await asyncio.to_thread(send, hub, KEEPER_KEY, gateway, FLUSH)
            print("[keeper] flushed the outbox", flush=True)
        except Exception as error:
            print(f"[keeper] the flush failed: {error!r}", flush=True)


async def score() -> None:
    async with client() as chain:
        while True:
            await asyncio.sleep(SCORE_EVERY)
            try:
                for subnet in json.loads(VALIDATED.read_text()) if VALIDATED.exists() else []:
                    await score_miner(chain, subnet["index"], subnet["netuid"], subnet["miner_uid"])
            except Exception as error:
                print(f"[keeper] the validators did not score: {error!r}", flush=True)


async def main() -> None:
    await asyncio.gather(feed(), flush(), score())


if __name__ == "__main__":
    asyncio.run(main())
