"""The rails, end to end, against the local rig.

A person on the spoke holds only the stablecoin and a wallet. This script does what that
person does and checks what the two chains say after each step:

1. buy a subnet's alpha token: deposit -> stake -> the token is in the wallet;
2. sell it: the stake is released -> the stablecoin is back;
3. buy and sell canonical TAO;
4. buy service credits: the gamma is burned at the service, and the gateway emits the
   event, keyed by the buyer's address, that the service watches;
5. an order that cannot meet its minimum returns in full, in the stablecoin;
6. a holder of TAO on Bittensor leaves through the bridge;
7. who paid for the messages: each order on the spoke, each exit on the hub, and the
   spoke's account on the hub for exactly what the transport charged;
8. the transport charges more than the hub pays for a message: orders still settle, and
   their answers are deferred. A newer one leaves first, when its receiver pays the
   difference. The older one leaves by itself when the hub may pay the difference from
   what the spoke's account can spare;
9. the hub's answers do not leave, so the order slots that they carry do not come back:
   when the portal has used its last slot it takes no order, and the payer keeps the
   funds. The slots come back with the answers;
10. the gateway cannot hand a message to the transport: the keeper raises an alert, and
    when the message has been at the head of its queue for long enough the keeper defers
    it on the hub, as anyone can. The message leaves when the gateway can send it.

It ends with the invariants that hold between the chains at any time.
"""

from __future__ import annotations

import asyncio
import sys
from dataclasses import dataclass, replace

from eth_abi import decode as abi_decode
from eth_abi import encode as abi_encode
from eth_utils import keccak
from rig import (
    ANSWER_FEE,
    DEPLOYMENTS,
    GUARDIAN_KEY,
    HUB_RPC,
    MESSAGE_CEILING,
    MESSAGE_RESERVE,
    MULTISIG,
    MULTISIG_KEY,
    ORDER_SLOTS,
    RESERVE_ACCOUNT,
    SPOKE_BRIDGE,
    SPOKE_DOMAIN,
    TRANSPORT_FEE,
    UNIT,
    USDC,
    USER,
    USER_KEY,
    VALIDATORS,
    Spoke,
    calldata,
    client,
    dev_wallet,
    index_hotkey,
    load_json,
    manifest,
    mirror,
    send,
    submit,
    vault_account,
    wait_for,
)

import bittensor as bt
from bittensor import rails
from bittensor.evm.addresses import ss58_to_pubkey
from bittensor.evm.rpc import EvmRpc, EvmRpcError

CREDITS_PURCHASED = keccak(text="CreditsPurchased(address,uint16,bytes32,uint32,uint64,uint64)")
# The Rails precompile on Bittensor's EVM.
RAILS_PRECOMPILE = "0x0000000000000000000000000000000000000815"
# One unit of the stablecoin in the hub's 9 decimals.
STABLE_SCALE = UNIT // USDC
# The EVM counts TAO with 18 decimals, the hub with 9.
WEI_PER_RAO = 10**9


def show(step: str, detail: str = "") -> None:
    print(f"[e2e] {step}" + (f": {detail}" if detail else ""), flush=True)


def units(amount: int, decimals: int = 9) -> str:
    return f"{amount / 10**decimals:,.6f}".rstrip("0").rstrip(".")


@dataclass(frozen=True)
class Transport:
    """What pays for the hub's messages to the spoke at one moment, in rao."""

    # What the spoke's account on the hub can pay the transport.
    budget: int
    # The TAO of the gateway account. The hub hands it what it pays the transport.
    gateway: int
    # How many messages for the spoke the gateway has handed to the transport.
    sent: int
    # The numbers of the spoke's deferred messages: they left the line, and not the hub.
    deferred: tuple[int, ...]


class Rig:
    """The two chains, as the e2e sees them."""

    def __init__(self, chain: bt.Client):
        facts = manifest()
        (addresses,) = facts["spokes"].values()
        self.chain = chain
        self.spoke = Spoke(addresses)
        self.hub_evm = EvmRpc(HUB_RPC)
        self.gateway = facts["hub"]["gateway"]
        self.markets = facts["markets"]

    async def answer(self, order: int) -> dict:
        """The portal's payment in answer to `order`, once it arrives."""

        async def arrived():
            return self.spoke.answers(order)

        (answer,) = await wait_for(f"the answer to order {order}", arrived)
        return answer

    async def receipt(self, order: int) -> dict:
        """What the hub did with `order`."""

        async def disposed():
            record = await self.chain.rails.bridge_order(SPOKE_BRIDGE, order)
            return record if record["status"] != "pending" else None

        return await wait_for(f"the hub to take order {order}", disposed)

    async def transport(self) -> Transport:
        """What pays for the hub's messages to the spoke, now."""
        budget = await self.chain.runtime(bt.runtime_api.RailsApi.message_budget, [SPOKE_BRIDGE])
        (bridge,) = [b for b in await self.chain.rails.bridges() if b["bridge"] == SPOKE_BRIDGE]
        deferred = tuple(bridge["messages_deferred"])
        # Every message below the head of the line left the hub, or is deferred.
        head = int(await self.chain.query(bt.storage.Rails.NextOutbound, [SPOKE_BRIDGE]) or 0)
        return Transport(
            budget=int(budget),
            gateway=(await self.chain.balances.balance(mirror(self.gateway))).rao,
            sent=head - len(deferred),
            deferred=deferred,
        )

    async def staked(self, netuid: int) -> int:
        """The alpha the hub's vault of `netuid` has staked through the index hotkey."""
        stake = await self.chain.staking.stake(vault_account(netuid), index_hotkey(netuid), netuid)
        return stake.rao

    def credits_events(self, order: int) -> list[dict]:
        """The gateway's `CreditsPurchased` events for `order` of this rig's spoke."""
        logs = self.hub_evm.call(
            "eth_getLogs",
            [
                {
                    "address": self.gateway,
                    "fromBlock": "0x0",
                    "toBlock": "latest",
                    "topics": ["0x" + CREDITS_PURCHASED.hex()],
                }
            ],
        )
        events = []
        for log in logs:
            origin, nonce, gamma = abi_decode(
                ["uint32", "uint64", "uint64"], bytes.fromhex(log["data"][2:])
            )
            if origin == SPOKE_DOMAIN and nonce == order:
                events.append(
                    {
                        "beneficiary": rails.decode_address(log["topics"][1]),
                        "netuid": int(log["topics"][2], 16),
                        "service": log["topics"][3],
                        "gamma": gamma,
                    }
                )
        return events

    def credits_of(self, netuid: int, service: str, address: str) -> int:
        """The precompile's running total of gamma spent at `service` for `address`."""
        data = keccak(text="creditsOf(uint16,bytes32,address)")[:4] + abi_encode(
            ["uint16", "bytes32", "address"],
            [netuid, bytes.fromhex(ss58_to_pubkey(service)[2:]), address],
        )
        return rails.decode_uint(
            self.hub_evm.eth_call({"to": RAILS_PRECOMPILE, "data": "0x" + data.hex()})
        )


async def buy_and_sell_alpha(rig: Rig, market: dict) -> None:
    spoke, netuid, token = rig.spoke, market["netuid"], market["token"]
    paid = 100 * USDC
    stable_before = spoke.balance(spoke.stable, USER)
    held_before = spoke.balance(token, USER)
    staked_before = await rig.staked(netuid)

    order = spoke.deposit(USER_KEY, paid, rails.to_alpha(netuid, USER))
    answer = await rig.answer(order)
    assert not answer["refund"], f"the hub refunded the purchase: {await rig.receipt(order)}"
    assert answer["asset"] == "alpha" and answer["netuid"] == netuid
    held = spoke.balance(token, USER) - held_before
    assert held == answer["amount"] > 0, "the token is not in the wallet"
    worth = spoke.convert_to_assets(token, held)
    staked = await rig.staked(netuid) - staked_before
    # The token is a share of the vault's stake: it is worth what was staked for it. The
    # stake has earned since, and on a localnet it earns by the second.
    assert 0 < worth <= staked <= worth + worth // 5, (worth, staked)
    show(
        f"bought {market['name']}",
        f"{units(paid, 6)} USDC -> {units(held)} {spoke.symbol(token)}, "
        f"worth {units(worth)} alpha staked through the index hotkey",
    )

    # The index hotkey has no key and no UID. It lends its stake weight to the subnet's
    # validators by rule: here to all four, a quarter each.
    children = await rig.chain.query(
        bt.storage.SubtensorModule.ChildKeys, [index_hotkey(netuid), netuid]
    )
    assert len(children) == VALIDATORS, children
    assert all(proportion == (2**64 - 1) // 4 for proportion, _ in children), children

    # The token does not rebase. It earns: what the validators earn on the stake is added
    # to it, so each token is worth more alpha than when it was bought.
    async def earned():
        rate = await rig.chain.runtime(bt.runtime_api.RailsApi.alpha_rate, [netuid])
        now = held * int(rate) // 10**18
        return now if now > worth else None

    grown = await wait_for("the token to earn", earned)
    show(
        f"{market['name']} earns",
        f"{units(held)} {spoke.symbol(token)} is worth {units(grown)} alpha, "
        f"+{(grown - worth) * 100 / worth:.4f}% since bought",
    )

    order = spoke.sell(USER_KEY, token, held, rails.to_stable(USER))
    answer = await rig.answer(order)
    assert not answer["refund"], f"the hub refunded the sale: {await rig.receipt(order)}"
    assert answer["asset"] == "stable"
    assert spoke.balance(token, USER) == held_before
    returned = spoke.balance(spoke.stable, USER) - (stable_before - paid)
    assert returned == answer["amount"] // STABLE_SCALE > 0, "the stablecoin did not come back"
    # Two pool fees and the price impact each way: well under a tenth on the rig's pools.
    assert returned > paid * 9 // 10, f"only {units(returned, 6)} USDC came back"
    # The sale releases the stake. The vault keeps a trace: the part of the earnings that
    # belongs to the shares nobody holds (the vault's guard against a first-buyer attack).
    assert await rig.staked(netuid) < staked_before + staked, "the stake was not released"
    show(f"sold {market['name']}", f"{units(held)} -> {units(returned, 6)} USDC back")


async def buy_and_sell_tao(rig: Rig) -> None:
    spoke = rig.spoke
    paid = 200 * USDC
    stable_before = spoke.balance(spoke.stable, USER)

    held_before = spoke.balance(spoke.tao, USER)
    order = spoke.deposit(USER_KEY, paid, rails.to_tao(USER))
    answer = await rig.answer(order)
    assert not answer["refund"] and answer["asset"] == "tao"
    held = spoke.balance(spoke.tao, USER) - held_before
    assert held == answer["amount"] > 0
    show("bought TAO", f"{units(paid, 6)} USDC -> {units(held)} TAO")

    # Sell half. The other half stays on the spoke, backed by the hub's reserve.
    sold = held // 2
    order = spoke.sell(USER_KEY, spoke.tao, sold, rails.to_stable(USER))
    answer = await rig.answer(order)
    assert not answer["refund"] and answer["asset"] == "stable"
    returned = spoke.balance(spoke.stable, USER) - (stable_before - paid)
    assert returned > paid * 99 // 200, f"only {units(returned, 6)} USDC came back"
    assert spoke.balance(spoke.tao, USER) == held_before + held - sold
    show("sold TAO", f"{units(sold)} TAO -> {units(returned, 6)} USDC back")


async def buy_credits(rig: Rig, market: dict) -> None:
    spoke, netuid, service = rig.spoke, market["netuid"], market["service"]
    paid = 50 * USDC
    before = rig.credits_of(netuid, service, USER)

    order = spoke.deposit(USER_KEY, paid, rails.to_credits(netuid, service, USER))
    receipt = await rig.receipt(order)
    assert receipt["status"] == "settled", receipt

    async def emitted():
        return rig.credits_events(order)

    (event,) = await wait_for("the gateway's CreditsPurchased event", emitted)
    assert event["beneficiary"].lower() == USER.lower()
    assert event["netuid"] == netuid
    assert event["service"] == ss58_to_pubkey(service)
    gamma = event["gamma"]
    assert gamma > 0
    # 1 gamma is one dollar of alpha burned, valued at the lower of the subnet's slow price
    # and its spot price: never more credit than the dollars paid. On a localnet, where a
    # subnet's price climbs by the minute, the slow price is well below the spot price.
    assert 0 < gamma <= paid * STABLE_SCALE, units(gamma)
    assert rig.credits_of(netuid, service, USER) == before + gamma
    total = await rig.chain.rails.credits_spent(netuid, service, USER)
    assert float(total) * UNIT >= gamma
    show(
        f"bought {market['name']} credits",
        f"{units(paid, 6)} USDC -> {units(gamma)} gamma burned at the service for {USER}",
    )


async def refund(rig: Rig, market: dict) -> None:
    spoke = rig.spoke
    paid = 25 * USDC
    stable_before = spoke.balance(spoke.stable, USER)

    # No pool gives this much alpha for 25 dollars: the hub must refuse the order.
    order = spoke.deposit(USER_KEY, paid, rails.to_alpha(market["netuid"], USER), min_out=10**18)
    answer = await rig.answer(order)
    assert answer["refund"] and answer["asset"] == "stable"
    assert spoke.balance(spoke.stable, USER) == stable_before, "the refund is not whole"
    receipt = await rig.receipt(order)
    assert receipt["status"] == "refunded", receipt
    show("refund", f"{units(paid, 6)} USDC returned in full ({receipt['error']})")


async def leave_from_bittensor(rig: Rig) -> None:
    """Alice sells TAO for tUSD in the pool, then sends the tUSD out as the stablecoin."""
    alice = dev_wallet()
    chain, spoke = rig.chain, rig.spoke
    address = alice.coldkey.ss58_address

    before = int(await chain.query(bt.storage.Tusd.Balances, [address]) or 0)
    result = await chain.execute(bt.intents.SwapPool(sell="tao", amount=1.0), alice)
    assert result.success, result.message
    held = int(await chain.query(bt.storage.Tusd.Balances, [address]) or 0) - before
    assert held > 0
    stable_before = spoke.balance(spoke.stable, USER)
    before = await rig.transport()
    result = await chain.execute(
        bt.intents.BridgeOut(
            bridge=SPOKE_BRIDGE,
            pay_with="tusd",
            amount=held / UNIT,
            receive="stable",
            to_address=USER,
        ),
        alice,
    )
    assert result.success, result.message

    async def released():
        return spoke.balance(spoke.stable, USER) - stable_before

    arrived = await wait_for("the stablecoin of the exit", released)
    assert arrived == held // STABLE_SCALE
    # The exit paid the reserve for one message into the spoke's account, on top of what
    # it sent. Its message left, and the account paid what the transport charged for it.
    after = await rig.transport()
    assert after.sent == before.sent + 1, (before, after)
    assert after.budget == before.budget + MESSAGE_RESERVE - TRANSPORT_FEE, (before, after)
    show(
        "left Bittensor",
        f"1 TAO -> {units(held)} tUSD -> {units(arrived, 6)} USDC on the spoke. The exit "
        f"paid {units(MESSAGE_RESERVE)} TAO into the spoke's account on the hub; the "
        f"transport charged {units(TRANSPORT_FEE)} TAO for its message",
    )


async def who_paid(rig: Rig, first_order: int, fees_before: int, start: Transport) -> None:
    """Each order paid for its answer on the spoke. On the hub, the spoke's account paid
    exactly what the transport charged for each message, and the gateway kept nothing."""
    spoke = rig.spoke
    orders = spoke.next_order() - first_order
    collected = spoke.native_balance(spoke.portal) - fees_before
    assert orders > 0 and collected == orders * ANSWER_FEE, (orders, collected)

    end = await rig.transport()
    sent = end.sent - start.sent
    # One exit paid the reserve for a message in. The rest of the change is what the
    # account paid the transport: its charge for each message.
    charged = start.budget + MESSAGE_RESERVE - end.budget
    assert sent > 0 and charged == sent * TRANSPORT_FEE, (sent, charged)
    # The hub handed the gateway that much, and the gateway paid all of it on: it holds
    # what it held before.
    assert end.gateway == start.gateway, (start, end)

    recipient_before = spoke.native_balance(MULTISIG)
    waiting = spoke.native_balance(spoke.portal)
    # Anyone may send them on. They can only go to the recipient.
    spoke.send(USER_KEY, spoke.portal, calldata("sweepFees()", [], []))
    assert spoke.native_balance(spoke.portal) == 0
    assert spoke.native_balance(MULTISIG) == recipient_before + waiting
    show(
        "who paid for the messages",
        f"{orders} orders paid {units(collected, 18)} ETH for their answers, swept to the "
        f"fee recipient; the spoke's account on the hub paid the transport "
        f"{units(charged)} TAO for {sent} messages and can pay {units(end.budget)} TAO "
        f"more; the gateway holds {end.gateway} rao, as before",
    )


async def set_message_terms(rig: Rig, reserve: int, ceiling: int) -> None:
    """Root sets what the hub pays the transport for one message to the spoke."""
    call = bt.calls.Rails.set_message_reserve(bridge=SPOKE_BRIDGE, reserve=reserve, ceiling=ceiling)
    await submit(rig.chain, call, dev_wallet(), root=True)


async def deferred_order(rig: Rig) -> tuple[int, int]:
    """An order that settles on the hub while its answer is deferred: the transport
    charges more for the answer than the hub pays, and the keeper adds nothing. Returns
    the order, and the number of the hub's message that answers it."""
    spoke = rig.spoke
    before = await rig.transport()
    order = spoke.deposit(USER_KEY, 10 * USDC, rails.to_tao(USER))
    receipt = await rig.receipt(order)
    assert receipt["status"] == "settled", receipt

    async def deferred():
        now = await rig.transport()
        return now if len(now.deferred) > len(before.deferred) else None

    after = await wait_for(f"the keeper to defer the answer to order {order}", deferred)
    (message,) = set(after.deferred) - set(before.deferred)
    # The answer left the line and not the hub: nothing was paid, and nothing arrived.
    assert after == replace(before, deferred=after.deferred), (before, after)
    assert not spoke.answers(order)
    return order, message


async def a_charge_that_is_not_paid_for(rig: Rig) -> None:
    """The transport charges more than the hub pays for a message. Orders still settle,
    and their answers are deferred: they leave the line and stay in the hub. The newer
    answer leaves first, when its receiver pays the difference. The older one leaves by
    itself when the hub may pay the difference from what the spoke's account can spare."""
    spoke, hub = rig.spoke, rig.hub_evm
    reserve = TRANSPORT_FEE // 2

    # The hub pays half the charge for a message, and no more.
    await set_message_terms(rig, reserve, reserve)
    try:
        tokens = spoke.balance(spoke.tao, USER)
        older, older_message = await deferred_order(rig)
        newer, newer_message = await deferred_order(rig)
        assert newer_message > older_message, (older_message, newer_message)

        # The receiver has the gateway send the newer answer, and pays what the hub does
        # not. What was sent along and not needed comes back.
        before = await rig.transport()
        payer_before = int(hub.call("eth_getBalance", [USER, "latest"]), 16)
        sent = send(
            hub,
            USER_KEY,
            rig.gateway,
            rails.encode_send(SPOKE_BRIDGE, newer_message),
            value=1_000 * TRANSPORT_FEE * WEI_PER_RAO,
        )
        gas = int(sent["gasUsed"], 16) * int(sent["effectiveGasPrice"], 16)
        paid = payer_before - int(hub.call("eth_getBalance", [USER, "latest"]), 16) - gas
        assert paid == (TRANSPORT_FEE - reserve) * WEI_PER_RAO, paid
        second = await rig.answer(newer)
        after = await rig.transport()
        # The hub paid the reserve and no more, and the gateway kept nothing. The older
        # answer is still deferred: the spoke takes the hub's messages in any order.
        expected = Transport(
            before.budget - reserve, before.gateway, before.sent + 1, (older_message,)
        )
        assert after == expected, (after, expected)
        assert not spoke.answers(older)

        # The admin lets the hub pay up to twice the charge for a message. The spoke's
        # account has TAO to spare, so the keeper sends the older answer: nobody pays the
        # gateway.
        await set_message_terms(rig, reserve, 2 * TRANSPORT_FEE)
        first = await rig.answer(older)
        before, after = after, await rig.transport()
        # The account paid the whole charge: the message's reserve, and the rest from what
        # it could spare.
        expected = Transport(before.budget - TRANSPORT_FEE, before.gateway, before.sent + 1, ())
        assert after == expected, (after, expected)
        assert spoke.balance(spoke.tao, USER) - tokens == first["amount"] + second["amount"]
    finally:
        await set_message_terms(rig, MESSAGE_RESERVE, MESSAGE_CEILING)
    show(
        "a charge that is not paid for",
        f"the transport charged {TRANSPORT_FEE} rao and the hub paid at most {reserve}: two "
        f"orders settled and their answers were deferred. The newer one left first, when "
        f"its receiver paid the other {paid // WEI_PER_RAO} rao. The older one left by "
        f"itself when the hub could pay the rest from what the spoke's account can spare",
    )


async def no_order_without_a_slot(rig: Rig) -> None:
    """The hub's answers do not leave, so the order slots that they carry do not come
    back. When the portal has used its last slot it takes no order, and the payer keeps
    the funds. When the answers leave, the slots come back, and the portal takes orders
    again."""
    spoke = rig.spoke
    reserve = TRANSPORT_FEE // 2
    paid = 10 * USDC

    async def slots_are_back():
        return spoke.order_slots() == ORDER_SLOTS

    # The hub pays half the charge for a message, and nobody pays the rest.
    await set_message_terms(rig, reserve, reserve)
    try:
        tokens = spoke.balance(spoke.tao, USER)
        assert spoke.order_slots() == ORDER_SLOTS, spoke.order_slots()
        # Each order uses a slot. Its answer would bring the slot back, and is deferred.
        orders = [(await deferred_order(rig))[0] for _ in range(ORDER_SLOTS)]
        assert spoke.order_slots() == 0

        # The hub has not promised to answer one more order. The portal refuses it, and
        # nothing leaves the payer's wallet.
        stable = spoke.balance(spoke.stable, USER)
        sent = spoke.next_order()
        try:
            spoke.deposit(USER_KEY, paid, rails.to_tao(USER))
            refused = False
        except EvmRpcError:
            refused = True
        assert refused, "the portal took an order without a slot"
        assert spoke.balance(spoke.stable, USER) == stable
        assert spoke.next_order() == sent

        # Root lets the hub pay up to twice the charge for a message. The keeper sends the
        # deferred answers, and they bring the slots back.
        await set_message_terms(rig, reserve, 2 * TRANSPORT_FEE)
        answers = [await rig.answer(order) for order in orders]
        await wait_for("the portal's order slots", slots_are_back)
        assert spoke.balance(spoke.tao, USER) - tokens == sum(a["amount"] for a in answers)
    finally:
        await set_message_terms(rig, MESSAGE_RESERVE, MESSAGE_CEILING)

    # The portal takes orders again.
    order = spoke.deposit(USER_KEY, paid, rails.to_tao(USER))
    answer = await rig.answer(order)
    assert not answer["refund"] and answer["asset"] == "tao", answer
    await wait_for("the portal's order slots", slots_are_back)
    show(
        "no order without a slot",
        f"the answers to {ORDER_SLOTS} orders were deferred, and with them the portal's "
        f"{ORDER_SLOTS} order slots. The portal refused the next order, and the payer kept "
        f"the {units(paid, 6)} USDC. When the hub paid for the answers they left, the slots "
        f"came back, and the next order settled",
    )


async def a_message_that_the_gateway_cannot_hand_over(rig: Rig) -> None:
    """The gateway cannot hand a message to the transport: here, because its guardian
    paused it. The message stays at the head of its queue. When it has been there for long
    enough, the keeper defers it on the hub, with a key that has no power. The message
    leaves when the gateway can send it."""
    chain, spoke, hub = rig.chain, rig.spoke, rig.hub_evm
    timelock = load_json(DEPLOYMENTS / "hub.local.json")["timelock"]
    stale_after = int(await chain.constant(bt.constants.Rails.StaleAfter))
    before = await rig.transport()
    tokens = spoke.balance(spoke.tao, USER)

    send(hub, GUARDIAN_KEY, rig.gateway, calldata("pause()", [], []))
    try:
        # A payment that starts on Bittensor queues a message. The gateway cannot take it.
        exit_ = bt.intents.BridgeOut(
            bridge=SPOKE_BRIDGE, pay_with="tao", amount=0.01, receive="tao", to_address=USER
        )
        result = await chain.execute(exit_, dev_wallet())
        assert result.success, result.message

        async def deferred():
            now = await rig.transport()
            return now.deferred if len(now.deferred) > len(before.deferred) else None

        # The keeper is not the gateway and not root. It defers the message when the
        # message has been at the head of the queue for long enough, and not before.
        (message,) = await wait_for("the keeper to defer the stale message", deferred)
        assert spoke.balance(spoke.tao, USER) == tokens
    finally:
        # The owner resumes the gateway: on the rig, a multisig through a timelock with
        # no delay.
        resume = bytes.fromhex(calldata("unpause()", [], [])[2:])
        types = ["address", "uint256", "bytes", "bytes32", "bytes32"]
        values = [rig.gateway, 0, resume, bytes(32), bytes(32)]
        schedule = "schedule(address,uint256,bytes,bytes32,bytes32,uint256)"
        send(hub, MULTISIG_KEY, timelock, calldata(schedule, [*types, "uint256"], [*values, 0]))
        execute = "execute(address,uint256,bytes,bytes32,bytes32)"
        send(hub, MULTISIG_KEY, timelock, calldata(execute, types, values))

    async def arrived():
        return spoke.balance(spoke.tao, USER) - tokens

    # The keeper sends the deferred message, and the payment arrives.
    amount = await wait_for("the deferred payment", arrived)
    after = await rig.transport()
    assert after.sent == before.sent + 1 and not after.deferred, (before, after)
    show(
        "a message that the gateway cannot hand over",
        f"the gateway was paused, and message {message} stayed at the head of the queue. "
        f"After {stale_after} blocks there, the keeper deferred it on the hub, as anyone "
        f"can. When the gateway resumed, the keeper sent the message: {units(amount)} TAO "
        f"arrived",
    )


async def invariants(rig: Rig) -> None:
    chain, spoke = rig.chain, rig.spoke
    storage = bt.storage

    # Every tUSD the bridge has outstanding is backed by the stablecoin in the vault.
    outstanding = int(await chain.query(storage.Tusd.BridgeOutstanding, [SPOKE_BRIDGE]) or 0)
    locked = spoke.balance(spoke.stable, spoke.vault)
    assert locked * STABLE_SCALE >= outstanding, (locked, outstanding)

    # Canonical TAO on the spoke is what the hub minted, and the hub's reserve covers it.
    supply = int(await chain.query(storage.Rails.TaoSupply, [SPOKE_BRIDGE]) or 0)
    assert spoke.total_supply(spoke.tao) == supply
    reserve = (await chain.balances.balance(RESERVE_ACCOUNT)).rao
    assert reserve >= supply, (reserve, supply)

    # Each alpha token's supply on the spoke is the shares the hub minted for the spoke.
    for market in rig.markets:
        shares = int(
            await chain.query(storage.Rails.AlphaSupply, [market["netuid"], SPOKE_BRIDGE]) or 0
        )
        assert spoke.total_supply(market["token"]) == shares, market

    show(
        "invariants hold",
        f"{units(locked, 6)} USDC locked for {units(outstanding)} tUSD; "
        f"{units(supply)} canonical TAO backed by {units(reserve)} TAO",
    )


async def main() -> None:
    async with client() as chain:
        rig = Rig(chain)
        chutes, lium = rig.markets
        first_order = rig.spoke.next_order()
        fees_before = rig.spoke.native_balance(rig.spoke.portal)
        start = await rig.transport()
        rig.spoke.mint_stable(USER_KEY, USER, 1_000 * USDC)
        show("faucet", f"1,000 USDC for {USER}")

        await buy_and_sell_alpha(rig, chutes)
        await buy_and_sell_tao(rig)
        await buy_credits(rig, lium)
        await refund(rig, chutes)
        await leave_from_bittensor(rig)
        await who_paid(rig, first_order, fees_before, start)
        await a_charge_that_is_not_paid_for(rig)
        await no_order_without_a_slot(rig)
        await a_message_that_the_gateway_cannot_hand_over(rig)
        await invariants(rig)
    show("passed")


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except (AssertionError, TimeoutError) as error:
        sys.exit(f"[e2e] FAILED: {error!r}")
