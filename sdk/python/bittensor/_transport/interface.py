"""``SubstrateConnection``: the transport's public facade.

This is the complete surface the SDK consumes — everything else in the package
is an implementation detail behind it. Values in and out are plain Python
(ints, strings, dicts, bytes) or the typed shapes in :mod:`contract`; the one
opaque object is the composed call, which callers pass around without
inspecting.
"""

from __future__ import annotations

import asyncio
import logging
from hashlib import blake2b
from typing import Any, AsyncIterator, Optional

from .codec import is_valid_ss58_address as _is_valid_ss58_address
from .codec import multisig_account as _multisig_account
from .codec import ss58_decode as _ss58_decode
from .codec import ss58_encode as _ss58_encode
from .const import SS58_FORMAT
from .contract import (
    BlockData,
    InclusionReport,
    MetadataIR,
    MultisigAccount,
    SignedExtrinsic,
    UnsignedExtrinsic,
)
from .errors import BlockNotFound, ExtrinsicNotFound, SubstrateRequestException
from .extrinsics import IMMORTAL, NonceCache, resolve_outcome, watch_status_block
from .extrinsics import attach_signature as _attach_signature
from .extrinsics import create_signed_extrinsic as _sign_and_assemble
from .extrinsics import prepare_extrinsic as _prepare_extrinsic
from .lru import LRUCache
from .protocols import Keypair
from .rpc import RpcSession
from .runtime import RuntimeManager
from .runtime_api import call_runtime_api, decode_runtime_api_result
from .storage import decode_map_pairs, decode_storage_value, decode_storage_values

logger = logging.getLogger("bittensor.transport")

# How long a probed finalized-head height is trusted (one mainnet block).
_FINALIZED_TTL = 12.0


class QueryMapResult:
    """Decoded (key, value) pairs of a storage map, paginating transparently.

    Iterate with ``async for``; more pages are fetched on demand. ``records``
    holds only the pages loaded so far.
    """

    def __init__(
        self,
        connection: "SubstrateConnection",
        *,
        module: str,
        storage_function: str,
        params: list,
        block_hash: Optional[str],
        records: list[tuple[Any, Any]],
        last_key: Optional[str],
        page_size: int,
        max_results: Optional[int],
        ignore_decoding_errors: bool,
        exhausted: bool,
    ):
        self.records = records
        self._connection = connection
        self._module = module
        self._storage_function = storage_function
        self._params = params
        self._block_hash = block_hash
        self._last_key = last_key
        self._page_size = page_size
        self._max_results = max_results
        self._ignore_decoding_errors = ignore_decoding_errors
        self._exhausted = exhausted
        self._index = 0

    def __aiter__(self) -> AsyncIterator[tuple[Any, Any]]:
        return self

    async def __anext__(self) -> tuple[Any, Any]:
        if self._max_results is not None and self._index >= self._max_results:
            raise StopAsyncIteration
        if self._index >= len(self.records):
            if self._exhausted:
                raise StopAsyncIteration
            more = await self._connection._query_map_page(
                self._module,
                self._storage_function,
                self._params,
                block_hash=self._block_hash,
                page_size=self._page_size,
                start_key=self._last_key,
                ignore_decoding_errors=self._ignore_decoding_errors,
            )
            pairs, self._last_key, self._exhausted = more
            if not pairs:
                raise StopAsyncIteration
            self.records.extend(pairs)
        item = self.records[self._index]
        self._index += 1
        return item


class SubstrateConnection:
    def __init__(
        self,
        url: str,
        *,
        ss58_format: int = SS58_FORMAT,
        fallback_urls: Optional[list[str]] = None,
        retry_forever: bool = False,
        max_retries: int = 5,
        response_timeout: float = 60.0,
        connect_factory=None,
    ):
        """One connection to one chain endpoint (pool)."""
        self.url = url
        self.ss58_format = ss58_format
        self._session = RpcSession(
            url,
            fallback_urls=fallback_urls,
            retry_forever=retry_forever,
            max_retries=max_retries,
            response_timeout=response_timeout,
            connect_factory=connect_factory,
        )
        self._runtimes = RuntimeManager(self._session, ss58_format=ss58_format)
        self._nonces = NonceCache(self._session)
        self._block_hash_by_number: LRUCache = LRUCache(max_size=512)
        self._block_number_by_hash: LRUCache = LRUCache(max_size=512)
        # Finalized height, re-probed at most every _FINALIZED_TTL seconds;
        # gates the number->hash cache so a reorg can't serve stale fork hashes.
        self._finalized_height = -1
        self._finalized_checked_at = float("-inf")

    # -- lifecycle ---------------------------------------------------------------

    async def initialize(self) -> None:
        await self._session.connect()
        # Warm the head codec: every subsequent call needs it, and doing it
        # here surfaces connection/metadata problems at connect time.
        await self._runtimes.codec_at(None)

    async def close(self) -> None:
        await self._session.close()

    async def __aenter__(self) -> "SubstrateConnection":
        await self.initialize()
        return self

    async def __aexit__(self, *_exc) -> None:
        await self.close()

    @property
    def current_endpoint(self) -> str:
        return self._session.url

    # -- chain / block identity ---------------------------------------------------

    async def genesis_hash(self) -> str:
        return await self._runtimes.genesis_hash()

    async def runtime_spec_version(self) -> int:
        """The connected runtime's ``spec_version`` at the chain head.

        Served from the head codec, which re-validates against
        ``state_getRuntimeVersion`` on a short TTL — so a runtime upgrade
        mid-session is reflected within about a block.
        """
        return (await self._runtimes.codec_at(None)).spec_version

    async def get_chain_head(self) -> str:
        return await self._session.request("chain_getHead", [])

    async def get_chain_finalised_head(self) -> str:
        return await self._session.request("chain_getFinalizedHead", [])

    async def get_block_hash(self, block_id: Optional[int]) -> str:
        if block_id is None:
            return await self.get_chain_head()
        cached = self._block_hash_by_number.get(block_id)
        if cached is not None:
            return cached
        block_hash = await self._session.request("chain_getBlockHash", [block_id])
        if block_hash is None:
            raise BlockNotFound(f"no block at height {block_id}")
        # Only finalized number->hash mappings are cached: a hash above the
        # finalized head can still be reorged away.
        if block_id <= await self._known_finalized_height():
            self._block_hash_by_number.set(block_id, block_hash)
            self._block_number_by_hash.set(block_hash, block_id)
        return block_hash

    async def _known_finalized_height(self) -> int:
        """The finalized head height, cached for ``_FINALIZED_TTL`` seconds.

        A stale (lower) value is safe: it only makes the block-hash cache skip
        recently finalized blocks.
        """
        now = asyncio.get_running_loop().time()
        if now - self._finalized_checked_at >= _FINALIZED_TTL:
            head = await self.get_chain_finalised_head()
            header = await self._session.request("chain_getHeader", [head])
            if header is not None:
                self._finalized_height = int(header["number"], 16)
                self._finalized_checked_at = now
        return self._finalized_height

    async def get_block_number(self, block_hash: Optional[str] = None) -> int:
        if block_hash is not None:
            cached = self._block_number_by_hash.get(block_hash)
            if cached is not None:
                return cached
        header = await self._session.request("chain_getHeader", [block_hash])
        if header is None:
            raise BlockNotFound(f"no header for {block_hash}")
        number = int(header["number"], 16)
        if block_hash is not None:
            self._block_number_by_hash.set(block_hash, number)
            self._block_hash_by_number.set(number, block_hash)
        return number

    # -- storage reads -----------------------------------------------------------------

    async def query(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> Any:
        """One storage item, decoded to a plain value (misses follow the item's
        Default/Option semantics)."""
        codec = await self._runtimes.codec_at(block_hash)
        entry = codec.storage_entry(module, storage_function)
        params = params or []
        if len(params) != len(entry.param_types):
            raise ValueError(
                f"Storage function requires {len(entry.param_types)} parameters, "
                f"{len(params)} given"
            )
        key = codec.storage_key(entry, params)
        raw = await self._session.request("state_getStorageAt", ["0x" + key.hex(), block_hash])
        raw_bytes = bytes.fromhex(raw[2:]) if raw is not None else None
        return decode_storage_value(codec, entry, raw_bytes)

    async def query_batch(
        self,
        module: str,
        storage_function: str,
        params_list: list[list],
        block_hash: Optional[str] = None,
    ) -> list[Any]:
        """The same storage item for many keys in one RPC; results in input order."""
        if not params_list:
            return []
        if block_hash is None:
            block_hash = await self.get_chain_head()
        codec = await self._runtimes.codec_at(block_hash)
        entry = codec.storage_entry(module, storage_function)
        keys = codec.storage_key_batch(entry, params_list)
        key_hexes = ["0x" + key.hex() for key in keys]
        response = await self._session.request("state_queryStorageAt", [key_hexes, block_hash])
        by_key: dict[str, Optional[bytes]] = {}
        for group in response or []:
            for key_hex, value_hex in group["changes"]:
                by_key[key_hex] = bytes.fromhex(value_hex[2:]) if value_hex is not None else None
        raws = [by_key.get(key_hex) for key_hex in key_hexes]
        return decode_storage_values(codec, entry, raws)

    async def query_map(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
        *,
        page_size: int = 100,
        max_results: Optional[int] = None,
        ignore_decoding_errors: bool = False,
    ) -> QueryMapResult:
        """All (key, value) pairs of a storage map, paginated behind an async iterator."""
        params = params or []
        if max_results is not None and max_results < page_size:
            page_size = max_results
        pairs, last_key, exhausted = await self._query_map_page(
            module,
            storage_function,
            params,
            block_hash=block_hash,
            page_size=page_size,
            start_key=None,
            ignore_decoding_errors=ignore_decoding_errors,
        )
        return QueryMapResult(
            self,
            module=module,
            storage_function=storage_function,
            params=params,
            block_hash=block_hash,
            records=pairs,
            last_key=last_key,
            page_size=page_size,
            max_results=max_results,
            ignore_decoding_errors=ignore_decoding_errors,
            exhausted=exhausted,
        )

    async def _query_map_page(
        self,
        module: str,
        storage_function: str,
        params: list,
        *,
        block_hash: Optional[str],
        page_size: int,
        start_key: Optional[str],
        ignore_decoding_errors: bool,
    ) -> tuple[list[tuple[Any, Any]], Optional[str], bool]:
        """One page of a map query: (pairs, last_raw_key, exhausted)."""
        if block_hash is None:
            # Pin the head so keys and values are read at the same block; a
            # key deleted between the two RPCs would otherwise come back as a
            # None value.
            block_hash = await self.get_chain_head()
        codec = await self._runtimes.codec_at(block_hash)
        entry = codec.storage_entry(module, storage_function)
        if len(entry.param_types) == 0:
            raise ValueError("Given storage function is not a map")
        if len(params) > len(entry.param_types) - 1:
            raise ValueError(
                f"Storage function map can accept max {len(entry.param_types) - 1} "
                f"parameters, {len(params)} given"
            )
        prefix_hex = "0x" + codec.storage_key(entry, params).hex()
        keys = await self._session.request(
            "state_getKeysPaged", [prefix_hex, page_size, start_key or prefix_hex, block_hash]
        )
        keys = keys or []
        if not keys:
            return [], start_key, True
        response = await self._session.request("state_queryStorageAt", [keys, block_hash])
        changes: list[tuple[str, Optional[str]]] = []
        for group in response or []:
            changes.extend(group["changes"])
        pairs = decode_map_pairs(
            codec,
            entry,
            params,
            changes,
            ignore_decoding_errors=ignore_decoding_errors,
        )
        return pairs, keys[-1], len(keys) < page_size

    # -- runtime API / constants -----------------------------------------------------------

    async def runtime_call(
        self,
        api: str,
        method: str,
        params: Optional[list | dict] = None,
        block_hash: Optional[str] = None,
    ) -> Any:
        codec = await self._runtimes.codec_at(block_hash)
        return await call_runtime_api(self._session, codec, api, method, params, block_hash)

    async def get_constant(self, module: str, name: str, block_hash: Optional[str] = None) -> Any:
        codec = await self._runtimes.codec_at(block_hash)
        return codec.constant(module, name)

    # -- blocks and events ---------------------------------------------------------------------

    async def get_block(
        self,
        block_hash: Optional[str] = None,
        block_number: Optional[int] = None,
        *,
        ignore_decoding_errors: bool = False,
    ) -> Optional[BlockData]:
        """One block's header and decoded extrinsics."""
        if block_hash is not None and block_number is not None:
            raise ValueError("Provide either block_hash or block_number, not both")
        if block_number is not None:
            try:
                block_hash = await self.get_block_hash(block_number)
            except BlockNotFound:
                return None
        if block_hash is None:
            block_hash = await self.get_chain_head()
        raw = await self._session.request("chain_getBlock", [block_hash])
        if raw is None:
            return None
        codec = await self._runtimes.codec_at(block_hash)
        block = raw["block"]
        header = dict(block["header"])
        header["number"] = int(header["number"], 16)
        header["hash"] = block_hash
        extrinsics: list[Optional[dict]] = []
        for extrinsic_hex in block.get("extrinsics") or []:
            try:
                extrinsics.append(codec.decode_extrinsic(extrinsic_hex))
            except Exception:
                if not ignore_decoding_errors:
                    raise
                extrinsics.append(None)
        return BlockData(header=header, extrinsics=extrinsics)

    async def get_events(self, block_hash: Optional[str] = None) -> list[dict]:
        """Decoded ``System.Events`` records for a block."""
        events = await self.query("System", "Events", [], block_hash=block_hash)
        return list(events or [])

    async def subscribe_heads(self, *, finalized: bool = False) -> AsyncIterator[dict]:
        """Stream decoded block headers (``number`` as int) as they are produced.

        The iterator ends only by the caller breaking out (which unsubscribes)
        or raises if the connection drops mid-subscription.
        """
        prefix = "Finalized" if finalized else "New"
        subscription = await self._session.subscribe(
            f"chain_subscribe{prefix}Heads", [], f"chain_unsubscribe{prefix}Heads"
        )
        try:
            async for update in subscription:
                header = dict(update["result"])
                header["number"] = int(header["number"], 16)
                yield header
        finally:
            await subscription.unsubscribe()

    # -- extrinsics --------------------------------------------------------------------------------

    async def compose_call(
        self,
        call_module: str,
        call_function: str,
        call_params: Optional[dict] = None,
        block_hash: Optional[str] = None,
    ) -> Any:
        codec = await self._runtimes.codec_at(block_hash)
        return codec.compose_call(call_module, call_function, call_params or {})

    async def decode_call(self, call_data: bytes | str, block_hash: Optional[str] = None) -> Any:
        """Decode raw call bytes into a plain call dict (module/function/args)."""
        if isinstance(call_data, str):
            call_data = bytes.fromhex(call_data.removeprefix("0x"))
        codec = await self._runtimes.codec_at(block_hash)
        return codec.decode_call(call_data)

    async def decode_scale(
        self, type_string: str, data: bytes | str, block_hash: Optional[str] = None
    ) -> Any:
        """Decode arbitrary SCALE bytes (or 0x-hex) as ``type_string``."""
        if isinstance(data, str):
            data = bytes.fromhex(data.removeprefix("0x"))
        codec = await self._runtimes.codec_at(block_hash)
        return codec.decode(type_string, bytes(data))

    async def _normalize_era(self, era: Optional[dict | str]) -> tuple[dict | str, str]:
        """(normalized era, era block hash): immortal anchors at genesis, a
        mortal era without ``current`` anchors at the finalized head."""
        if not era:
            era = IMMORTAL
        if era == IMMORTAL:
            return era, await self.genesis_hash()
        if isinstance(era, dict):
            if "current" not in era and "phase" not in era:
                era = dict(era)
                era["current"] = await self.get_block_number(await self.get_chain_finalised_head())
            codec = await self._runtimes.codec_at(None)
            birth = codec.era_birth(era, era.get("current"))
            return era, await self.get_block_hash(birth)
        raise ValueError(f"unsupported era: {era!r}")

    async def create_signed_extrinsic(
        self,
        call: Any,
        keypair: Keypair,
        *,
        era: Optional[dict | str] = None,
        nonce: Optional[int] = None,
        tip: int = 0,
        tip_asset_id: Optional[int] = None,
        signature: Optional[bytes | str] = None,
    ) -> SignedExtrinsic:
        """Sign a composed call at the current runtime, for submission.

        With no ``nonce`` the account's next index is taken (and cached for
        pipelining); an explicit nonce is pinned into the cache. To sign
        without touching the nonce cache (fee estimation, offline vectors) use
        :meth:`sign_without_nonce_tracking`.
        """
        if nonce is None:
            nonce = await self._nonces.next_for(keypair.ss58_address)
        else:
            self._nonces.pin(keypair.ss58_address, nonce)
        return await self.sign_without_nonce_tracking(
            call,
            keypair,
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
            signature=signature,
        )

    async def sign_without_nonce_tracking(
        self,
        call: Any,
        keypair: Keypair,
        *,
        nonce: int,
        era: Optional[dict | str] = None,
        tip: int = 0,
        tip_asset_id: Optional[int] = None,
        signature: Optional[bytes | str] = None,
    ) -> SignedExtrinsic:
        """Sign at an explicit nonce without recording it in the nonce cache.

        For extrinsics that will not be submitted from this session (fee
        estimation, externally-submitted payloads, deterministic test vectors).
        """
        codec = await self._runtimes.codec_at(None)
        era, era_block_hash = await self._normalize_era(era)
        return await _sign_and_assemble(
            codec,
            call,
            keypair,
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
            genesis_hash=await self.genesis_hash(),
            era_block_hash=era_block_hash,
            signature=signature,
        )

    async def prepare_unsigned(
        self,
        call: Any,
        *,
        address: str,
        crypto_type: int = 1,
        era: Optional[dict | str] = None,
        nonce: Optional[int] = None,
        tip: int = 0,
        tip_asset_id: Optional[int] = None,
        metadata_hash: Optional[bytes] = None,
    ) -> UnsignedExtrinsic:
        """Build an :class:`UnsignedExtrinsic` for out-of-process signing.

        No key material is touched: the signing account is identified by its
        ``address`` alone (``crypto_type`` declares which scheme's signature
        will come back; a 65-byte signature overrides it). With no ``nonce``
        the account's next index is fetched fresh — never from the pipelining
        cache, since the signature may come back much later.

        ``metadata_hash`` enables the ``CheckMetadataHash`` extension with the
        given RFC-0078 digest, for devices that verify the runtime before
        signing.
        """
        codec = await self._runtimes.codec_at(None)
        if nonce is None:
            nonce = await self._nonces.next_for(address, use_cache=False)
        era, era_block_hash = await self._normalize_era(era)
        return _prepare_extrinsic(
            codec,
            call,
            address=address,
            public_key=bytes.fromhex(_ss58_decode(address, self.ss58_format)),
            crypto_type=crypto_type,
            era=era,
            nonce=nonce,
            tip=tip,
            tip_asset_id=tip_asset_id,
            genesis_hash=await self.genesis_hash(),
            era_block_hash=era_block_hash,
            metadata_hash=metadata_hash,
        )

    async def attach_signature(
        self, unsigned: UnsignedExtrinsic, signature: bytes | str
    ) -> SignedExtrinsic:
        """Assemble a submittable extrinsic from a prepared one plus its signature.

        Refuses when the runtime upgraded since preparation: the signature
        covers the old ``spec_version``, so the chain would reject it anyway —
        better to fail here with a reason than at submission with a cryptic
        ``BadProof``.
        """
        codec = await self._runtimes.codec_at(None)
        if codec.spec_version != unsigned.spec_version:
            raise SubstrateRequestException(
                f"runtime upgraded since the extrinsic was prepared "
                f"(spec {unsigned.spec_version} -> {codec.spec_version}); "
                "prepare and sign again"
            )
        return _attach_signature(codec, unsigned, signature)

    async def submit_extrinsic(
        self,
        extrinsic: SignedExtrinsic,
        *,
        wait_for_inclusion: bool = False,
        wait_for_finalization: bool = False,
    ) -> InclusionReport:
        """Submit; optionally watch until included/finalized and resolve the outcome."""
        if not (wait_for_inclusion or wait_for_finalization):
            result = await self._session.request("author_submitExtrinsic", [extrinsic.data_hex])
            return InclusionReport(extrinsic_hash=str(result))

        subscription = await self._session.subscribe(
            "author_submitAndWatchExtrinsic",
            [extrinsic.data_hex],
            "author_unwatchExtrinsic",
        )
        block_hash: Optional[str] = None
        finalized = False
        try:
            async for update in subscription:
                landed = watch_status_block(
                    update["result"],
                    wait_for_inclusion=wait_for_inclusion,
                    wait_for_finalization=wait_for_finalization,
                )
                if landed is not None:
                    block_hash, finalized = landed
                    break
        finally:
            await subscription.unsubscribe()

        if block_hash is None:
            raise SubstrateRequestException(
                f"extrinsic watch for {extrinsic.extrinsic_hash} ended without "
                "reaching the awaited state"
            )
        return await self._resolve_inclusion(extrinsic.extrinsic_hash, block_hash, finalized)

    async def resolve_extrinsic(
        self, extrinsic_hash: str, block_hash: str, *, finalized: bool = False
    ) -> InclusionReport:
        """Resolve the outcome of an extrinsic already included in ``block_hash``.

        Used to follow a MEV-shielded submission to its decrypted inner
        extrinsic, which the block author includes separately from the carrier
        that was watched. Raises ``ExtrinsicNotFound`` when the block does not
        carry the extrinsic.
        """
        return await self._resolve_inclusion(extrinsic_hash, block_hash, finalized)

    async def _resolve_inclusion(
        self, extrinsic_hash: str, block_hash: str, finalized: bool
    ) -> InclusionReport:
        """Locate the extrinsic in its block and resolve its outcome from events."""
        codec = await self._runtimes.codec_at(block_hash)
        raw = await self._session.request("chain_getBlock", [block_hash])
        block = raw["block"]
        block_number = int(block["header"]["number"], 16)
        extrinsic_idx: Optional[int] = None
        for idx, extrinsic_hex in enumerate(block.get("extrinsics") or []):
            data = bytes.fromhex(extrinsic_hex.removeprefix("0x"))
            if "0x" + blake2b(data, digest_size=32).hexdigest() == extrinsic_hash:
                extrinsic_idx = idx
                break
        if extrinsic_idx is None:
            # Never fabricate an outcome: with no index, the event filter below
            # would match the block's phase-less system events instead.
            raise ExtrinsicNotFound(
                f"extrinsic {extrinsic_hash} not found in block {block_hash} "
                f"(#{block_number}); the serving node may not have imported it yet"
            )
        events = await self.get_events(block_hash=block_hash)
        triggered = [e for e in events if e.get("extrinsic_idx") == extrinsic_idx]
        outcome = resolve_outcome(triggered, codec)
        return InclusionReport(
            extrinsic_hash=extrinsic_hash,
            finalized=finalized,
            block_hash=block_hash,
            block_number=block_number,
            extrinsic_idx=extrinsic_idx,
            is_success=outcome["is_success"],
            total_fee_amount=outcome["total_fee_amount"],
            weight=outcome["weight"],
            triggered_events=triggered,
            error_message=outcome["error_message"],
        )

    async def get_payment_info(self, call: Any, keypair: Keypair) -> dict:
        """Fee/weight estimation for a call (no valid signature required)."""
        nonce = await self._nonces.next_for(keypair.ss58_address, use_cache=False)
        extrinsic = await self.sign_without_nonce_tracking(
            call, keypair, nonce=nonce, signature=b"\x00" * 64
        )
        codec = await self._runtimes.codec_at(None)
        length = len(extrinsic.data)
        params_hex = (extrinsic.data + length.to_bytes(4, "little")).hex()
        raw = await self._session.request(
            "state_call", ["TransactionPaymentApi_query_info", params_hex, None]
        )
        return decode_runtime_api_result(
            codec,
            "TransactionPaymentApi",
            "query_info",
            bytes.fromhex(raw.removeprefix("0x")),
        )

    # -- nonces ----------------------------------------------------------------------------------

    async def get_account_next_index(self, address: str, *, use_cache: bool = True) -> int:
        return await self._nonces.next_for(address, use_cache=use_cache)

    def clear_nonce_cache_for_account(self, address: str) -> None:
        self._nonces.clear(address)

    # -- multisig ----------------------------------------------------------------------------------

    def generate_multisig_account(self, signatories: list[str], threshold: int) -> MultisigAccount:
        return _multisig_account(signatories, threshold, self.ss58_format)

    async def create_multisig_extrinsic(
        self,
        call: Any,
        keypair: Keypair,
        multisig_account: MultisigAccount,
        *,
        max_weight: Optional[dict | int] = None,
        era: Optional[dict] = None,
        nonce: Optional[int] = None,
    ) -> SignedExtrinsic:
        """One signatory's approval of ``call`` for a multisig account.

        Composes ``Multisig.as_multi`` with the full call (so the opening
        extrinsic embeds the call bytes on-chain), carrying the existing
        timepoint when the operation is already open.
        """
        codec = await self._runtimes.codec_at(None)
        if max_weight is None:
            payment_info = await self.get_payment_info(call, keypair)
            max_weight = payment_info["weight"]
        pending = await self.query(
            "Multisig",
            "Multisigs",
            [multisig_account.ss58_address, "0x" + codec.call_hash(call).hex()],
        )
        maybe_timepoint = pending["when"] if pending else None
        signer_address = _ss58_encode(bytes(keypair.public_key), self.ss58_format)
        other_signatories = [
            address for address in multisig_account.signatories if address != signer_address
        ]
        as_multi = codec.compose_call(
            "Multisig",
            "as_multi",
            {
                "other_signatories": other_signatories,
                "threshold": multisig_account.threshold,
                "maybe_timepoint": maybe_timepoint,
                "call": call,
                "max_weight": max_weight,
            },
        )
        return await self.create_signed_extrinsic(as_multi, keypair, era=era, nonce=nonce)

    # -- addresses ---------------------------------------------------------------------------------

    def ss58_encode(self, public_key: bytes | str) -> str:
        return _ss58_encode(public_key, self.ss58_format)

    def ss58_decode(self, address: str) -> str:
        return _ss58_decode(address, self.ss58_format)

    def is_valid_ss58_address(self, value: str) -> bool:
        return _is_valid_ss58_address(value, self.ss58_format)

    # -- metadata (codegen) ------------------------------------------------------------------------

    async def metadata_ir(self, block_hash: Optional[str] = None) -> MetadataIR:
        codec = await self._runtimes.codec_at(block_hash)
        return codec.metadata_ir()

    # -- escape hatch ------------------------------------------------------------------------------

    async def rpc_request(self, method: str, params: Optional[list] = None) -> Any:
        """Raw JSON-RPC request; returns the ``result``."""
        return await self._session.request(method, params or [])
