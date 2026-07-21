"""The async entry point: ``Client``.

A ``Client`` owns one chain connection and has two verbs, each a projection of
the chain's own metadata:

- **Read state.** Generic ``query`` / ``query_map`` / ``runtime`` / ``constant``
  over the generated descriptors (``bittensor.storage`` / ``runtime_api`` /
  ``constants``), plus typed namespaces (``subnets``, ``staking``, ``balances``,
  ``delegation``, ... — one per read category) that decode or aggregate.
- **Submit an intent.** ``plan`` / ``execute`` / ``execute_tool`` — mutations as
  serializable data, previewed with fee + effects, gated by an optional
  :class:`Policy`.

Open with ``async with`` or ``connect()``.
"""

from __future__ import annotations

import asyncio
import contextlib
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, AsyncIterator, Optional, Union

from . import config
from . import reads as read_registry
from ._generated import storage as _st
from ._substrate import RpcSubstrate, Substrate
from ._transport.contract import UnsignedExtrinsic
from .balance import Balance
from .executor import Executor
from .intents import Intent, Plan, Policy
from .multisig import Multisig
from .namespaces import (
    Balances,
    Chain,
    Collateral,
    Delegation,
    Epochs,
    Hyperparameters,
    Identity,
    Leasing,
    Locks,
    Neurons,
    Prices,
    Staking,
    Subnets,
    Weights,
)
from .result import BittensorError, ChainError, ExtrinsicResult
from .settings import (
    DEFAULT_NETWORK,
    default_archive_endpoints,
    default_fallback_endpoints,
    explorer_block_url,
    resolve_endpoint,
)
from .signing import WalletLike
from .snapshot import Snapshot
from .sp_core import ss58_decode

# A generated descriptor is a (container, name) pair (see bittensor/_generated);
# a plain 2-tuple of strings works for items missing from the generated catalog.
Descriptor = tuple[str, str]

# A moment in time: datetime, unix timestamp, or ISO-8601 string.
WhenLike = Union[datetime, str, int, float]

# Slot duration of a fast-blocks chain (local/e2e testing mode), in seconds.
FAST_BLOCK_TIME = 0.25


@dataclass
class BlockHeader:
    """A new block seen on a subscription (``client.blocks()``)."""

    number: int
    parent_hash: Optional[str]
    raw: dict = field(repr=False)


@dataclass
class BlockInfo:
    """One block's metadata and contents (``client.block_info()``)."""

    number: int
    hash: Optional[str]
    timestamp: datetime  # UTC, from the block's Timestamp.set inherent
    explorer_url: Optional[str]
    header: dict = field(repr=False)
    extrinsics: list = field(repr=False)  # decoded values; None for undecodable entries


class _AddressView:
    """Keypair-shaped public view of a bare ss58 address, for fee/weight quotes.

    Fee estimation signs with a zeroed signature, so only the public parts are
    ever read (``sp_core.ss58_decode`` recovers the public key).
    """

    crypto_type = 1  # sr25519

    def __init__(self, address: str):
        self.ss58_address = address
        self.public_key = bytes(ss58_decode(address))


@dataclass
class EpochEvent:
    """An epoch run observed by ``client.wait_for_epoch()``."""

    netuid: int
    block: int  # block at which the epoch ran (the new LastEpochBlock)
    previous: int  # LastEpochBlock before this run


def _as_unix(when: WhenLike) -> float:
    """A unix timestamp from a datetime, ISO-8601 string, or numeric timestamp.

    Naive datetimes and ISO strings without an offset are taken as UTC — chain
    time is global, so local-time guessing would be a footgun.
    """
    if isinstance(when, datetime):
        if when.tzinfo is None:
            when = when.replace(tzinfo=timezone.utc)
        return when.timestamp()
    if isinstance(when, (int, float)):
        return float(when)
    try:
        return _as_unix(datetime.fromisoformat(str(when).strip().replace("Z", "+00:00")))
    except ValueError:
        raise BittensorError(
            f"cannot parse moment {when!r}; use a datetime, a unix timestamp, or "
            "ISO-8601 like '2026-07-08T12:00Z' (no offset means UTC)"
        ) from None


class Client:
    def __init__(
        self,
        network: str = DEFAULT_NETWORK,
        *,
        policy: Optional[Policy] = None,
        fallback_endpoints: Optional[list[str]] = None,
        archive_endpoints: Optional[list[str]] = None,
        retry_forever: bool = False,
        substrate: Optional[Substrate] = None,
    ):
        """Create a client for a network name (``finney``/``test``/``local``) or a
        raw ``ws://`` / ``wss://`` endpoint. An optional ``policy`` guards every
        mutation. The connection opens on ``connect()``.

        Connection resilience is on by default: when the endpoint is unreachable
        (or drops mid-session) the connection rotates through
        ``fallback_endpoints``, and reads against pruned state are retried on
        ``archive_endpoints``. Both default to the public pools for the network
        (see ``settings.FALLBACK_ENDPOINTS`` / ``settings.ARCHIVE_ENDPOINTS``);
        pass ``[]`` to pin the client to its single endpoint. With
        ``retry_forever`` connection failures never give up — the client keeps
        cycling through the endpoint pool until one answers.

        ``substrate`` swaps the chain-access backend: any :class:`Substrate`
        implementation (e.g. an in-memory fake for tests). When set, the
        connection options above don't apply — they configure the default
        RPC backend — while ``network`` still labels explorer links.
        """
        self.network, self.endpoint = resolve_endpoint(network)
        if substrate is not None:
            self._substrate: Substrate = substrate
        else:
            if fallback_endpoints is None:
                fallback_endpoints = default_fallback_endpoints(self.network)
            if archive_endpoints is None:
                archive_endpoints = default_archive_endpoints(self.network)
            self._substrate = RpcSubstrate(
                self.endpoint,
                fallback_endpoints=[url for url in fallback_endpoints if url != self.endpoint],
                archive_endpoints=archive_endpoints,
                retry_forever=retry_forever,
            )
        self._executor = Executor(self._substrate, policy=policy)

        # Typed read namespaces: projections over the read registry
        # (bittensor.reads), one per category — curated methods plus every
        # registered read. Anything else is reachable via the generic
        # query/runtime accessors below. (Keep in sync with namespaces.NAMESPACES
        # and Snapshot; assigned explicitly so type checkers see each attribute.)
        self.balances = Balances(self)
        self.chain = Chain(self)
        self.collateral = Collateral(self)
        self.delegation = Delegation(self)
        self.epochs = Epochs(self)
        self.hyperparameters = Hyperparameters(self)
        self.identity = Identity(self)
        self.leasing = Leasing(self)
        self.locks = Locks(self)
        self.neurons = Neurons(self)
        self.prices = Prices(self)
        self.staking = Staking(self)
        self.subnets = Subnets(self)
        self.weights = Weights(self)

    # Reads: generic accessors over generated descriptors ---------------------

    async def _block_hash(self, block: Optional[int]) -> Optional[str]:
        return await self._substrate.block_hash(block) if block is not None else None

    async def query(
        self, item: Descriptor, params: Optional[list] = None, *, block: Optional[int] = None
    ) -> Any:
        """Read a storage item, e.g. ``client.query(storage.SubtensorModule.Tempo, [netuid])``.

        A descriptor is just a ``(pallet, item)`` pair, so storage not yet in the
        generated catalog is reachable with a plain tuple:
        ``client.query(("SubtensorModule", "Tempo"), [netuid])``.
        """
        return await self._substrate.query(
            item[0], item[1], params, block_hash=await self._block_hash(block)
        )

    async def query_map(
        self, item: Descriptor, params: Optional[list] = None, *, block: Optional[int] = None
    ) -> list[tuple[Any, Any]]:
        """Read a whole storage map, e.g. ``client.query_map(storage.SubtensorModule.Tempo)``.

        As with :meth:`query`, a plain ``(pallet, item)`` tuple works for maps
        missing from the generated catalog.
        """
        return await self._substrate.query_map(
            item[0], item[1], params, block_hash=await self._block_hash(block)
        )

    async def query_batch(
        self, item: Descriptor, param_sets: list[list], *, block: Optional[int] = None
    ) -> list[Any]:
        """Read one storage map for many keys in a single RPC round-trip.

        ``param_sets`` is a list of parameter lists (one per key); results come
        back in the same order, all resolved against the same block.
        """
        return await self._substrate.query_batch(
            item[0], item[1], param_sets, block_hash=await self._block_hash(block)
        )

    async def runtime(
        self, method: Descriptor, params: list, *, block: Optional[int] = None
    ) -> Any:
        """Call a runtime API, e.g.
        ``client.runtime(runtime_api.NeuronInfoRuntimeApi.get_neurons_lite, [netuid])``.

        As with :meth:`query`, a plain ``(api, method)`` tuple works for APIs
        missing from the generated catalog.
        """
        return await self._substrate.runtime_call(
            method[0], method[1], params, block_hash=await self._block_hash(block)
        )

    async def constant(self, item: Descriptor) -> Any:
        """Read a pallet constant, e.g.
        ``client.constant(constants.Balances.ExistentialDeposit)``."""
        return await self._substrate.constant(item[0], item[1])

    async def decode_scale(self, type_string: str, data: Any) -> Any:
        """Decode SCALE-encoded bytes (or 0x-hex) against the connected runtime's types.

        The read-side escape hatch, like :meth:`submit_call` for writes: e.g.
        ``client.decode_scale("Call", call_data)`` recovers the call a multisig
        approval or timelock ciphertext carries as opaque bytes.
        """
        return await self._substrate.decode_scale(type_string, data)

    async def read(self, name: str, **params: Any) -> Any:
        """Run a named typed read at the chain head (see ``reads()`` for the catalog).

        e.g. ``await client.read("subnet_hyperparameters", netuid=1)``. For a
        block-pinned read use a snapshot: ``(await client.at(block)).read(...)``.
        """
        return await read_registry.dispatch(self, name, params)

    def reads(self) -> list[dict]:
        """Machine-readable catalog of every typed read (for agents)."""
        return read_registry.list_reads()

    async def blocks(self, *, finalized: bool = False) -> AsyncIterator[BlockHeader]:
        """Stream new block headers as they are produced (or finalized).

        e.g. ``async for header in client.blocks(): ...`` — break out of the loop
        to cancel the underlying subscription.
        """
        async for block in self._substrate.blocks(finalized=finalized):
            header = block.get("header") or {}
            yield BlockHeader(
                number=int(header["number"]),
                parent_hash=header.get("parentHash"),
                raw=header,
            )

    # Chain / block metadata ---------------------------------------------------

    async def timestamp(self, block: Optional[int] = None) -> datetime:
        """UTC timestamp of a block (defaults to the chain head)."""
        ms = await self.query(_st.Timestamp.Now, block=block)
        return datetime.fromtimestamp(int(ms) / 1000, tz=timezone.utc)

    async def block_time(self) -> float:
        """Seconds per block, detected from the chain (cached after first read).

        12.0 on mainnet, 0.25 on fast-blocks local chains — use this instead of
        assuming a block time whenever converting blocks to wall-clock time.
        """
        return await self._substrate.block_time()

    async def is_fast_blocks(self) -> bool:
        """Whether the chain runs fast blocks (0.25s slots, local/e2e testing mode)."""
        return await self.block_time() == FAST_BLOCK_TIME

    async def block_info(self, block: Optional[int] = None) -> Optional[BlockInfo]:
        """A block's header, timestamp, and decoded extrinsics (defaults to the head).

        One aggregated lookup for "what happened in this block". Everything else
        about chain state at that block is reachable via the generic
        ``query``/``runtime`` accessors with ``block=``.
        """
        raw = await self._substrate.get_block(block_number=block)
        header = (raw or {}).get("header")
        if not header:
            return None
        number = int(header["number"])
        extrinsics = [_extrinsic_value(e) for e in (raw or {}).get("extrinsics") or []]
        stamp = _timestamp_from_extrinsics(extrinsics)
        if stamp is None:
            stamp = await self.timestamp(block=number)
        return BlockInfo(
            number=number,
            hash=header.get("hash"),
            timestamp=stamp,
            explorer_url=explorer_block_url(self.network, number),
            header=header,
            extrinsics=extrinsics,
        )

    # Waits --------------------------------------------------------------------

    async def wait_for_block(
        self, block: Optional[int] = None, *, timeout: Optional[float] = None
    ) -> BlockHeader:
        """Wait until the chain reaches a block, returning the header that got there.

        With no ``block``, waits for the next block. Subscription-based (no
        polling); raises ``asyncio.TimeoutError`` if ``timeout`` seconds elapse.
        """

        async def wait() -> BlockHeader:
            target = block if block is not None else await self.block() + 1
            async for header in self.blocks():
                if header.number >= target:
                    return header
            raise ChainError("block subscription ended before the target block")

        return await asyncio.wait_for(wait(), timeout) if timeout else await wait()

    async def wait_for_timestamp(
        self, when: WhenLike, *, timeout: Optional[float] = None
    ) -> BlockHeader:
        """Wait until chain time reaches ``when``, returning the header that got there.

        ``when`` is a datetime, unix timestamp, or ISO-8601 string (naive means
        UTC). Long gaps are slept through instead of holding a subscription open;
        the decision is always made on the chain's own clock, not the local one.
        """

        async def wait() -> BlockHeader:
            target = _as_unix(when)
            block_time = await self.block_time()
            gap = target - (await self.timestamp()).timestamp()
            if gap > 4 * block_time:
                await asyncio.sleep(gap - 2 * block_time)
            async for header in self.blocks():
                stamp = await self.timestamp(block=header.number)
                if stamp.timestamp() >= target:
                    return header
            raise ChainError("block subscription ended before the target timestamp")

        return await asyncio.wait_for(wait(), timeout) if timeout else await wait()

    async def wait_for_epoch(self, netuid: int, *, timeout: Optional[float] = None) -> EpochEvent:
        """Wait until the subnet's next epoch has actually run.

        Watches ``LastEpochBlock`` advance rather than waiting for a predicted
        block: the chain's epoch schedule is stateful (owner ``trigger_epoch``
        can pull an epoch earlier, the per-block epoch cap can defer one later),
        so only an observed run is trustworthy. Costs one storage read per new
        block while waiting.
        """

        async def wait() -> EpochEvent:
            tempo = int(await self.query(_st.SubtensorModule.Tempo, [netuid]) or 0)
            if tempo == 0:
                raise BittensorError(f"subnet {netuid} has tempo 0 and never runs epochs")
            baseline = int(await self.query(_st.SubtensorModule.LastEpochBlock, [netuid]) or 0)
            async for header in self.blocks():
                value = int(
                    await self.query(
                        _st.SubtensorModule.LastEpochBlock, [netuid], block=header.number
                    )
                    or 0
                )
                if value > baseline:
                    return EpochEvent(netuid=netuid, block=value, previous=baseline)
            raise ChainError("block subscription ended before the next epoch")

        return await asyncio.wait_for(wait(), timeout) if timeout else await wait()

    # Intent layer -----------------------------------------------------------

    async def plan(self, intent: Intent, wallet: WalletLike, **kwargs) -> Plan:
        """Preview an intent (fee, effects, warnings, policy) without submitting.

        This is the dry run: to see what a mutation would do, ``plan`` it. Accepts
        the same ``policy`` / ``proxy_for`` / ``proxy_type`` options as ``execute``.
        """
        return await self._executor.plan(intent, wallet, **kwargs)

    async def execute(self, intent: Intent, wallet: WalletLike, **kwargs) -> ExtrinsicResult:
        """Sign and submit an intent through the policy-gated choke point.

        Raises :class:`PolicyError` if the intent violates the active policy.
        """
        return await self._executor.execute(intent, wallet, **kwargs)

    async def execute_tool(
        self, op: str, args: dict, wallet: WalletLike, **kwargs
    ) -> ExtrinsicResult:
        """Build an intent by name from a dict of args and execute it."""
        return await self._executor.execute_tool(op, args, wallet, **kwargs)

    async def submit_shielded(
        self, intent: Intent, wallet: WalletLike, **kwargs
    ) -> ExtrinsicResult:
        """Submit an intent MEV-shielded (encrypted until block-author execution).

        Same policy gating as :meth:`execute`; see ``Executor.submit_shielded``.
        """
        return await self._executor.submit_shielded(intent, wallet, **kwargs)

    async def submit_call(self, call, wallet: WalletLike, **kwargs) -> ExtrinsicResult:
        """Escape hatch: submit any generated raw call (``bittensor.calls``) directly.

        No intent, no preview — an active :class:`Policy` refuses this unless it
        sets ``allow_raw_calls=True``. Pass ``signer="hotkey"`` for hotkey-signed
        extrinsics.
        """
        return await self._executor.submit_call(call, wallet, **kwargs)

    async def prepare_call(self, call, *, address: str, **kwargs) -> UnsignedExtrinsic:
        """Build an unsigned extrinsic to be signed out-of-process (QR vault,
        air-gapped machine, hardware device elsewhere).

        Needs no key material — only the signing account's ``address``. The
        result carries the exact payload bytes to sign (``payload``) and the
        Polkadot-JS ``SignerPayloadJSON`` (``payload_json``); once the external
        signer returns a signature, submit it with :meth:`submit_signature`.
        ``call`` is a generated call builder or an already-composed call, so a
        planned intent works too::

            plan = await client.plan(intent, wallet)
            unsigned = await client.prepare_call(plan.call, address=plan.signer_address)
            # ...display unsigned.payload as a QR, scan the signature back...
            result = await client.submit_signature(unsigned, signature)

        To cross a process or machine boundary, ``unsigned.to_dict()`` is
        JSON-native and ``UnsignedExtrinsic.from_dict`` reconstructs it on the
        submitting side.
        """
        return await self._executor.prepare_call(call, address=address, **kwargs)

    async def submit_signature(
        self, unsigned: UnsignedExtrinsic, signature: bytes | str, **kwargs
    ) -> ExtrinsicResult:
        """Submit an externally-produced signature for a prepared extrinsic.

        Policy-gated like :meth:`submit_call` (the prepared call is opaque, so
        an active :class:`Policy` must set ``allow_raw_calls=True``). Refuses
        with a clear error if the runtime upgraded since preparation.
        """
        return await self._executor.submit_signature(unsigned, signature, **kwargs)

    async def compose(self, call):
        """Compose a generated call into a chain-ready call object.

        Needed to nest one call inside another — the inner call of ``Sudo.sudo``,
        ``Utility.batch``, or ``Proxy.proxy`` must be composed before it becomes a
        parameter, e.g.::

            inner = await client.compose(bt.calls.System.set_code(code=wasm_hex))
            await client.submit_call(bt.calls.Sudo.sudo(call=inner), sudo_wallet)
        """
        return await self._substrate.compose(call)

    async def estimate_weight(self, call, *, address: str) -> dict:
        """The declared dispatch weight ``{ref_time, proof_size}`` of a composed call.

        This is what ``pallet_multisig`` compares an executing approval's
        ``max_weight`` against; needs no key material, only the prospective
        signer's ``address`` (fee estimation signs with a zeroed signature).
        """
        view = _AddressView(address)
        return await self._substrate.estimate_weight(call, view)

    async def multisig(self, signatories: list[str], threshold: int) -> Multisig:
        """A handle to the M-of-N multisig account for a signer set.

        The address is derived deterministically from the signatory *set* and
        ``threshold`` against the connected runtime. Have each signatory call
        ``approve(call, wallet)`` on the returned object; the one that reaches the
        threshold executes the call.
        """
        account = self._substrate.multisig_account(signatories, threshold)
        return Multisig(
            signatories=list(signatories),
            threshold=threshold,
            address=account.ss58_address,
            _client=self,
            _account=account,
        )

    def tools(self) -> list[dict]:
        """Machine-readable catalog of every executable operation (for agents)."""
        return self._executor.tools()

    @property
    def token_symbols(self) -> dict[int, str]:
        """netuid -> chain-registered token symbol for this connection.

        Populated by ``connect()`` (from the disk cache or one storage-map
        scan); empty before connecting. Display-only metadata.
        """
        return self._substrate.token_symbols

    def balance(self, rao: int, netuid: int = 0) -> Balance:
        """A :class:`Balance` tagged with this connection's token symbol.

        Use this (rather than ``Balance.from_rao``) when decoding a chain
        amount you intend to display, so it renders with the subnet's real
        symbol instead of the generic ``α`` + netuid fallback.
        """
        return self._substrate.balance(rao, netuid)

    @property
    def policy(self) -> Optional[Policy]:
        return self._executor.policy

    @policy.setter
    def policy(self, value: Optional[Policy]) -> None:
        self._executor.policy = value

    # Lifecycle --------------------------------------------------------------

    async def connect(self) -> "Client":
        await self._substrate.connect()
        # Balances render with each subnet's chain-registered token symbol; the
        # map is installed on THIS connection (never process-global, so clients
        # on different networks can't clobber each other). Symbols essentially
        # never change, so the disk cache saves a full storage-map scan on
        # every connect; it refreshes on TTL expiry.
        if config.token_symbols_fresh(self.network):
            self._substrate.token_symbols = config.load_token_symbols(self.network)
        else:
            try:
                symbols = await read_registry.token_symbols(self)
            except ChainError:
                pass  # symbols are display-only; never fail a connect over them
            else:
                self._substrate.token_symbols = symbols
                # cache write is best-effort
                with contextlib.suppress(OSError):
                    config.save_token_symbols(self.network, symbols)

        return self

    async def close(self) -> None:
        await self._substrate.close()

    async def block(self) -> int:
        """Current chain block number."""
        return await self._substrate.block_number()

    async def spec_version(self) -> int:
        """The connected runtime's ``spec_version`` (at the chain head)."""
        return await self._substrate.spec_version()

    async def at(self, block: Optional[int] = None) -> Snapshot:
        """A read-only view pinned to ``block`` (defaults to the current head).

        All reads through the snapshot hit the same block: consistent values,
        and the block hash resolves once instead of per call.
        """
        if block is None:
            block = await self._substrate.block_number()
        return Snapshot(self, block)

    async def __aenter__(self) -> "Client":
        return await self.connect()

    async def __aexit__(self, *_exc) -> None:
        await self.close()

    def __repr__(self) -> str:
        return f"Client(network={self.network!r}, endpoint={self.endpoint!r})"


def _extrinsic_value(extrinsic: Any) -> Any:
    """The plain decoded value of a transport extrinsic (None for decode failures)."""
    return getattr(extrinsic, "value", extrinsic)


def _timestamp_from_extrinsics(extrinsics: list) -> Optional[datetime]:
    """The block's timestamp from its ``Timestamp.set`` inherent, or None.

    Every block carries exactly one such inherent, so this avoids a second
    storage round-trip when the extrinsics decoded cleanly.
    """
    for value in extrinsics:
        call = value.get("call") if isinstance(value, dict) else None
        if not isinstance(call, dict) or call.get("call_module") != "Timestamp":
            continue
        args = call.get("call_args") or []
        if args and isinstance(args[0], dict) and args[0].get("value") is not None:
            return datetime.fromtimestamp(int(args[0]["value"]) / 1000, tz=timezone.utc)
    return None
