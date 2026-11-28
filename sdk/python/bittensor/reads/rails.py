"""Canonical rails reads: tUSD, the tUSD/TAO pool, the token registry, and bridge orders."""

from __future__ import annotations

from typing import Optional

from .._generated import runtime_apis as api
from .._generated import storage as st
from ..balance import Balance
from ..intents._money import tusd_amount, tusd_from_units, tusd_units
from ..result import BittensorError, chain_error_from_dispatch
from .base import read, utf8_text

# The scale of prices (tUSD per TAO) and of an alpha token's rate.
_PRICE_SCALE = 10**9
_RATE_SCALE = 10**18
_PPM = 1_000_000

# What became of an order, by the variant of its receipt.
_OUTCOMES = {
    "Settled": "settled",
    "Refunded": "refunded",
    "Rejected": "rejected",
    "Skipped": "skipped",
}


def _price(value) -> Optional[float]:
    return None if value is None else int(value) / _PRICE_SCALE


def _asset(asset) -> tuple[str, Optional[int]]:
    """A registry asset as its kind and, for an alpha token, its netuid."""
    if isinstance(asset, dict):
        (kind, value), *_ = asset.items()
        return kind.lower(), None if value is None else int(value)
    return str(asset).lower(), None


@read(
    "tusd_balance",
    {"coldkey_ss58": "string"},
    category="Canonical rails",
    param_docs={"coldkey_ss58": "Account holding the tUSD."},
)
async def tusd_balance(view, coldkey_ss58: str) -> str:
    """The tUSD an account holds, as an exact decimal string.

    tUSD is the chain's dollar: bridges mint it one for one against a
    stablecoin they lock on another chain.
    """
    return tusd_from_units(await view.query(st.Tusd.Balances, [coldkey_ss58]) or 0)


@read("canonical_pool", {}, category="Canonical rails")
async def canonical_pool(view) -> dict:
    """The canonical tUSD/TAO pool: its reserves, prices, fee, caps and whether it is on.

    `price_usd` is the pool's spot price of a TAO and `twap_usd` its price
    averaged over the last period, which values credits; `twap_usd` is None
    until the pool has a full period of history. `fee` is the swap fee as a
    fraction. One swap may pay in at most `max_swap_fraction` of the paying
    reserve, and each direction at most `window_fraction` of it over
    `window_blocks` blocks.
    """
    view = await view.at()
    enabled = await view.query(st.CanonicalPool.Enabled)
    tao = int(await view.query(st.CanonicalPool.ReserveTao) or 0)
    tusd = int(await view.query(st.CanonicalPool.ReserveTusd) or 0)
    shares = int(await view.query(st.CanonicalPool.TotalShares) or 0)
    fee = int(await view.query(st.CanonicalPool.Fee) or 0)
    caps = await view.query(st.CanonicalPool.Caps)
    spot = await view.runtime(api.CanonicalPoolApi.spot_price, [])
    twap = await view.runtime(api.CanonicalPoolApi.twap_price, [])
    return {
        "enabled": bool(enabled),
        "reserve_tao": view.balance(tao, 0),
        "reserve_tusd": tusd_from_units(tusd),
        "total_shares": shares,
        "price_usd": _price(spot),
        "twap_usd": _price(twap),
        "fee": fee / _PPM,
        "max_swap_fraction": int(caps["per_swap"]) / _PPM,
        "window_fraction": int(caps["per_window"]) / _PPM,
        "window_blocks": int(caps["window"]),
    }


@read(
    "pool_shares",
    {"coldkey_ss58": "string"},
    category="Canonical rails",
    param_docs={"coldkey_ss58": "Account holding the shares."},
)
async def pool_shares(view, coldkey_ss58: str) -> dict:
    """An account's shares of the canonical pool and what they redeem now.

    `tao` and `tusd` are the parts of the reserves that `shares` redeem,
    which grow with the swap fees the pool earns.
    """
    view = await view.at()
    shares = int(await view.query(st.CanonicalPool.Shares, [coldkey_ss58]) or 0)
    total = int(await view.query(st.CanonicalPool.TotalShares) or 0)
    tao = int(await view.query(st.CanonicalPool.ReserveTao) or 0)
    tusd = int(await view.query(st.CanonicalPool.ReserveTusd) or 0)
    return {
        "shares": shares,
        "total_shares": total,
        "tao": view.balance(tao * shares // total if total else 0, 0),
        "tusd": tusd_from_units(tusd * shares // total if total else 0),
    }


@read(
    "quote_pool_swap",
    {"sell": "string", "amount": "number"},
    category="Canonical rails",
    param_docs={
        "sell": "The asset sold: tusd or tao.",
        "amount": "How much of the sold asset.",
    },
)
async def quote_pool_swap(view, sell: str, amount: float) -> dict:
    """What a swap in the canonical pool would pay now, after its fee.

    `receive` is in the other asset: tUSD as an exact decimal string when TAO
    is sold, TAO when tUSD is sold. The pool's caps can still refuse a swap of
    this size; `swap_pool` reports that when it plans the call.
    """
    asset = str(sell).strip().lower()
    if asset == "tao":
        units = Balance.from_tao(amount).rao
        quote = await view.runtime(api.CanonicalPoolApi.quote_tao_for_tusd, [units])
        receive = None if quote is None else tusd_from_units(quote)
    elif asset == "tusd":
        units = tusd_units(tusd_amount(amount))
        quote = await view.runtime(api.CanonicalPoolApi.quote_tusd_for_tao, [units])
        receive = None if quote is None else view.balance(int(quote), 0)
    else:
        raise BittensorError(f"sell is tusd or tao; got {sell!r}.")
    if receive is None:
        raise BittensorError("the pool cannot make this swap: it is empty, or pays nothing")
    return {"sell": asset, "amount": amount, "receive": receive}


@read("bridges", {}, category="Canonical rails")
async def bridges(view) -> list[dict]:
    """Every bridge that may mint tUSD: its chain, portal, caps and what it has outstanding.

    `outstanding_tusd` is the tUSD the bridge minted and has not burned: the
    stablecoin its vault must hold. `mint_cap_tusd` bounds it. `tao_supply`
    and `tao_cap` are the canonical TAO on the bridge's chain and its cap.

    The last five fields say who pays for the hub's messages to the bridge's
    chain. `message_account` holds the TAO for them, and `message_budget` is
    what it can pay: its balance less the existential deposit. `message_reserve`
    is what the hub sets aside for each message, or None while it is not set,
    and `message_ceiling` is the most the hub pays for one: above the reserve
    it pays from what the account can spare. `messages_waiting` is how many
    wait for the gateway. The hub takes an order of the bridge only while the
    account holds the reserve for every waiting message and one more. Anyone
    can pay into the account (`Rails.fund_messages`).
    """
    view = await view.at()
    rows = await view.query_map(st.Tusd.Bridges)
    records = []
    for key, bridge in rows:
        bridge_id = int(key[0] if isinstance(key, (list, tuple)) else key)
        route = bridge["route"]
        outstanding = await view.query(st.Tusd.BridgeOutstanding, [bridge_id])
        tao_supply = await view.query(st.Rails.TaoSupply, [bridge_id])
        tao_cap = await view.query(st.Rails.TaoCap, [bridge_id])
        next_order = await view.query(st.Rails.NextOrder, [bridge_id])
        reserve = await view.query(st.Rails.MessageReserve, [bridge_id])
        ceiling = await view.query(st.Rails.MessageCeiling, [bridge_id])
        account = await view.runtime(api.RailsApi.message_account, [bridge_id])
        budget = await view.runtime(api.RailsApi.message_budget, [bridge_id])
        queued = await view.query(st.Rails.NextMessage, [bridge_id])
        taken = await view.query(st.Rails.NextOutbound, [bridge_id])
        records.append(
            {
                "bridge": bridge_id,
                "enabled": bool(bridge["enabled"]),
                "origin": route["origin"],
                "domain": int(route["domain"]),
                "portal": str(route["portal"]),
                "stablecoin": str(route["asset"]),
                "outstanding_tusd": tusd_from_units(outstanding or 0),
                "mint_cap_tusd": tusd_from_units(bridge["mint_cap"]),
                "tao_supply": view.balance(int(tao_supply or 0), 0),
                "tao_cap": view.balance(int(tao_cap or 0), 0),
                "next_order": int(next_order or 0),
                "message_reserve": None if reserve is None else view.balance(int(reserve), 0),
                "message_ceiling": view.balance(int(ceiling or 0), 0),
                "message_account": str(account),
                "message_budget": view.balance(int(budget or 0), 0),
                "messages_waiting": int(queued or 0) - int(taken or 0),
            }
        )
    return sorted(records, key=lambda record: record["bridge"])


@read("rails_tokens", {}, category="Canonical rails")
async def rails_tokens(view) -> list[dict]:
    """The registry of canonical tokens: which contract on which chain stands for which asset.

    One row per bridge and asset. `asset` is `stable`, `tao` or `alpha`, with
    the subnet of an alpha token in `netuid`. For an alpha token, `rate` is
    the alpha that one token is worth now, and `supply` the tokens outstanding
    on that bridge's chain; a token never rebases, its rate grows. `rate` is
    None once the subnet was dissolved and the token only redeems TAO.
    """
    view = await view.at()
    rows = await view.query_map(st.Rails.Tokens)
    records = []
    for key, token in rows:
        bridge_id, asset = int(key[0]), key[1]
        kind, netuid = _asset(asset)
        record = {
            "bridge": bridge_id,
            "asset": kind,
            "netuid": netuid,
            "symbol": utf8_text(token["symbol"]),
            "name": utf8_text(token["name"]),
            "address": str(token["address"]),
            "rate": None,
            "supply": None,
        }
        if netuid is not None:
            rate = await view.runtime(api.RailsApi.alpha_rate, [netuid])
            supply = await view.query(st.Rails.AlphaSupply, [netuid, bridge_id])
            record["rate"] = None if rate is None else int(rate) / _RATE_SCALE
            record["supply"] = tusd_from_units(supply or 0)
        records.append(record)
    return sorted(records, key=lambda r: (r["bridge"], r["asset"], r["netuid"] or 0))


@read(
    "bridge_order",
    {"bridge": "integer", "nonce": "integer"},
    category="Canonical rails",
    param_docs={
        "bridge": "Id of the bridge the order came through.",
        "nonce": "The order's sequence number at its portal.",
    },
)
async def bridge_order(view, bridge: int, nonce: int) -> dict:
    """What became of an order that a portal sent to the hub.

    `status` is `pending` (the hub has not taken it yet), `settled`,
    `refunded` (the portal returns what it took; `error` says why),
    `rejected`, `skipped`, or `unknown` when the hub has taken the order but
    no longer keeps its receipt. `amount_out` is what a settled order
    delivered, with 9 decimals, and `block` where the hub disposed of it.
    """
    view = await view.at()
    next_order = int(await view.query(st.Rails.NextOrder, [bridge]) or 0)
    receipt = await view.query(st.Rails.Receipts, [bridge, nonce])
    record = {
        "bridge": bridge,
        "nonce": nonce,
        "status": "pending" if nonce >= next_order else "unknown",
        "amount_out": None,
        "error": None,
        "block": None,
    }
    if receipt is None:
        return record
    outcome = receipt["outcome"]
    kind, value = next(iter(outcome.items())) if isinstance(outcome, dict) else (outcome, None)
    record["status"] = _OUTCOMES.get(str(kind), str(kind).lower())
    record["block"] = int(receipt["block"])
    if isinstance(value, dict):
        if "amount_out" in value:
            record["amount_out"] = tusd_from_units(value["amount_out"])
        if "error" in value:
            record["error"] = str(chain_error_from_dispatch(value["error"]))
    return record


@read(
    "credits_spent",
    {"netuid": "integer", "service_hotkey_ss58": "string", "payer_address": "string"},
    category="Canonical rails",
    param_docs={
        "netuid": "Subnet of the service.",
        "service_hotkey_ss58": "Hotkey of the service.",
        "payer_address": "0x address the credits were bought for.",
    },
)
async def credits_spent(view, netuid: int, service_hotkey_ss58: str, payer_address: str) -> str:
    """The gamma that payments through the rails spent at a service for an address.

    An exact decimal string. It only grows, until the subnet is dissolved. A
    service credits an address by the growth of this total, read at a finalized
    block.
    """
    spent = await view.query(
        st.SubtensorModule.GammaSpentByPayer,
        [netuid, service_hotkey_ss58, {"Evm": payer_address}],
    )
    return tusd_from_units(spent or 0)
