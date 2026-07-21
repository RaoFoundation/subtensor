"""The chain-access seam: the :class:`Substrate` contract and its RPC backend.

``Substrate`` is the narrow protocol everything above the transport is written
against — the executor, intents, reads, and domain namespaces all take "a
Substrate", never a websocket. :class:`RpcSubstrate` is the production
implementation: a thin async wrapper over the transport's
``SubstrateConnection``, and the only code in the SDK that talks to the
transport directly.

``Client`` constructs an :class:`RpcSubstrate` by default; pass ``substrate=``
to inject any other implementation (an in-memory fake for tests needs no
network at all).
"""

from __future__ import annotations

import asyncio
from typing import Any, AsyncIterator, Awaitable, Callable, Optional, Protocol, TypeVar

from ._transport import SubstrateConnection
from ._transport.contract import (
    InclusionReport,
    MultisigAccount,
    SignedExtrinsic,
    UnsignedExtrinsic,
)
from ._transport.errors import StateDiscardedError, SubstrateRequestException
from .balance import Balance
from .result import (
    ChainError,
    ConnectionNotReady,
    ExtrinsicResult,
    chain_error_from_substrate_request,
)
from .settings import DEFAULT_ERA_PERIOD, SS58_FORMAT, explorer_extrinsic_url

T = TypeVar("T")


class Substrate(Protocol):
    """Anything that can read chain state and submit extrinsics.

    This is the SDK's chain-access contract. Reads take an optional
    ``block_hash`` and return plain decoded values; writes take a composed call
    and a keypair-shaped signer and return a typed :class:`ExtrinsicResult`
    (never a raw receipt). Implementations raise :class:`ChainError` for
    chain-side failures on reads; a failed write is reported through the
    result, not an exception.
    """

    # Display metadata -----------------------------------------------------------

    # netuid -> chain-registered token symbol for this connection, installed by
    # ``Client.connect``. Connection-scoped: two clients on different networks
    # each render their own chain's symbols.
    token_symbols: dict[int, str]

    def balance(self, rao: int, netuid: int = 0) -> Balance:
        """A :class:`Balance` tagged with this connection's token symbol.

        The decode-time factory reads use for subnet-denominated amounts, so a
        value renders with the right symbol no matter how many clients or
        networks the process talks to.
        """
        ...

    # Lifecycle ----------------------------------------------------------------

    async def connect(self) -> None:
        """Open the connection. Idempotent."""
        ...

    async def close(self) -> None:
        """Close the connection and release resources."""
        ...

    # Reads --------------------------------------------------------------------

    async def block_hash(self, block: Optional[int] = None) -> str:
        """Hash of a block number (defaults to the chain head)."""
        ...

    async def block_number(self) -> int:
        """Current chain block number."""
        ...

    async def block_time(self) -> float:
        """Seconds per block, detected from the chain."""
        ...

    async def spec_version(self) -> int:
        """The connected runtime's ``spec_version`` (at the chain head)."""
        ...

    async def get_block(
        self, block_number: Optional[int] = None, block_hash: Optional[str] = None
    ) -> Optional[dict]:
        """A block's header and decoded extrinsics (defaults to the chain head)."""
        ...

    async def query(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> Any:
        """Read one storage item, returning its plain decoded value."""
        ...

    async def query_map(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> list[tuple[Any, Any]]:
        """Read a whole storage map as decoded ``(key, value)`` pairs."""
        ...

    async def query_batch(
        self,
        module: str,
        storage_function: str,
        param_sets: list[list],
        block_hash: Optional[str] = None,
    ) -> list[Any]:
        """Read one storage map for many keys, results in ``param_sets`` order."""
        ...

    async def runtime_call(
        self, api: str, method: str, params: list, block_hash: Optional[str] = None
    ) -> Any:
        """Call a runtime API, returning its plain decoded value."""
        ...

    async def constant(self, module: str, name: str) -> Any:
        """Read a pallet constant."""
        ...

    async def decode_scale(self, type_string: str, data: Any) -> Any:
        """Decode SCALE-encoded bytes against the connected runtime's types."""
        ...

    def blocks(self, *, finalized: bool = False) -> AsyncIterator[dict]:
        """Stream decoded block headers as they are produced (or finalized)."""
        ...

    async def events(self, block_hash: Optional[str] = None) -> list[dict]:
        """Decoded ``System.Events`` records for a block."""
        ...

    # Calls and fees -------------------------------------------------------------

    async def compose(self, call) -> Any:
        """Compose a generated ``(module, function, params)`` call into a chain-ready call."""
        ...

    async def estimate_fee(self, call, keypair) -> Balance:
        """Estimate the fee for a call without submitting it (no signature needed)."""
        ...

    async def estimate_weight(self, call, keypair) -> dict:
        """The dispatch weight ``{ref_time, proof_size}`` of a call."""
        ...

    async def account_next_index(self, address: str) -> int:
        """Next valid nonce for an account, including transactions in the pool."""
        ...

    # Writes ---------------------------------------------------------------------

    async def mev_next_key(self) -> Optional[bytes]:
        """The MEV Shield ML-KEM-768 public key from ``NextKey`` storage, if active."""
        ...

    async def sign_extrinsic(self, call, keypair, *, nonce: int, period: int) -> tuple[bytes, str]:
        """A signed extrinsic's (raw bytes, 0x-hex hash), without submitting."""
        ...

    async def prepare(
        self,
        call,
        *,
        address: str,
        crypto_type: int = 1,
        nonce: Optional[int] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        tip: int = 0,
        metadata_hash: Optional[bytes] = None,
    ) -> UnsignedExtrinsic:
        """An unsigned extrinsic for out-of-process signing (no key material)."""
        ...

    async def submit_signature(
        self,
        unsigned: UnsignedExtrinsic,
        signature: bytes | str,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Assemble a prepared extrinsic with its signature and submit it."""
        ...

    async def submit(
        self,
        call,
        keypair,
        *,
        nonce: Optional[int] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Sign, submit, and wait for a call, returning a typed result."""
        ...

    async def submit_signed(
        self,
        extrinsic,
        keypair,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Submit an already-signed extrinsic, returning a typed result."""
        ...

    # Multisig ---------------------------------------------------------------------

    def multisig_account(self, signatories: list[str], threshold: int):
        """Derive the deterministic multisig ``MultiAccountId`` for a signer set."""
        ...

    async def submit_multisig(
        self,
        call,
        keypair,
        multisig_account,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Record one signatory's approval of ``call`` for a multisig account."""
        ...


class RpcSubstrate:
    def __init__(
        self,
        endpoint: str,
        *,
        fallback_endpoints: Optional[list[str]] = None,
        archive_endpoints: Optional[list[str]] = None,
        retry_forever: bool = False,
    ):
        self.endpoint = endpoint
        self.fallback_endpoints = list(fallback_endpoints or [])
        self.archive_endpoints = [url for url in (archive_endpoints or []) if url != endpoint]
        self.retry_forever = retry_forever
        self._substrate: Optional[SubstrateConnection] = None
        self._archive_substrate: Optional[SubstrateConnection] = None
        self._archive_lock = asyncio.Lock()
        self._block_time: Optional[float] = None
        # netuid -> chain-registered token symbol for THIS connection, installed
        # by ``Client.connect``. Connection-scoped on purpose: two clients on
        # different networks each render their own chain's symbols.
        self.token_symbols: dict[int, str] = {}

    def balance(self, rao: int, netuid: int = 0) -> Balance:
        """A :class:`Balance` tagged with this connection's token symbol.

        The decode-time factory every read should use for subnet-denominated
        amounts, so the value renders with the right symbol regardless of how
        many clients or networks the process talks to. Symbol lookup is a dict
        get against the map fetched once at connect.
        """
        symbol = self.token_symbols.get(netuid) if netuid else None
        return Balance(int(rao), netuid, symbol)

    @property
    def raw(self) -> SubstrateConnection:
        if self._substrate is None:
            raise ConnectionNotReady(
                "Client connection is not open. Use `async with Client(...)` "
                "or call `await client.connect()` first."
            )
        return self._substrate

    def _interface(self, endpoint: str, fallbacks: list[str]) -> SubstrateConnection:
        return SubstrateConnection(
            endpoint,
            ss58_format=SS58_FORMAT,
            fallback_urls=fallbacks,
            retry_forever=self.retry_forever,
        )

    async def connect(self) -> None:
        if self._substrate is not None:
            return
        substrate = self._interface(self.endpoint, self.fallback_endpoints)
        await substrate.initialize()
        self._substrate = substrate

    async def _archive(self) -> Optional[SubstrateConnection]:
        """The lazily-opened archive connection, or None when no archive pool is configured."""
        if not self.archive_endpoints:
            return None
        async with self._archive_lock:
            if self._archive_substrate is None:
                primary, *fallbacks = self.archive_endpoints
                substrate = self._interface(primary, fallbacks)
                await substrate.initialize()
                self._archive_substrate = substrate
        return self._archive_substrate

    async def _read(self, op: Callable[[SubstrateConnection], Awaitable[T]]) -> T:
        """Run a read against the live connection, normalizing transport errors
        into :class:`ChainError`.

        When the read hits state the (lite) primary node has already pruned, it
        is transparently retried against the archive pool instead of failing
        with "state discarded".
        """
        try:
            try:
                return await op(self.raw)
            except StateDiscardedError:
                archive = await self._archive()
                if archive is None:
                    raise
                return await op(archive)
        except SubstrateRequestException as error:
            raise ChainError(str(error)) from error

    async def close(self) -> None:
        if self._substrate is not None:
            await self._substrate.close()
            self._substrate = None
        if self._archive_substrate is not None:
            await self._archive_substrate.close()
            self._archive_substrate = None

    # Reads ------------------------------------------------------------------

    async def block_hash(self, block: Optional[int] = None) -> str:
        if block is None:
            return await self._read(lambda raw: raw.get_chain_head())
        return await self._read(lambda raw: raw.get_block_hash(block))

    async def block_number(self) -> int:
        return await self._read(lambda raw: raw.get_block_number(None))

    async def block_time(self) -> float:
        """Seconds per block, from the chain's ``Aura.SlotDuration`` constant.

        Cached after the first read — a constant can only change with a runtime
        upgrade. 12.0 on mainnet, 0.25 on fast-blocks local chains; the detected
        value is what all timing math (commit-reveal rounds, "retry in ~Ns"
        hints, waits) must use, never a hardcoded block time.
        """
        if self._block_time is None:
            self._block_time = int(await self.constant("Aura", "SlotDuration")) / 1000.0
        return self._block_time

    async def spec_version(self) -> int:
        """The connected runtime's ``spec_version``.

        Read from the transport's head codec (TTL-validated against
        ``state_getRuntimeVersion``), so a runtime upgrade mid-session is
        picked up within about a block.
        """
        return await self._read(lambda raw: raw.runtime_spec_version())

    async def get_block(
        self, block_number: Optional[int] = None, block_hash: Optional[str] = None
    ) -> Optional[dict]:
        """A block's header and decoded extrinsics (defaults to the chain head).

        Extrinsics that fail to decode are returned as None entries rather than
        failing the whole block.
        """
        block = await self._read(
            lambda raw: raw.get_block(
                block_hash=block_hash,
                block_number=block_number,
                ignore_decoding_errors=True,
            )
        )
        if block is None:
            return None
        return {"header": block.header, "extrinsics": block.extrinsics}

    async def query(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> Any:
        return await self._read(
            lambda raw: raw.query(module, storage_function, params or [], block_hash)
        )

    async def query_map(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> list[tuple[Any, Any]]:
        async def op(raw: SubstrateConnection) -> list[tuple[Any, Any]]:
            # Iterate inside the op so pagination follows the same connection
            # the first page came from (matters for the archive retry).
            result = await raw.query_map(module, storage_function, params or [], block_hash)
            return [(_unwrap_key(k), v) async for k, v in result]

        return await self._read(op)

    async def query_batch(
        self,
        module: str,
        storage_function: str,
        param_sets: list[list],
        block_hash: Optional[str] = None,
    ) -> list[Any]:
        """Read the same storage map for many keys in a single RPC round-trip.

        ``param_sets`` is a list of parameter lists (one per key); results are
        returned in the same order. Building the storage keys is local work against
        cached runtime metadata, so only one network request is made.
        """
        if not param_sets:
            return []
        return await self._read(
            lambda raw: raw.query_batch(module, storage_function, param_sets, block_hash)
        )

    async def runtime_call(
        self,
        api: str,
        method: str,
        params: list,
        block_hash: Optional[str] = None,
    ) -> Any:
        return await self._read(lambda raw: raw.runtime_call(api, method, params, block_hash))

    async def constant(self, module: str, name: str) -> Any:
        return await self._read(lambda raw: raw.get_constant(module, name))

    async def decode_scale(self, type_string: str, data: Any) -> Any:
        """Decode SCALE-encoded bytes (or 0x-hex) against the connected runtime's types.

        e.g. ``decode_scale("Call", call_data_hex)`` recovers the call a
        multisig or timelock carries around, as a plain decoded dict.
        """
        return await self._read(lambda raw: raw.decode_scale(type_string, data))

    async def blocks(self, *, finalized: bool = False) -> AsyncIterator[dict]:
        """Stream decoded block headers as they are produced (or finalized).

        Yields ``{"header": {...}}`` dicts with the block number as an int.
        Break out of the loop to cancel the underlying subscription.
        """
        try:
            async for header in self.raw.subscribe_heads(finalized=finalized):
                yield {"header": header}
        except SubstrateRequestException as error:
            raise ChainError(str(error)) from error

    async def events(self, block_hash: Optional[str] = None) -> list[dict]:
        """Decoded ``System.Events`` records for a block."""
        return await self._read(lambda raw: raw.get_events(block_hash))

    async def compose(self, call):
        """Compose a chain call from a generated ``Call`` (module, function, params)."""
        module, function, params = call
        return await self._read(
            lambda raw: raw.compose_call(
                call_module=module, call_function=function, call_params=params
            )
        )

    async def estimate_fee(self, call, keypair) -> Balance:
        """Estimate the fee for a call without submitting it (no signature needed)."""
        info = await self._read(lambda raw: raw.get_payment_info(call, keypair))
        fee = info.get("partial_fee", info.get("partialFee", 0))
        return Balance.from_rao(int(fee))

    async def estimate_weight(self, call, keypair) -> dict:
        """The dispatch weight ``{ref_time, proof_size}`` of a call, for multisig max_weight."""
        info = await self._read(lambda raw: raw.get_payment_info(call, keypair))
        weight = info.get("weight") or {}
        return {
            "ref_time": int(weight.get("ref_time", 0)),
            "proof_size": int(weight.get("proof_size", 0)),
        }

    async def account_next_index(self, address: str) -> int:
        """Next valid nonce for an account, including transactions in the pool."""
        return await self._read(lambda raw: raw.get_account_next_index(address))

    # Writes -----------------------------------------------------------------

    async def mev_next_key(self) -> Optional[bytes]:
        """The MEV Shield ML-KEM-768 public key from ``NextKey`` storage (rotates per block)."""
        value = await self.query("MevShield", "NextKey")
        if not value:
            return None
        if isinstance(value, str):
            return bytes.fromhex(value.removeprefix("0x"))
        return bytes(value)

    async def sign_extrinsic(self, call, keypair, *, nonce: int, period: int) -> tuple[bytes, str]:
        """Create a signed extrinsic and return its (raw bytes, 0x-hex hash) without submitting.

        Used to build the inner extrinsic for MEV-shielded submission, which is
        encrypted and carried inside ``MevShield.submit_encrypted``.
        """
        extrinsic = await self.raw.create_signed_extrinsic(
            call, keypair, nonce=nonce, era={"period": period}
        )
        return extrinsic.data, extrinsic.extrinsic_hash

    async def submit(
        self,
        call,
        keypair,
        *,
        nonce: Optional[int] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Sign, submit, and wait for a call, returning a typed result.

        The nonce is fetched via ``get_account_next_index`` (unless one is passed
        explicitly, as for MEV-shielded submission where the outer/inner nonces must
        be pinned) so the outer/inner nonces of a call are deterministic. On any
        submission failure the account's cached next-index is cleared, because a
        failed submit does not consume the nonce on-chain and a stale cached value
        would wedge every subsequent submit for that account into future-nonce limbo.
        """
        try:
            extrinsic = await self.raw.create_signed_extrinsic(
                call,
                keypair,
                nonce=nonce,
                era={"period": period} if period is not None else None,
            )
        except Exception as error:
            # Signing may already have consumed a cached nonce; see _failed_submit.
            return self._failed_submit(error, keypair.ss58_address)
        return await self._submit_and_report(
            extrinsic,
            signer_address=keypair.ss58_address,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    async def prepare(
        self,
        call,
        *,
        address: str,
        crypto_type: int = 1,
        nonce: Optional[int] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        tip: int = 0,
        metadata_hash: Optional[bytes] = None,
    ) -> UnsignedExtrinsic:
        """An unsigned extrinsic for out-of-process signing.

        Only the account's *address* is needed — the private key can live on a
        QR-scanned vault device, a Ledger, or another machine entirely. The
        result carries the exact payload bytes to sign plus the Polkadot-JS
        payload JSON; hand the signature to :meth:`submit_signature`.
        """
        try:
            return await self.raw.prepare_unsigned(
                call,
                address=address,
                crypto_type=crypto_type,
                nonce=nonce,
                era={"period": period} if period is not None else None,
                tip=tip,
                metadata_hash=metadata_hash,
            )
        except SubstrateRequestException as error:
            raise ChainError(str(error)) from error

    async def submit_signature(
        self,
        unsigned: UnsignedExtrinsic,
        signature: bytes | str,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Assemble a prepared extrinsic with an externally-produced signature
        and submit it, with the same outcome decoding as :meth:`submit`."""
        try:
            extrinsic = await self.raw.attach_signature(unsigned, signature)
        except SubstrateRequestException as error:
            raise ChainError(str(error)) from error
        return await self._submit_and_report(
            extrinsic,
            signer_address=unsigned.address,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    async def submit_signed(
        self,
        extrinsic: SignedExtrinsic,
        keypair,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Submit an already-signed extrinsic and return a typed result.

        Used when the extrinsic is built elsewhere (e.g. a multisig approval), so
        the same outcome decoding and nonce-cache hygiene as :meth:`submit`
        apply without this layer re-signing the call.
        """
        return await self._submit_and_report(
            extrinsic,
            signer_address=keypair.ss58_address,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    async def _submit_and_report(
        self,
        extrinsic: SignedExtrinsic,
        *,
        signer_address: str,
        wait_for_inclusion: bool,
        wait_for_finalization: bool,
    ) -> ExtrinsicResult:
        """Submit a signed extrinsic and decode the outcome."""
        try:
            report = await self.raw.submit_extrinsic(
                extrinsic,
                wait_for_inclusion=wait_for_inclusion,
                wait_for_finalization=wait_for_finalization,
            )
        except Exception as error:
            return self._failed_submit(error, signer_address)
        return self._result_from_report(report, wait_for_inclusion or wait_for_finalization)

    def _failed_submit(self, error: Exception, signer_address: str) -> ExtrinsicResult:
        """Translate a failure between signing and inclusion into the write contract.

        Any such failure (RPC rejection, dropped websocket, cancellation)
        leaves the nonce unconsumed on-chain, so the signer's cached next-index
        is cleared — a stale value would wedge every subsequent submit for
        that account into future-nonce limbo. Chain-side rejections become a
        failed :class:`ExtrinsicResult`; anything else raises :class:`ChainError`.
        """
        self.raw.clear_nonce_cache_for_account(signer_address)
        if not isinstance(error, SubstrateRequestException):
            raise ChainError(str(error)) from error
        chain_error = chain_error_from_substrate_request(error)
        return ExtrinsicResult(
            success=False,
            message=chain_error.message,
            error=chain_error,
        )

    def _result_from_report(self, report: InclusionReport, waited: bool) -> ExtrinsicResult:
        """Turn the transport's inclusion report into a typed :class:`ExtrinsicResult`."""
        if not waited:
            return ExtrinsicResult(success=True, message="Submitted (not waiting).")

        extrinsic_id = None
        if report.block_number is not None and report.extrinsic_idx is not None:
            # Index zero-padded to 4 digits, as explorers render it.
            extrinsic_id = f"{report.block_number}-{report.extrinsic_idx:04d}"
        explorer = explorer_extrinsic_url(self.endpoint, extrinsic_id) if extrinsic_id else None

        if report.is_success:
            return ExtrinsicResult(
                success=True,
                message="Success",
                block_hash=report.block_hash,
                extrinsic_id=extrinsic_id,
                explorer_url=explorer,
                fee=Balance.from_rao(report.total_fee_amount or 0),
                events=list(report.triggered_events),
            )

        error_message = report.error_message
        name = None
        if isinstance(error_message, dict):
            name = error_message.get("name")
            text = error_message.get("docs") or error_message.get("name") or str(error_message)
            text = " ".join(text) if isinstance(text, list) else str(text)
        else:
            text = str(error_message)
        return ExtrinsicResult(
            success=False,
            message=text,
            block_hash=report.block_hash,
            extrinsic_id=extrinsic_id,
            explorer_url=explorer,
            error=ChainError(text, name),
        )

    # Multisig ---------------------------------------------------------------

    def multisig_account(self, signatories: list[str], threshold: int) -> MultisigAccount:
        """Derive the deterministic multisig account for a signer set."""
        return self.raw.generate_multisig_account(signatories, threshold)

    async def submit_multisig(
        self,
        call,
        keypair,
        multisig_account: MultisigAccount,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Record one signatory's approval of ``call`` for a multisig account.

        The transport composes ``Multisig.as_multi`` with the full call (so the
        opening extrinsic embeds the call bytes on-chain), carrying the existing
        timepoint when the operation is already open. ``call`` must be a
        composed call (see :meth:`compose`).
        """
        try:
            extrinsic = await self.raw.create_multisig_extrinsic(call, keypair, multisig_account)
        except SubstrateRequestException as error:
            raise ChainError(str(error)) from error
        return await self.submit_signed(
            extrinsic,
            keypair,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )


def _unwrap_key(key):
    """query_map keys arrive as a scalar or a length-1 tuple; normalize to a scalar."""
    if isinstance(key, (tuple, list)) and len(key) == 1:
        return key[0]
    return key
