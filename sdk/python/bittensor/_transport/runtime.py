"""Runtime resolution: which codec decodes a given block, and codec caching.

A block's state is written by the runtime of its *parent* block, so resolution
goes block hash -> parent hash -> spec version -> :class:`RuntimeCodec`. Codecs
are cached in memory per spec version, and the raw metadata bytes are cached on
disk keyed by (genesis hash, spec version) so short-lived processes skip the
metadata download.

Metadata is fetched as V15 via the ``Metadata_metadata_at_version`` runtime
call; runtimes that predate it (old archive blocks) fall back to
``state_getMetadata`` (V14).
"""

from __future__ import annotations

import asyncio
import logging
import os
import struct
import tempfile
from contextlib import suppress
from pathlib import Path
from typing import Optional

from .codec import RuntimeCodec, strip_option_opaque_metadata
from .const import SS58_FORMAT
from .errors import BlockNotFound, SubstrateRequestException
from .lru import LRUCache
from .rpc import RpcSession

logger = logging.getLogger("bittensor.transport")

_V15_VERSION_HEX = "0x0f000000"
_V15_MISSING_NEEDLE = "Exported method Metadata_metadata_at_version is not found"

# Raw-bytes disk cache format: magic + flags/versions header + metadata blob.
_DISK_MAGIC = b"STMD"
_DISK_FORMAT_VERSION = 1
_DISK_CACHE_MAX_FILES = 8


def _transaction_version(runtime_info: dict) -> int:
    """The runtime's transactionVersion, required for a signing-capable codec.

    Defaulting a missing field to 0 would only surface much later as an opaque
    ``BadProof`` at submission time, so refuse to build the codec instead.
    """
    version = runtime_info.get("transactionVersion")
    if version is None:
        raise SubstrateRequestException(
            "state_getRuntimeVersion returned no transactionVersion; "
            "cannot build a codec that signs correctly"
        )
    return int(version)


def _disk_cache_dir() -> Path:
    override = os.getenv("BITTENSOR_RUNTIME_CACHE_DIR")
    if override:
        return Path(override)
    return Path.home() / ".bittensor" / "runtime-cache"


def _disk_cache_path(genesis_hash: str, spec_version: int) -> Path:
    chain_id = genesis_hash.removeprefix("0x")[:16]
    return _disk_cache_dir() / f"md{_DISK_FORMAT_VERSION}-{chain_id}-{spec_version}.bin"


def _load_metadata_from_disk(
    genesis_hash: str, spec_version: int
) -> Optional[tuple[bytes, int, bool]]:
    """(metadata_bytes, transaction_version, is_v15) from disk, or None."""
    path = _disk_cache_path(genesis_hash, spec_version)
    try:
        blob = path.read_bytes()
    except OSError:
        return None
    try:
        if blob[:4] != _DISK_MAGIC:
            raise ValueError("bad magic")
        tx_version, is_v15 = struct.unpack_from("<IB", blob, 4)
        return blob[9:], tx_version, bool(is_v15)
    except Exception as error:
        logger.debug(f"Discarding unreadable metadata cache {path}: {error}")
        with suppress(OSError):
            path.unlink()
        return None


def _save_metadata_to_disk(
    genesis_hash: str,
    spec_version: int,
    metadata_bytes: bytes,
    transaction_version: int,
    is_v15: bool,
) -> None:
    path = _disk_cache_path(genesis_hash, spec_version)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        header = _DISK_MAGIC + struct.pack("<IB", transaction_version, int(is_v15))
        # Atomic write: a concurrent reader never sees a half-written file.
        fd, tmp_name = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
        try:
            with os.fdopen(fd, "wb") as f:
                f.write(header + metadata_bytes)
            os.replace(tmp_name, path)
        except BaseException:
            with suppress(OSError):
                os.unlink(tmp_name)
            raise
        _prune_disk_cache(path.parent)
    except Exception as error:
        logger.debug(f"Could not write metadata cache {path}: {error}")


def _prune_disk_cache(directory: Path) -> None:
    files = sorted(directory.glob("md*.bin"), key=lambda p: p.stat().st_mtime, reverse=True)
    for stale in files[_DISK_CACHE_MAX_FILES:]:
        with suppress(OSError):
            stale.unlink()


class RuntimeManager:
    def __init__(
        self,
        session: RpcSession,
        *,
        ss58_format: int = SS58_FORMAT,
        codec_cache_size: int = 4,
        spec_cache_size: int = 512,
    ):
        self._session = session
        self._ss58_format = ss58_format
        self._codecs: LRUCache = LRUCache(max_size=codec_cache_size)  # spec -> RuntimeCodec
        self._spec_by_block_hash: LRUCache = LRUCache(max_size=spec_cache_size)
        self._genesis_hash: Optional[str] = None
        self._inflight: dict[int, asyncio.Future] = {}
        self._head_codec: Optional[RuntimeCodec] = None
        self._head_checked_at: float = float("-inf")
        # How long the head codec is trusted before re-probing the node's
        # runtime version (one cheap RPC). One mainnet block keeps staleness
        # across a runtime upgrade to at most a block or so.
        self._head_ttl = 12.0

    async def genesis_hash(self) -> str:
        if self._genesis_hash is None:
            self._genesis_hash = await self._session.request("chain_getBlockHash", [0])
        return self._genesis_hash

    async def codec_at(self, block_hash: Optional[str]) -> RuntimeCodec:
        """The codec for the runtime governing ``block_hash`` (None = chain head).

        The chain-head case is served from a cached head codec, re-validated at
        most every ``_head_ttl`` seconds against ``state_getRuntimeVersion`` so
        a runtime upgrade mid-session is picked up within about a block.
        """
        if block_hash is not None:
            return await self._codec_for_block_hash(block_hash)
        now = asyncio.get_running_loop().time()
        if self._head_codec is not None and now - self._head_checked_at < self._head_ttl:
            return self._head_codec
        runtime_info = await self._session.request("state_getRuntimeVersion", [None])
        self._head_checked_at = now
        spec_version = runtime_info["specVersion"]
        if self._head_codec is not None and self._head_codec.spec_version == spec_version:
            return self._head_codec
        codec = self._codecs.get(spec_version)
        if codec is None:
            codec = await self._fetch_codec(
                spec_version,
                _transaction_version(runtime_info),
                None,
                spec_name=str(runtime_info.get("specName") or ""),
            )
        self._head_codec = codec
        return codec

    async def _codec_for_block_hash(self, block_hash: str) -> RuntimeCodec:
        spec_version = self._spec_by_block_hash.get(block_hash)
        if spec_version is not None:
            codec = self._codecs.get(spec_version)
            if codec is not None:
                return codec
        parent_hash = await self._parent_hash(block_hash)
        runtime_info = await self._session.request("state_getRuntimeVersion", [parent_hash])
        if runtime_info is None:
            raise SubstrateRequestException(f"No runtime information for block '{block_hash}'")
        spec_version = runtime_info["specVersion"]
        self._spec_by_block_hash.set(block_hash, spec_version)
        codec = self._codecs.get(spec_version)
        if codec is not None:
            return codec
        return await self._fetch_codec(
            spec_version,
            _transaction_version(runtime_info),
            parent_hash,
            spec_name=str(runtime_info.get("specName") or ""),
        )

    async def _parent_hash(self, block_hash: str) -> str:
        header = await self._session.request("chain_getHeader", [block_hash])
        if header is None:
            raise BlockNotFound(f'Block not found for "{block_hash}"')
        parent = header["parentHash"]
        if int(parent, 16) == 0:  # genesis has no parent; use itself
            return block_hash
        return parent

    async def _fetch_codec(
        self,
        spec_version: int,
        transaction_version: int,
        at_hash: Optional[str],
        *,
        spec_name: str = "",
    ) -> RuntimeCodec:
        # Concurrent callers for the same spec share one fetch.
        existing = self._inflight.get(spec_version)
        if existing is not None:
            return await existing
        loop = asyncio.get_running_loop()
        future: asyncio.Future = loop.create_future()
        self._inflight[spec_version] = future
        try:
            codec = await self._build_codec(
                spec_version, transaction_version, at_hash, spec_name=spec_name
            )
            self._codecs.set(spec_version, codec)
            future.set_result(codec)
            return codec
        except BaseException as error:
            if not future.done():
                future.set_exception(error)
                # Consume the exception so a lone fetcher doesn't warn.
                future.exception()
            raise
        finally:
            self._inflight.pop(spec_version, None)

    async def _build_codec(
        self,
        spec_version: int,
        transaction_version: int,
        at_hash: Optional[str],
        *,
        spec_name: str = "",
    ) -> RuntimeCodec:
        genesis = await self.genesis_hash()
        cached = _load_metadata_from_disk(genesis, spec_version)
        if cached is not None:
            metadata_bytes, tx_version, _is_v15 = cached
            try:
                return self._make_codec(
                    metadata_bytes, spec_version, tx_version, spec_name=spec_name
                )
            except Exception as error:
                # A cache entry whose body fails to decode would otherwise wedge
                # this spec version forever; discard it and download fresh.
                logger.warning(
                    f"Runtime {spec_version}: cached metadata failed to decode "
                    f"({error}); discarding cache and re-downloading"
                )
                with suppress(OSError):
                    _disk_cache_path(genesis, spec_version).unlink()
        metadata_bytes, is_v15 = await self._download_metadata(at_hash)
        codec = self._make_codec(
            metadata_bytes, spec_version, transaction_version, spec_name=spec_name
        )
        _save_metadata_to_disk(genesis, spec_version, metadata_bytes, transaction_version, is_v15)
        logger.debug(
            f"Runtime {spec_version}: metadata downloaded "
            f"({'v15' if is_v15 else 'v14'}, {len(metadata_bytes)} bytes)"
        )
        return codec

    def _make_codec(
        self,
        metadata_bytes: bytes,
        spec_version: int,
        transaction_version: int,
        *,
        spec_name: str = "",
    ) -> RuntimeCodec:
        return RuntimeCodec(
            metadata_bytes,
            spec_version=spec_version,
            transaction_version=transaction_version,
            spec_name=spec_name,
            ss58_format=self._ss58_format,
        )

    async def _download_metadata(self, at_hash: Optional[str]) -> tuple[bytes, bool]:
        """(raw MetadataVersioned bytes, is_v15) from the node."""
        try:
            result = await self._session.request(
                "state_call", ["Metadata_metadata_at_version", _V15_VERSION_HEX, at_hash]
            )
        except SubstrateRequestException as error:
            if _V15_MISSING_NEEDLE not in str(error):
                raise
            result = None
        if result is not None:
            inner = strip_option_opaque_metadata(bytes.fromhex(result.removeprefix("0x")))
            if inner is not None:
                return inner, True
        # Pre-V15 runtime: the legacy metadata RPC returns the V14 blob directly.
        legacy = await self._session.request("state_getMetadata", [at_hash])
        if not legacy:
            raise SubstrateRequestException(f"No metadata for block '{at_hash}'")
        return bytes.fromhex(legacy.removeprefix("0x")), False
