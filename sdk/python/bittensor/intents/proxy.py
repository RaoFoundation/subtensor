"""Proxy delegation: let a low-value key sign on behalf of the coldkey.

The security model: sign ``add_proxy`` once from the coldkey (granting a scoped
``proxy_type`` to a delegate key), then keep the coldkey offline. The delegate
executes any intent with ``proxy_for=<coldkey ss58>`` (see ``Executor``), which
wraps the call in ``Proxy.proxy`` so it dispatches with the coldkey's origin.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional

from .._generated import calls
from .base import Intent
from .registry import build as build_intent
from .registry import register

# Variants of the runtime's ProxyType enum (subtensor/common/src/lib.rs).
PROXY_TYPES = (
    "Any",
    "Owner",
    "NonCritical",
    "NonTransfer",
    "Senate",
    "NonFungible",
    "Triumvirate",
    "Governance",
    "Staking",
    "Registration",
    "Transfer",
    "SmallTransfer",
    "RootWeights",
    "ChildKeys",
    "SudoUncheckedSetCode",
    "SwapHotkey",
    "SubnetLeaseBeneficiary",
    "RootClaim",
)


# CLI-facing choice type: Typer/Click render the variants in --help and shell
# completion. str-mixed so members compare and format as the plain variant name.
ProxyTypeChoice = Enum("ProxyTypeChoice", [(name, name) for name in PROXY_TYPES], type=str)


def check_proxy_type(proxy_type: str) -> str:
    if proxy_type not in PROXY_TYPES:
        raise ValueError(
            f"unknown proxy type {proxy_type!r}; expected one of: {', '.join(PROXY_TYPES)}"
        )
    return proxy_type


PROXY_TYPE_HELP = (
    "Scope of calls the delegation covers. One of: "
    + ", ".join(PROXY_TYPES)
    + ". Triumvirate, Senate, Governance, and RootWeights are deprecated on "
    "the current runtime: they deny all calls, so a proxy of those types can "
    "dispatch nothing. Prefer the narrowest type that covers your use; Any "
    "can do everything the account can, including transfers."
)

DELAY_HELP = (
    "Announcement delay in blocks: the delegate must announce each call and wait "
    "this long before executing it, giving you time to veto. 0 executes immediately."
)


@register
@dataclass
class AddProxy(Intent):
    """Authorize a delegate key to sign calls on this account's behalf.

    The foundation of the keep-the-coldkey-offline setup: sign this once from
    the coldkey, then let the delegate submit day-to-day calls with
    ``proxy_for=<this account>``. The chain reserves a small deposit from the
    signer per delegation (returned on removal). The delegate can act within
    ``proxy_type`` immediately (or after announcing, if ``delay`` > 0) — grant
    only to keys you control or fully trust.
    """

    op = "add_proxy"
    signer = "coldkey"
    wraps = (("Proxy", "add_proxy"),)

    delegate_ss58: str = field(
        metadata={"help": "Key that will be allowed to sign for this account."}
    )
    proxy_type: str = field(default="Staking", metadata={"help": PROXY_TYPE_HELP})
    delay: int = field(default=0, metadata={"help": DELAY_HELP})

    def __post_init__(self):
        check_proxy_type(self.proxy_type)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Proxy.add_proxy(
                delegate=self.delegate_ss58, proxy_type=self.proxy_type, delay=self.delay
            )
        )

    def summary(self) -> str:
        return f"add {self.proxy_type} proxy {self.delegate_ss58} (delay {self.delay})"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out = ["reserves a proxy deposit from the signer (returned on removal)"]
        if self.proxy_type == "Any":
            out.append(
                "an Any proxy can do everything this account can, including transfers; "
                "prefer a narrower proxy type"
            )
        return out


@register
@dataclass
class RemoveProxy(Intent):
    """Revoke one proxy delegation.

    The (delegate, proxy_type, delay) triple must match the original
    ``add_proxy`` exactly — a mismatch fails with NotFound rather than removing
    a different delegation. The proxy deposit reserved at add time is returned
    to the signer. Check current delegations with ``btcli query proxies``.
    """

    op = "remove_proxy"
    signer = "coldkey"
    wraps = (("Proxy", "remove_proxy"),)

    delegate_ss58: str = field(metadata={"help": "Delegate whose authorization to revoke."})
    proxy_type: str = field(
        default="Staking",
        metadata={"help": "Type the delegation was granted with (must match exactly)."},
    )
    delay: int = field(
        default=0,
        metadata={"help": "Delay the delegation was granted with (must match exactly)."},
    )

    def __post_init__(self):
        check_proxy_type(self.proxy_type)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Proxy.remove_proxy(
                delegate=self.delegate_ss58, proxy_type=self.proxy_type, delay=self.delay
            )
        )

    def summary(self) -> str:
        return f"remove {self.proxy_type} proxy {self.delegate_ss58} (delay {self.delay})"


@register
@dataclass
class RemoveProxies(Intent):
    """Revoke every proxy delegation for the signing account at once.

    A cleanup/panic switch: all delegates lose access in one call and all proxy
    deposits are returned. Careful if this account spawned pure proxies — they
    are controlled *through* delegations, so removing everything can strand them
    permanently (there is no key to recover a pure proxy with).
    """

    op = "remove_proxies"
    signer = "coldkey"
    wraps = (("Proxy", "remove_proxies"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Proxy.remove_proxies())

    def summary(self) -> str:
        return "remove ALL proxies for the signer"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["revokes every delegate at once; pure-proxy accounts would become inaccessible"]


@register
@dataclass
class CreatePureProxy(Intent):
    """Create a pure proxy: a fresh keyless account controlled via delegation.

    The chain derives a new address from the signer, ``proxy_type``, ``index``,
    and the creation block. Nobody holds its private key — the spawner controls
    it purely through the proxy relationship, which makes it useful as a
    disposable or role-scoped account (e.g. a Staking-only treasury). Record the
    creation block and extrinsic index (shown in the result): ``kill_pure_proxy``
    needs them to close the account later. Losing the spawner key means losing
    the pure proxy and anything it holds.
    """

    op = "create_pure_proxy"
    signer = "coldkey"
    wraps = (("Proxy", "create_pure"),)

    proxy_type: str = field(default="Staking", metadata={"help": PROXY_TYPE_HELP})
    delay: int = field(default=0, metadata={"help": DELAY_HELP})
    index: int = field(
        default=0,
        metadata={
            "help": "Disambiguator so one signer can create several pure proxies in "
            "one block; also part of the derived address. Keep 0 unless batching."
        },
    )

    def __post_init__(self):
        check_proxy_type(self.proxy_type)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Proxy.create_pure(proxy_type=self.proxy_type, delay=self.delay, index=self.index)
        )

    def summary(self) -> str:
        return f"create pure {self.proxy_type} proxy (index {self.index}, delay {self.delay})"


@register
@dataclass
class KillPureProxy(Intent):
    """Close a pure proxy account and return its reserved deposit.

    Must be signed *by the pure proxy itself* (i.e. dispatched through it with
    ``proxy_for=<pure proxy>``), and the parameters must reproduce the exact
    creation: spawner, type, index, plus the block height and extrinsic index
    of the ``create_pure_proxy`` call. Irreversible — any funds left in the
    account become permanently inaccessible, so empty it first.
    """

    op = "kill_pure_proxy"
    signer = "coldkey"
    wraps = (("Proxy", "kill_pure"),)

    spawner_ss58: str = field(metadata={"help": "Account that originally created the pure proxy."})
    proxy_type: str = field(
        default="Staking",
        metadata={"help": "Type the pure proxy was created with (must match exactly)."},
    )
    index: int = field(
        default=0, metadata={"help": "Index the pure proxy was created with (must match exactly)."}
    )
    height: int = field(
        default=0, metadata={"help": "Block number of the creating create_pure_proxy call."}
    )
    ext_index: int = field(
        default=0,
        metadata={"help": "Extrinsic index of the creating call within that block."},
    )

    def __post_init__(self):
        check_proxy_type(self.proxy_type)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Proxy.kill_pure(
                spawner=self.spawner_ss58,
                proxy_type=self.proxy_type,
                index=self.index,
                height=self.height,
                ext_index=self.ext_index,
            )
        )

    def summary(self) -> str:
        return f"kill pure proxy index {self.index} for {self.spawner_ss58}"


@register
@dataclass
class ExecuteProxyAnnounced(Intent):
    """Execute a proxy call that was announced and has passed its delay.

    The delayed-proxy flow: a delegate added with ``delay`` > 0 first announces
    the call's hash, waits out the delay (during which the real account can
    veto), then anyone may dispatch the announced call with this intent. The
    inner call is given as an intent op name plus its arguments and must hash
    to exactly what was announced. Fails if the delay has not elapsed or no
    matching announcement exists.
    """

    op = "execute_proxy_announced"
    signer = "coldkey"
    wraps = (("Proxy", "proxy_announced"),)

    delegate_ss58: str = field(metadata={"help": "Delegate that made the announcement."})
    real_ss58: str = field(
        metadata={"help": "Account the call executes as (the delegation's grantor)."}
    )
    inner_op: str = field(
        metadata={"help": "Intent op name of the announced call (see btcli tools for the catalog)."}
    )
    inner_args: dict = field(
        metadata={"help": "Arguments of the announced call, as a JSON object."}
    )
    force_proxy_type: Optional[str] = field(
        default=None,
        metadata={
            "help": "Require the delegation to be exactly this proxy type instead of "
            "accepting any type that covers the call."
        },
    )

    async def build(self, substrate, wallet: Any):
        inner = await build_intent(self.inner_op, self.inner_args).build(substrate, wallet)
        inner_call = inner.call if hasattr(inner, "call") else inner
        return await substrate.compose(
            calls.Proxy.proxy_announced(
                delegate=self.delegate_ss58,
                real=self.real_ss58,
                force_proxy_type=self.force_proxy_type,
                call=inner_call,
            )
        )

    def summary(self) -> str:
        return f"execute announced {self.inner_op} as {self.real_ss58} via {self.delegate_ss58}"
