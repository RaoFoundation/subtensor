"""The rails, end to end, against the local rig.

A person on the spoke holds only the stablecoin and a wallet. This script does what that
person does and checks what the two chains say after each step:

1. buy a subnet's alpha token: deposit -> stake -> the token is in the wallet;
2. sell it: the stake is released -> the stablecoin is back;
3. buy and sell canonical TAO;
4. buy service credits: the gamma is burned at the service, and the gateway emits the
   event, keyed by the buyer's address, that the service watches;
5. an order that cannot meet its minimum returns in full, in the stablecoin;
6. a holder of TAO on Bittensor leaves through the bridge.

It ends with the invariants that hold between the chains at any time.
"""

from __future__ import annotations

import asyncio
import sys

from eth_abi import decode as abi_decode
from eth_abi import encode as abi_encode
from eth_utils import keccak
from rig import (
    ANSWER_FEE,
    EXIT_FEE,
    HUB_RPC,
    MULTISIG,
    RESERVE_ACCOUNT,
    SPOKE_BRIDGE,
    SPOKE_DOMAIN,
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
    manifest,
    mirror,
    vault_account,
    wait_for,
)

import bittensor as bt
from bittensor import rails
from bittensor.evm.addresses import ss58_to_pubkey
from bittensor.evm.rpc import EvmRpc

CREDITS_PURCHASED = keccak(text="CreditsPurchased(address,uint16,bytes32,uint32,uint64,uint64)")
# The Rails precompile on Bittensor's EVM.
RAILS_PRECOMPILE = "0x0000000000000000000000000000000000000815"
# One unit of the stablecoin in the hub's 9 decimals.
STABLE_SCALE = UNIT // USDC


def show(step: str, detail: str = "") -> None:
    print(f"[e2e] {step}" + (f": {detail}" if detail else ""), flush=True)


def units(amount: int, decimals: int = 9) -> str:
    return f"{amount / 10**decimals:,.6f}".rstrip("0").rstrip(".")


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
    gateway = mirror(rig.gateway)
    gateway_before = (await chain.balances.balance(gateway)).rao
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
    # The exit paid the gateway for its message, on top of what it sent.
    assert (await chain.balances.balance(gateway)).rao == gateway_before + EXIT_FEE
    show(
        "left Bittensor",
        f"1 TAO -> {units(held)} tUSD -> {units(arrived, 6)} USDC on the spoke, "
        f"and {units(EXIT_FEE)} TAO to the gateway",
    )


def answer_fees(rig: Rig, first_order: int, fees_before: int) -> None:
    """Each order paid for its answer. The fees wait in the portal for their recipient."""
    spoke = rig.spoke
    orders = spoke.next_order() - first_order
    collected = spoke.native_balance(spoke.portal) - fees_before
    assert orders > 0 and collected == orders * ANSWER_FEE, (orders, collected)

    recipient_before = spoke.native_balance(MULTISIG)
    waiting = spoke.native_balance(spoke.portal)
    # Anyone may send them on. They can only go to the recipient.
    spoke.send(USER_KEY, spoke.portal, calldata("sweepFees()", [], []))
    assert spoke.native_balance(spoke.portal) == 0
    assert spoke.native_balance(MULTISIG) == recipient_before + waiting
    show(
        "answer fees",
        f"{orders} orders paid {units(collected, 18)} ETH for their answers, "
        "swept to the fee recipient",
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
        rig.spoke.mint_stable(USER_KEY, USER, 1_000 * USDC)
        show("faucet", f"1,000 USDC for {USER}")

        await buy_and_sell_alpha(rig, chutes)
        await buy_and_sell_tao(rig)
        await buy_credits(rig, lium)
        await refund(rig, chutes)
        await leave_from_bittensor(rig)
        answer_fees(rig, first_order, fees_before)
        await invariants(rig)
    show("passed")


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except (AssertionError, TimeoutError) as error:
        sys.exit(f"[e2e] FAILED: {error!r}")
