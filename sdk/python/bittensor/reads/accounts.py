"""Account reads: balances, proxies, multisig operations, coldkey swaps."""

from __future__ import annotations

import asyncio
from typing import Optional

from .._generated import constants
from .._generated import storage as st
from ..balance import Balance
from .base import read


@read(
    "balance",
    {"coldkey_ss58": "string"},
    category="Accounts & keys",
    param_docs={"coldkey_ss58": "Coldkey whose free balance to read."},
)
async def balance(view, coldkey_ss58: str) -> Balance:
    """Free TAO balance of a coldkey address."""
    account = await view.query(st.System.Account, [coldkey_ss58])
    return Balance.from_rao(int(account["data"]["free"]))


@read(
    "balances",
    {"coldkey_ss58s": "array"},
    category="Accounts & keys",
    param_docs={"coldkey_ss58s": "Coldkeys whose free balances to read."},
)
async def balances(view, coldkey_ss58s: list[str]) -> dict[str, Balance]:
    """Free TAO balance for several coldkey addresses in one batched request."""
    accounts = await view.query_batch(st.System.Account, [[a] for a in coldkey_ss58s])
    return {
        address: Balance.from_rao(int(account["data"]["free"]) if account else 0)
        for address, account in zip(coldkey_ss58s, accounts)
    }


@read("existential_deposit", {}, category="Accounts & keys")
async def existential_deposit(view) -> Balance:
    """Minimum balance an account must keep to stay alive."""
    value = await view.constant(constants.Balances.ExistentialDeposit)
    return Balance.from_rao(int(value))


@read(
    "proxies",
    {"coldkey_ss58": "string"},
    category="Accounts & keys",
    param_docs={"coldkey_ss58": "Account whose proxy delegations to list."},
)
async def proxies(view, coldkey_ss58: str) -> dict:
    """Proxy delegations of an account: who may sign on its behalf, and the reserved deposit."""
    value = await view.query(st.Proxy.Proxies, [coldkey_ss58])
    delegations, deposit = value or ([], 0)
    return {
        "proxies": [
            {
                "delegate": str(d["delegate"]),
                "proxy_type": str(d["proxy_type"]),
                "delay": int(d["delay"]),
            }
            for d in delegations
        ],
        "deposit": Balance.from_rao(int(deposit)),
    }


@read(
    "coldkey_swap_announcement",
    {"coldkey_ss58": "string"},
    category="Accounts & keys",
    param_docs={"coldkey_ss58": "Coldkey whose pending swap announcement to check."},
)
async def coldkey_swap_announcement(view, coldkey_ss58: str) -> Optional[dict]:
    """A coldkey's pending swap announcement (execute block, new-key hash, disputed), or None.

    This is the ``swap-check`` status: ``ColdkeySwapAnnouncements`` stores the block
    at which the swap becomes executable and the BlakeTwo256 hash committed to.
    """
    value, disputed = await asyncio.gather(
        view.query(st.SubtensorModule.ColdkeySwapAnnouncements, [coldkey_ss58]),
        view.query(st.SubtensorModule.ColdkeySwapDisputes, [coldkey_ss58]),
    )
    if not value:
        return None
    execute_block, new_coldkey_hash = value
    return {
        "execute_block": int(execute_block),
        "new_coldkey_hash": str(new_coldkey_hash),
        "disputed": bool(disputed),
        "dispute_block": int(disputed) if disputed else None,
    }


@read(
    "multisig",
    {"account_ss58": "string", "call_hash": "string"},
    category="Accounts & keys",
    param_docs={
        "account_ss58": "Multisig account the pending operation belongs to.",
        "call_hash": "Hash of the multisig call, as 0x-prefixed or bare hex.",
    },
)
async def multisig(view, account_ss58: str, call_hash: str) -> Optional[dict]:
    """A pending multisig operation (opening timepoint, approvals, depositor), or None."""
    ch = call_hash if call_hash.startswith("0x") else "0x" + call_hash
    value = await view.query(st.Multisig.Multisigs, [account_ss58, ch])
    if not value:
        return None
    when = value.get("when") or {}
    return {
        "timepoint": {"height": int(when.get("height", 0)), "index": int(when.get("index", 0))},
        "deposit": Balance.from_rao(int(value.get("deposit") or 0)),
        "depositor": str(value.get("depositor")),
        "approvals": [str(a) for a in value.get("approvals") or []],
    }
