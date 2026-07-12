"""Emit ``bittensor/namespaces.pyi`` from the read registry.

The runtime module (``bittensor/namespaces.py``) resolves registry reads
dynamically via ``__getattr__``; this emitter makes that surface visible to
type checkers and editors by generating a stub that declares, per namespace:

- every hand-written (curated) method, introspected from the class itself, and
- every registry read in the namespace's category (unless shadowed by a
  curated method), with the wrapper's keyword-only ``block=`` pin appended.

The stub is committed; ``python -m codegen.check --namespaces`` fails CI when
it drifts from the registry. Regenerate with ``python -m codegen.emit_namespaces``.
"""

from __future__ import annotations

import inspect
import re
import sys
from pathlib import Path
from typing import Any

from bittensor import namespaces
from bittensor.namespaces import NAMESPACES, _accepts_pin
from bittensor.reads.base import REGISTRY, ReadSpec

OUT_PATH = Path(__file__).resolve().parent.parent / "bittensor" / "namespaces.pyi"

HEADER = '''"""Typed surface of the read namespaces. GENERATED — DO NOT EDIT.

Generated from the read registry by ``python -m codegen.emit_namespaces``;
``python -m codegen.check --namespaces`` gates drift in CI. The runtime
implementation lives in ``namespaces.py`` (curated methods + a ``__getattr__``
that dispatches any registry read).
"""
'''

# Where the stub imports each domain type from. The emitter fails loudly on an
# annotation whose type is not listed, so a new read's return type must be
# deliberately mapped before the stub can regenerate.
_TYPE_MODULES = (
    "bittensor.balance",
    "bittensor.metagraph",
    "bittensor.reads.delegation",
    "bittensor.reads.identity",
    "bittensor.reads.neurons",
    "bittensor.reads.prices",
    "bittensor.reads.staking",
    "bittensor.reads.subnets",
)

_TYPING_NAMES = {"Optional", "Any"}
_BUILTIN_NAMES = {"list", "dict", "tuple", "set", "str", "int", "float", "bool", "bytes", "None"}


def _type_sources() -> dict[str, str]:
    """Public class name -> module path, for resolving annotation identifiers."""
    sources: dict[str, str] = {}
    for module_path in _TYPE_MODULES:
        module = sys.modules[module_path]
        for name, obj in vars(module).items():
            if inspect.isclass(obj) and not name.startswith("_") and obj.__module__ == module_path:
                sources.setdefault(name, module_path)
    return sources


def _identifiers(annotation: str) -> set[str]:
    return set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", annotation))


class _Stub:
    def __init__(self):
        self._sources = _type_sources()
        self.typing_used: set[str] = set()
        self.imports: dict[str, set[str]] = {}  # module path -> names
        self.lines: list[str] = []

    def note_annotation(self, annotation: str) -> None:
        for ident in _identifiers(annotation):
            if ident in _BUILTIN_NAMES:
                continue
            if ident in _TYPING_NAMES:
                self.typing_used.add(ident)
            elif ident in self._sources:
                self.imports.setdefault(self._sources[ident], set()).add(ident)
            else:
                raise ValueError(
                    f"annotation {annotation!r} uses {ident!r}, which no module in "
                    "codegen.emit_namespaces._TYPE_MODULES exports — add its module there"
                )

    def render_imports(self) -> list[str]:
        lines = []
        if self.typing_used:
            lines.append(f"from typing import {', '.join(sorted(self.typing_used | {'Any'}))}")
        else:
            lines.append("from typing import Any")
        for module_path in sorted(self.imports):
            names = ", ".join(sorted(self.imports[module_path]))
            relative = "." + module_path.removeprefix("bittensor.")
            lines.append(f"from {relative} import {names}")
        return lines


def _annotation_str(value: Any) -> str:
    """Annotations are strings under ``from __future__ import annotations``."""
    if isinstance(value, str):
        return value
    raise ValueError(f"expected a string annotation, got {value!r}")


def _render_params(stub: _Stub, params: list[inspect.Parameter]) -> str:
    parts = ["self"]
    star_emitted = False
    for p in params:
        if p.kind is inspect.Parameter.KEYWORD_ONLY and not star_emitted:
            parts.append("*")
            star_emitted = True
        piece = p.name
        if p.annotation is not inspect.Parameter.empty:
            annotation = _annotation_str(p.annotation)
            stub.note_annotation(annotation)
            piece += f": {annotation}"
            if p.default is not inspect.Parameter.empty:
                piece += f" = {p.default!r}"
        elif p.default is not inspect.Parameter.empty:
            piece += f"={p.default!r}"
        parts.append(piece)
    return ", ".join(parts)


def _render_method(
    stub: _Stub, name: str, params: list[inspect.Parameter], returns: str, doc: str
) -> list[str]:
    stub.note_annotation(returns)
    lines = [f"    async def {name}({_render_params(stub, params)}) -> {returns}:"]
    body = doc.strip().replace("\\", "\\\\").replace('"""', r"\"\"\"")
    if not body:
        lines.append("        ...")
    elif "\n" not in body:
        lines.append(f'        """{body}"""')
    else:
        first, *rest = body.split("\n")
        lines.append(f'        """{first}')
        lines.extend(f"        {line}".rstrip() for line in rest)
        lines.append('        """')
    return lines


def _curated_methods(cls: type) -> dict[str, Any]:
    """Hand-written public coroutine methods, in definition order."""
    return {
        name: fn
        for name, fn in vars(cls).items()
        if inspect.iscoroutinefunction(fn) and not name.startswith("_")
    }


def _dynamic_params(spec: ReadSpec) -> list[inspect.Parameter]:
    params = [p for p in inspect.signature(spec.fetch).parameters.values() if p.name != "view"]
    if _accepts_pin(spec):
        params.append(
            inspect.Parameter(
                "block",
                inspect.Parameter.KEYWORD_ONLY,
                annotation="Optional[int]",
                default=None,
            )
        )
    return params


def generate() -> str:
    stub = _Stub()
    class_blocks: list[str] = []

    base_lines = [
        "class _ReadNamespace:",
        "    def __init__(self, view: Any) -> None: ...",
    ]
    class_blocks.append("\n".join(base_lines))

    for cls in NAMESPACES.values():
        lines = [f"class {cls.__name__}(_ReadNamespace):"]
        doc = inspect.cleandoc(cls.__doc__ or "").split("\n")[0]
        if doc:
            lines.append(f'    """{doc}"""')
        curated = _curated_methods(cls)
        for name, fn in curated.items():
            sig = inspect.signature(fn)
            params = [p for p in sig.parameters.values() if p.name != "self"]
            returns = _annotation_str(sig.return_annotation)
            lines.append("")
            lines.extend(
                _render_method(stub, name, params, returns, inspect.cleandoc(fn.__doc__ or ""))
            )
        reads = sorted(
            (spec for spec in REGISTRY.values() if spec.category == cls._category),
            key=lambda spec: spec.name,
        )
        for spec in reads:
            if spec.name in curated:
                continue  # curated method shadows the registry read
            returns = _annotation_str(inspect.signature(spec.fetch).return_annotation)
            lines.append("")
            lines.extend(_render_method(stub, spec.name, _dynamic_params(spec), returns, spec.doc))
        class_blocks.append("\n".join(lines))

    footer = (
        "NAMESPACES: dict[str, type[_ReadNamespace]]\n"
        "\n"
        "async def _scoped(view: Any, block: Optional[int]) -> Any: ...\n"
        "def _accepts_pin(spec: Any) -> bool: ...\n"
    )
    stub.typing_used.add("Optional")

    covered = {cls._category for cls in NAMESPACES.values()}
    stray = sorted({spec.category for spec in REGISTRY.values()} - covered)
    if stray:
        raise ValueError(
            f"read categories with no namespace class in bittensor.namespaces: {stray}"
        )

    parts = [HEADER, "\n".join(stub.render_imports()), *class_blocks, footer]
    return "\n\n".join(parts).rstrip() + "\n"


def main() -> None:
    _ = namespaces  # imported for its side effect of registering every read
    content = generate()
    OUT_PATH.write_text(content)
    print(f"wrote {OUT_PATH}")


if __name__ == "__main__":
    main()
