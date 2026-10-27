"""Intent registry and tool manifest.

Every intent registers itself here via ``@register``. Because intents are
self-describing (schema + summary), the agent-facing tool catalog
(``list_tools``/``build``) is generated from this registry rather than
hand-maintained, so it can never drift from the actual operations.
"""

from __future__ import annotations

from typing import Any, Type, TypeVar

from ..settings import tx_docs_url
from .base import Intent

REGISTRY: dict[str, Type[Intent]] = {}

IntentT = TypeVar("IntentT", bound=Intent)


def register(cls: Type[IntentT]) -> Type[IntentT]:
    """Class decorator: add an intent to the registry keyed by its ``op``.

    Building the schema here fails registration (import time) if a field has an
    annotation the schema generator can't map, rather than shipping a wrong schema.
    """
    if cls.op in REGISTRY:
        raise ValueError(f"Duplicate intent op: {cls.op}")
    cls.json_schema()
    REGISTRY[cls.op] = cls
    return cls


def build(op: str, args: dict[str, Any]) -> Intent:
    """Construct an intent from an op name and a plain dict of arguments."""
    try:
        cls = REGISTRY[op]
    except KeyError:
        raise ValueError(f"Unknown op {op!r}. Known ops: {sorted(REGISTRY)}") from None
    return cls.from_args(args)


def list_tools() -> list[dict[str, Any]]:
    """Machine-readable catalog of every operation, for agent/tool discovery.

    ``summary`` is the docstring's first line (for listings); ``description``
    is the full docstring, which carries the implications an agent needs
    (deposits, irreversibility, signer, when to use).
    """
    tools = []
    for op, cls in sorted(REGISTRY.items()):
        description = cls.describe()
        tools.append(
            {
                "name": op,
                "summary": description.split("\n")[0],
                "description": description,
                "signer": cls.signer,
                "origin": cls.origin,
                "verify": cls.verify,
                "wraps": [list(pair) for pair in cls.wraps],
                "docs_url": tx_docs_url(op),
                "input_schema": cls.json_schema(),
            }
        )
    return tools
