from __future__ import annotations

from typing import Any


def format_rpc_error(payload: Any) -> str:
    """Turn a JSON-RPC error payload (or plain message) into readable text."""
    if isinstance(payload, dict):
        if "error" in payload and isinstance(payload["error"], dict):
            err = payload["error"]
            message = str(err.get("message") or "Request failed")
            data = err.get("data")
            if data is None:
                return message
            if isinstance(data, str):
                return f"{message}: {data}"
            return f"{message}: {data!s}"
        return str(payload)
    return str(payload)


class SubstrateRequestException(Exception):
    """A chain/transport failure.

    Construct with either a plain message string or a JSON-RPC error payload
    dict (``{"error": {"message": ..., "data": ...}}``); either way ``str()``
    yields readable text.
    """

    def __init__(self, payload: Any):
        self.payload = payload if isinstance(payload, dict) else None
        super().__init__(format_rpc_error(payload))


class MaxRetriesExceeded(SubstrateRequestException):
    pass


class StateDiscardedError(SubstrateRequestException):
    def __init__(self, block_hash: str):
        self.block_hash = block_hash
        message = (
            f"State discarded for {block_hash}. This indicates the block is too old, and you "
            "should instead make this request using an archive node."
        )
        super().__init__(message)


class BlockNotFound(SubstrateRequestException):
    """The requested block does not exist (or the node has not imported it)."""


class ExtrinsicNotFound(SubstrateRequestException):
    """The extrinsic's hash was not found in the block it was reported in."""


class StorageFunctionNotFound(ValueError):
    pass
