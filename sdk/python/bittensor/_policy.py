"""Lossless adapters for the portable policy shared by all SDK bindings."""

from __future__ import annotations

import json
from typing import Any

import bittensor_core


def _json_value(value: Any) -> Any:
    if isinstance(value, (bytes, bytearray, memoryview)):
        return "0x" + bytes(value).hex()
    if isinstance(value, dict):
        return {str(key): _json_value(child) for key, child in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_value(child) for child in value]
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value) if abs(value) > 2**53 - 1 else value
    return value


def evaluate_policy(operation: str, value: Any) -> Any:
    response = json.loads(
        bittensor_core.policy_evaluate(operation, json.dumps(_json_value(value), allow_nan=False))
    )
    if response.get("version") != 1:
        raise RuntimeError("incompatible Rust SDK policy version")
    if error := response.get("error"):
        kind = TypeError if error["kind"] == "type" else ValueError
        raise kind(error["message"])
    return response["value"]


def evaluate_policy_preserving_values(operation: str, value: Any) -> Any:
    """Adapt opaque codec scalars for structural policies, never arithmetic rules."""
    strings: set[str] = set()

    def visit(item: Any, depth: int = 0) -> None:
        if depth > 128:
            raise ValueError("policy value nesting exceeds 128")
        if isinstance(item, str):
            strings.add(item)
        elif isinstance(item, dict):
            for key, child in item.items():
                strings.add(str(key))
                visit(child, depth + 1)
        elif isinstance(item, (list, tuple)):
            for child in item:
                visit(child, depth + 1)

    visit(value)
    prefix = "__bittensor_policy_value__"
    while any(text.startswith(prefix) for text in strings):
        prefix += "_"
    scalars: dict[str, Any] = {}

    def encode(item: Any) -> Any:
        if isinstance(item, (bytes, bytearray, memoryview)) or (
            isinstance(item, int) and not isinstance(item, bool) and abs(item) > 2**53 - 1
        ):
            token = prefix + str(len(scalars))
            scalars[token] = item
            return token
        if isinstance(item, dict):
            return {key: encode(child) for key, child in item.items()}
        if isinstance(item, (list, tuple)):
            return [encode(child) for child in item]
        return item

    def decode(item: Any) -> Any:
        if isinstance(item, str) and item in scalars:
            return scalars[item]
        if isinstance(item, dict):
            return {key: decode(child) for key, child in item.items()}
        if isinstance(item, list):
            return [decode(child) for child in item]
        return item

    return decode(evaluate_policy(operation, encode(value)))
