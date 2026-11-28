"""The rig's keepers, for as long as this runs.

1. The price feed. The chain accepts a TAO price for 300 blocks. On the fast runtime
   that is 75 seconds: without a feed gamma stops minting, and credits orders return as
   refunds.
2. The flusher. The hub keeps its messages in a queue for each bridge until someone asks
   the gateway to hand them to the transport (`Gateway.flush`): this keeper does. A
   message that is not paid for is deferred, so that the ones behind it leave. The keeper
   sends a deferred message as soon as the hub pays all that the transport charges for it
   (`Gateway.send`). It raises an alert when the gateway cannot hand over the message at
   the head of the queue. When that message has been the head for long enough, the keeper
   defers it on the hub, as anyone can, so that the messages behind it leave
   (`Rails.defer_outbound`). And it has the hub send the portal its order slots when the
   portal is short of them (`Rails.grant_slots`).
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
    keeper_wallet,
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


def head(hub: EvmRpc) -> int | None:
    """The number of the message at the head of the spoke's queue, if one waits there."""
    data = calldata("nextOutbound(uint32)", ["uint32"], [SPOKE_BRIDGE])
    result = bytes.fromhex(hub.eth_call({"to": RAILS_PRECOMPILE, "data": data})[2:])
    pending, nonce = abi_decode(["bool", "uint64"], result[:64])
    return nonce if pending else None


def refusal(hub: EvmRpc, gateway: str) -> str | None:
    """Why the gateway cannot take the spoke's queue now, or None if it can. A message
    that is not paid for is no reason: the gateway defers it. What stops a queue is a
    transport that refuses the message at its head, or a paused gateway."""
    try:
        hub.eth_call({"from": KEEPER, "to": gateway, "data": FLUSH})
    except EvmRpcError as error:
        return str(error)
    return None


async def spoke_bridge(chain: bt.Client) -> dict | None:
    """What the hub holds for the spoke: its messages, its account and its order slots.
    None before the hub knows the spoke's bridge."""
    for bridge in await chain.rails.bridges():
        if bridge["bridge"] == SPOKE_BRIDGE:
            return bridge
    return None


async def grant_slots(chain: bt.Client, bridge: dict) -> None:
    """Has the hub send the portal its order slots, when at most half of them are out and
    the spoke's account covers the message and one slot at least. Slots go with the hub's
    payments otherwise."""
    reserve = bridge["message_reserve"]
    if reserve is None or bridge["order_slots"] * 2 > bridge["max_order_slots"]:
        return
    in_use = bridge["messages_waiting"] + bridge["order_slots"]
    if bridge["message_budget"].rao < reserve.rao * (in_use + 2):
        return
    result = await chain.submit_call(
        bt.calls.Rails.grant_slots(bridge=SPOKE_BRIDGE), keeper_wallet()
    )
    if result.success:
        print("[keeper] the hub sends the spoke its order slots", flush=True)


async def defer_if_stale(chain: bt.Client, nonce: int) -> None:
    """Defers message `nonce`, which the gateway cannot hand over, once it has been at the
    head of the spoke's queue for long enough. Anyone can: the hub asks for no key."""
    since = int(await chain.query(bt.storage.Rails.HeadSince, [SPOKE_BRIDGE]) or 0)
    waited = await chain.block() - since
    if waited < int(await chain.constant(bt.constants.Rails.StaleAfter)):
        return
    call = bt.calls.Rails.defer_outbound(bridge=SPOKE_BRIDGE, nonce=nonce)
    result = await chain.submit_call(call, keeper_wallet())
    if result.success:
        print(
            f"[keeper] deferred message {nonce}: the gateway could not hand it over for "
            f"{waited} blocks. The messages behind it go on, and it leaves when the gateway "
            "can send it.",
            flush=True,
        )


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
    # The message at the head of the queue that the gateway could not hand over.
    stuck: int | None = None
    async with client() as chain:
        while True:
            await asyncio.sleep(FLUSH_EVERY)
            try:
                # The gateway is deployed after this keeper starts.
                if not deployment.exists():
                    continue
                gateway = json.loads(deployment.read_text())["gateway"]
                nonce = head(hub)
                reason = None if nonce is None else refusal(hub, gateway)
                if reason is not None:
                    if stuck != nonce:
                        print(
                            f"[keeper] ALERT: the gateway cannot hand message {nonce} of the "
                            f"spoke to the transport: {reason}. The messages behind it "
                            "wait. Repair the transport. If the message stays at the head, "
                            "this keeper defers it so that the others leave, as anyone can: "
                            f"Rails.defer_outbound({SPOKE_BRIDGE}, {nonce}).",
                            flush=True,
                        )
                    stuck = nonce
                    await defer_if_stale(chain, nonce)
                elif nonce is not None:
                    # Each message in the queue leaves, or is deferred if it is not paid for.
                    await asyncio.to_thread(send, hub, KEEPER_KEY, gateway, FLUSH)
                    print("[keeper] flushed the spoke's queue", flush=True)
                    stuck = None
                bridge = await spoke_bridge(chain)
                if bridge is None:
                    continue
                await grant_slots(chain, bridge)
                waiting = bridge["messages_deferred"]
                reported &= set(waiting)
                for nonce in waiting:
                    if paid_for(hub, gateway, nonce):
                        data = rails.encode_send(SPOKE_BRIDGE, nonce)
                        await asyncio.to_thread(send, hub, KEEPER_KEY, gateway, data)
                        print(f"[keeper] sent deferred message {nonce}", flush=True)
                    elif nonce not in reported:
                        reported.add(nonce)
                        print(
                            f"[keeper] message {nonce} is deferred: the gateway cannot send "
                            "it now. If the transport charges more for it than the hub pays: "
                            "pay into the spoke's account on the hub (Rails.fund_messages), "
                            "raise the ceiling, or pay the difference (Gateway.send).",
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
