"""The rig's keepers, for as long as this runs.

1. The price feed. The chain accepts a TAO price for 300 blocks. On the fast runtime
   that is 75 seconds: without a feed gamma stops minting, and credits orders return as
   refunds.
2. The flusher. The hub keeps its messages in a queue for each bridge until someone asks
   the gateway to hand them to the transport (`Gateway.flush`): this keeper does. A
   message that is not paid for is deferred, so that the ones behind it leave. The keeper
   sends a deferred message as soon as the hub pays all that the transport charges for it
   (`Gateway.send`).
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
from bittensor import rails
from bittensor.evm.rpc import EvmRpc, EvmRpcError

# Seconds between feeds: well inside the 75 seconds a price lasts.
FEED_EVERY = 20
# Seconds between looks at the hub's messages for the spoke.
FLUSH_EVERY = 2
# Seconds between the validators' weights: well inside the two minutes they stay active.
SCORE_EVERY = 30
# The Rails precompile on Bittensor's EVM.
RAILS_PRECOMPILE = "0x0000000000000000000000000000000000000815"
# The gateway's hand-over of up to eight of the messages in the spoke's line.
FLUSH = rails.encode_flush(SPOKE_BRIDGE, 8)


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


def in_line(hub: EvmRpc) -> bool:
    """Whether a message for the spoke waits at the head of the hub's line."""
    data = calldata("nextOutbound(uint32)", ["uint32"], [SPOKE_BRIDGE])
    result = hub.eth_call({"to": RAILS_PRECOMPILE, "data": data})
    return abi_decode(["bool"], bytes.fromhex(result[2:66]))[0]


async def deferred(chain: bt.Client) -> list[int]:
    """The numbers of the spoke's deferred messages: they left the line, and not the hub.
    None before the hub knows the spoke's bridge."""
    for bridge in await chain.rails.bridges():
        if bridge["bridge"] == SPOKE_BRIDGE:
            return bridge["messages_deferred"]
    return []


def paid_for(hub: EvmRpc, gateway: str, nonce: int) -> bool:
    """Whether the hub now pays all that the transport charges for deferred message
    `nonce`. This keeper adds nothing of its own: it sends the message only then."""
    try:
        hub.eth_call(
            {"from": KEEPER, "to": gateway, "data": rails.encode_send(SPOKE_BRIDGE, nonce)}
        )
    except EvmRpcError:
        return False
    return True


async def flush() -> None:
    hub = EvmRpc(HUB_RPC)
    deployment = DEPLOYMENTS / "hub.local.json"
    # The deferred messages this keeper has already said are not paid for.
    reported: set[int] = set()
    async with client() as chain:
        while True:
            await asyncio.sleep(FLUSH_EVERY)
            try:
                # The gateway is deployed after this keeper starts.
                if not deployment.exists():
                    continue
                gateway = json.loads(deployment.read_text())["gateway"]
                if in_line(hub):
                    # Each message in line leaves, or is deferred if it is not paid for.
                    await asyncio.to_thread(send, hub, KEEPER_KEY, gateway, FLUSH)
                    print("[keeper] flushed the spoke's line", flush=True)
                waiting = await deferred(chain)
                reported &= set(waiting)
                for nonce in waiting:
                    if paid_for(hub, gateway, nonce):
                        data = rails.encode_send(SPOKE_BRIDGE, nonce)
                        await asyncio.to_thread(send, hub, KEEPER_KEY, gateway, data)
                        print(f"[keeper] sent deferred message {nonce}", flush=True)
                    elif nonce not in reported:
                        reported.add(nonce)
                        print(
                            f"[keeper] message {nonce} is deferred: the transport charges "
                            "more for it than the hub pays. Pay into the spoke's account on "
                            "the hub (Rails.fund_messages), raise the ceiling, or pay the "
                            "difference (Gateway.send).",
                            flush=True,
                        )
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
